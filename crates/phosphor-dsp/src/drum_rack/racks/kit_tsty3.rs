//! TSTY3 kit synthesis.

use super::super::*;
use std::f64::consts::TAU;

impl DrumVoice {
    // TSTY-3: Studio acoustic kit — 88 unique sounds, NO filler
    // Dispatches on MIDI note directly. Every sound is a unique synthesis.
    // Modeled after close-mic'd drums through Studer A800 reel-to-reel.
    // ══════════════════════════════════════════════════════════════════════════

    pub(crate) fn synth_tsty3(&mut self, sr: f64, dm: f64, tm: f64, nm: f64, _dr: f64) -> f64 {
        let raw = self.t3_dispatch(sr, dm, tm, nm);
        // Every tsty-3 sound goes through tape processing
        Self::tape_process(raw, self.time, sr, &mut self.lp1_state)
    }

    pub(crate) fn t3_dispatch(&mut self, sr: f64, dm: f64, tm: f64, nm: f64) -> f64 {
        let t = self.time;
        let n = self.note;
        match n {
            // ══ KICKS: 24-38 (15 unique kicks) ══
            // Each has different fundamental, beater, damping, and modal content

            24 => { // Kick: Studio Felt — warm, round, 60Hz, felt beater, medium damping
                let f = 60.0 * tm;
                let sw = f * 0.25 * (-t * 55.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                let rise = (t / 0.0018).min(1.0);
                let body = osc_sine(self.phase1) * 0.6 * (0.3*(-t/0.01).exp() + 0.7*(-t/(0.22*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.06*dm)).exp();
                let beater = self.noise() * rise * (-t*180.0).exp();
                let bf = self.svf1.bandpass(beater, 2200.0, 1.3, sr) * 0.15;
                body + m1 + bf
            }
            25 => { // Kick: Tight Funk — 72Hz, damped, wood beater, dry
                let f = 72.0 * tm;
                let sw = f * 0.18 * (-t * 90.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.7 * (0.5*(-t/0.005).exp() + 0.5*(-t/(0.13*dm)).exp());
                let beater = self.noise() * (t/0.0008).min(1.0) * (-t*450.0).exp();
                let bf = self.svf1.bandpass(beater, 4200.0, 2.0, sr) * 0.25;
                body + bf
            }
            26 => { // Kick: Jazz Brushed — 52Hz, resonant, slow sweep, soft
                let f = 52.0 * tm;
                let sw = f * 0.45 * (-t * 30.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 2.296, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.2*(-t/0.018).exp() + 0.8*(-t/(0.4*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.08 * (-t/(0.1*dm)).exp();
                let brush = self.noise() * (t/0.003).min(1.0) * (-t*100.0).exp();
                let bf = self.svf1.bandpass(brush, 1500.0, 0.9, sr) * 0.1;
                body + m1 + bf
            }
            27 => { // Kick: Deep Sub — 42Hz, very long decay, minimal click
                let f = 42.0 * tm;
                let sw = f * 0.5 * (-t * 20.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f * 0.5) + sw * 0.3, sr);
                let body = osc_sine(self.phase1) * 0.55 * (-t/(0.5*dm)).exp();
                let sub = osc_sine(self.phase2) * 0.25 * (-t/(0.35*dm)).exp();
                body + sub
            }
            28 => { // Kick: Rock Plastic — 65Hz, bright beater click, medium body
                let f = 65.0 * tm;
                let sw = f * 0.35 * (-t * 60.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                advance_phase(&mut self.phase3, (f + sw) * 2.136, sr);
                let env = 0.4*(-t/0.006).exp() + 0.6*(-t/(0.2*dm)).exp();
                let body = osc_sine(self.phase1) * 0.6 * env;
                let m1 = osc_sine(self.phase2) * 0.12 * (-t/(0.05*dm)).exp();
                let m2 = osc_sine(self.phase3) * 0.06 * (-t/(0.03*dm)).exp();
                let beater = self.noise() * (t/0.0005).min(1.0) * (-t*600.0).exp();
                let bf = self.svf1.bandpass(beater, 5500.0, 2.2, sr) * 0.3;
                body + m1 + m2 + bf
            }
            29 => { // Kick: Boomy Floor — 48Hz, long, shell resonance dominant
                let f = 48.0 * tm;
                let sw = f * 0.3 * (-t * 25.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.45*dm)).exp();
                // Shell resonance via resonant filter
                let shell_exc = self.noise() * (-t*40.0).exp();
                let shell = self.svf1.bandpass(shell_exc, 220.0*tm, 15.0, sr) * 0.12;
                let beater = self.noise() * (t/0.002).min(1.0) * (-t*120.0).exp();
                let bf = self.svf2.bandpass(beater, 1800.0, 1.0, sr) * 0.08;
                body + shell + bf
            }
            30 => { // Kick: Tight Click — 78Hz, very short, lots of attack
                let f = 78.0 * tm;
                let sw = f * 0.15 * (-t * 120.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.55 * (0.6*(-t/0.003).exp() + 0.4*(-t/(0.08*dm)).exp());
                let click = self.noise() * (t/0.0003).min(1.0) * (-t*900.0).exp();
                let cf = self.hp1.tick_hp(click, 3000.0, sr) * 0.35;
                body + cf
            }
            31 => { // Kick: Warm Vintage — 58Hz, triangle body, soft character
                let f = 58.0 * tm;
                let sw = f * 0.3 * (-t * 45.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_triangle(self.phase1) * 0.55 * (0.25*(-t/0.012).exp() + 0.75*(-t/(0.28*dm)).exp());
                let warmth = osc_sine(self.phase1 * 0.5) * 0.12 * (-t/(0.2*dm)).exp();
                let felt = self.noise() * (t/0.002).min(1.0) * (-t*150.0).exp();
                let ff = self.svf1.bandpass(felt, 1800.0, 1.2, sr) * 0.08;
                body + warmth + ff
            }
            32 => { // Kick: Punchy Mid — 68Hz, strong 2nd mode, snappy
                let f = 68.0 * tm;
                let sw = f * 0.4 * (-t * 75.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.45*(-t/0.005).exp() + 0.55*(-t/(0.18*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.2 * (-t/(0.07*dm)).exp(); // strong 2nd mode
                let beater = self.noise() * (t/0.001).min(1.0) * (-t*350.0).exp();
                let bf = self.svf1.bandpass(beater, 3200.0, 1.6, sr) * 0.18;
                body + m1 + bf
            }
            33 => { // Kick: Thuddy — 55Hz, very damped, almost no ring
                let f = 55.0 * tm;
                let sw = f * 0.2 * (-t * 80.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.65 * (-t/(0.1*dm)).exp();
                let thud = self.noise() * (t/0.001).min(1.0) * (-t*200.0).exp();
                let tf = self.svf1.lowpass(thud, 800.0, 0.5, sr) * 0.15;
                body + tf
            }
            34 => { // Kick: Room — 62Hz, emphasis on shell + room reflections
                let f = 62.0 * tm;
                let sw = f * 0.3 * (-t * 50.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.45 * (0.3*(-t/0.008).exp() + 0.7*(-t/(0.25*dm)).exp());
                // "Room" via delayed noise burst
                let room = self.noise() * (-(t-0.015).max(0.0) * 30.0).exp() * 0.08;
                let rf = self.svf1.lowpass(room, 2000.0, 1.5, sr);
                let shell = self.noise() * (-t*50.0).exp();
                let sf = self.svf2.bandpass(shell, 300.0*tm, 10.0, sr) * 0.07;
                body + rf + sf
            }
            35 => { // Kick: Muffled — 50Hz, pillow inside, almost pure fundamental
                let f = 50.0 * tm;
                let sw = f * 0.15 * (-t * 40.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                let body = osc_sine(self.phase1) * 0.7 * (-t/(0.15*dm)).exp();
                body
            }
            36 => { // Kick: Studio Standard — balanced, 64Hz, all-purpose
                let f = 64.0 * tm;
                let sw = f * 0.28 * (-t * 58.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                let env = 0.35*(-t/0.007).exp() + 0.65*(-t/(0.2*dm)).exp();
                let body = osc_sine(self.phase1) * 0.6 * env;
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.055*dm)).exp();
                let sub = osc_sine(self.phase1 * 0.5) * 0.1 * (-t/(0.12*dm)).exp();
                let beater = self.noise() * (t/0.001).min(1.0) * (-t*300.0).exp();
                let bf = self.svf1.bandpass(beater, 3000.0, 1.5, sr) * 0.18;
                body + m1 + sub + bf
            }
            37 => { // Kick: Ringy — 56Hz, long undamped, 3 audible modes
                let f = 56.0 * tm;
                let sw = f * 0.35 * (-t * 35.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, (f + sw) * 1.593, sr);
                advance_phase(&mut self.phase3, (f + sw) * 2.296, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.4*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.18 * (-t/(0.15*dm)).exp();
                let m2 = osc_sine(self.phase3) * 0.1 * (-t/(0.1*dm)).exp();
                body + m1 + m2
            }
            38 => { // Kick: Chest Hit — 45Hz, max sub, air push
                let f = 45.0 * tm;
                let sw = f * 0.6 * (-t * 22.0).exp();
                advance_phase(&mut self.phase1, f + sw, sr);
                advance_phase(&mut self.phase2, f * 0.5, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.35*dm)).exp();
                let air = osc_sine(self.phase2) * 0.3 * (-t/(0.15*dm)).exp(); // sub push
                let thump = self.noise() * (-t*60.0).exp();
                let tf = self.svf1.lowpass(thump, 500.0, 0.8, sr) * 0.1;
                body + air + tf
            }

            // ══ SNARES: 39-53 (15 unique snares) ══

            39 => { // Snare: Funk Tight — 310Hz, short wires, crisp
                let f = 310.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                let stick = self.noise() * (t/0.0003).min(1.0) * (-t*1800.0).exp();
                let sf = self.hp1.tick_hp(stick, 3500.0, sr) * 0.3;
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.08*dm)).exp()
                         + osc_sine(self.phase2) * 0.12 * (-t/(0.05*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 4500.0*tm, 0.7, sr);
                let wf = self.hp1.tick_hp(wire, 2000.0, sr) * (-t/(0.12*dm)).exp() * 0.35;
                sf + head + wf
            }
            40 => { // Snare: Fat Backbeat — 230Hz, big body, long wires
                let f = 230.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                advance_phase(&mut self.phase3, f * 2.136, sr);
                let head = osc_sine(self.phase1) * 0.4 * (-t/(0.12*dm)).exp()
                         + osc_sine(self.phase2) * 0.18 * (-t/(0.08*dm)).exp()
                         + osc_sine(self.phase3) * 0.08 * (-t/(0.05*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 3800.0*tm, 0.6, sr);
                let wf = wire * (-t/(0.25*dm)).exp() * 0.4;
                let stick = self.noise() * (-t*1500.0).exp() * 0.2;
                head + wf + stick
            }
            41 => { // Snare: Dry Studio — 285Hz, damped, Purdie-style
                let f = 285.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.35 * (-t/(0.06*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 5200.0*tm, 0.5, sr);
                let wf = wire * (-t/(0.08*dm)).exp() * 0.3;
                let stick = self.noise() * (-t*2200.0).exp() * 0.25;
                head + wf + stick
            }
            42 => { // Snare: Brush Swish — 260Hz, noise-dominated, gentle
                let f = 260.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.2 * (-t/(0.1*dm)).exp();
                let brush = self.noise() * (t/0.004).min(1.0); // slow rise = brush stroke
                let bf = self.svf1.bandpass(brush, 3000.0, 0.8, sr) * (-t/(0.15*dm)).exp() * 0.4;
                head + bf
            }
            43 => { // Snare: Cross-Stick — rim click, no wires
                advance_phase(&mut self.phase1, 550.0*tm, sr);
                advance_phase(&mut self.phase2, 1450.0*tm, sr);
                let crack = osc_sine(self.phase1) * 0.3 + osc_sine(self.phase2) * 0.2;
                let click = self.noise() * (-t*1000.0).exp() * 0.25;
                let env = (-t/(0.02*dm)).exp();
                (crack + click) * env
            }
            44 => { // Snare: Ghost Note — very soft, all wire buzz, minimal head
                let f = 295.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.1 * (-t/(0.04*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 4800.0, 0.7, sr);
                let wf = wire * (-t/(0.06*dm)).exp() * 0.2;
                head + wf
            }
            45 => { // Snare: Ringy Metal Shell — 340Hz, long shell ring
                let f = 340.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.1*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.15 * (-t/(0.08*dm)).exp();
                let shell = self.noise() * (-t*40.0).exp();
                let shell_r = self.svf2.bandpass(shell, 520.0*tm, 18.0, sr) * 0.12; // metal ring
                let wire = self.svf1.bandpass(self.noise()*nm, 4200.0, 0.6, sr) * (-t/(0.18*dm)).exp() * 0.35;
                head + m1 + shell_r + wire
            }
            46 => { // Snare: Loose Wires — 270Hz, rattly long buzz
                let f = 270.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.09*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 3500.0, 0.5, sr);
                let wf = wire * (-t/(0.35*dm)).exp() * 0.45; // long loose buzz
                head + wf
            }
            47 => { // Snare: Piccolo — 380Hz, high tuned, bright, short
                let f = 380.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.25 * (-t/(0.05*dm)).exp();
                let crack = self.noise() * (-t*2500.0).exp() * 0.3;
                let cf = self.hp1.tick_hp(crack, 5000.0, sr);
                let wire = self.svf1.bandpass(self.noise()*nm, 6000.0, 0.8, sr) * (-t/(0.1*dm)).exp() * 0.3;
                head + cf + wire
            }
            48 => { // Snare: Wood Shell Deep — 220Hz, warm woody character
                let f = 220.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 2.136, sr);
                let head = osc_sine(self.phase1) * 0.35 * (-t/(0.1*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.06*dm)).exp();
                let shell = self.noise() * (-t*50.0).exp();
                let sf = self.svf2.bandpass(shell, 320.0*tm, 10.0, sr) * 0.08; // wood resonance
                let wire = self.svf1.bandpass(self.noise()*nm, 3600.0, 0.7, sr) * (-t/(0.15*dm)).exp() * 0.35;
                head + m1 + sf + wire
            }
            49 => { // Snare: Crack — 320Hz, maximum attack, minimal body
                advance_phase(&mut self.phase1, 320.0*tm, sr);
                let head = osc_sine(self.phase1) * 0.2 * (-t/(0.04*dm)).exp();
                let crack = self.noise() * (t/0.0002).min(1.0) * (-t*3000.0).exp();
                let cf = self.hp1.tick_hp(crack, 4000.0, sr) * 0.45;
                let wire = self.svf1.bandpass(self.noise()*nm, 5500.0, 0.8, sr) * (-t/(0.08*dm)).exp() * 0.25;
                head + cf + wire
            }
            50 => { // Snare: Thick — 250Hz, lots of body + wires together
                let f = 250.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, f * 1.593, sr);
                advance_phase(&mut self.phase3, f * 2.296, sr);
                let head = osc_sine(self.phase1) * 0.4 * (-t/(0.12*dm)).exp()
                         + osc_sine(self.phase2) * 0.2 * (-t/(0.08*dm)).exp()
                         + osc_sine(self.phase3) * 0.1 * (-t/(0.06*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr) * (-t/(0.2*dm)).exp() * 0.4;
                let stick = self.noise() * (-t*1200.0).exp() * 0.15;
                head + wire + stick
            }
            51 => { // Snare: Rim Shot Full — stick hits rim + head simultaneously
                let f = 300.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                advance_phase(&mut self.phase2, 900.0*tm, sr); // rim harmonic
                advance_phase(&mut self.phase3, 2200.0*tm, sr); // rim overtone
                let head = osc_sine(self.phase1) * 0.3 * (-t/(0.08*dm)).exp();
                let rim = osc_sine(self.phase2) * 0.2 * (-t/(0.015*dm)).exp()
                        + osc_sine(self.phase3) * 0.1 * (-t/(0.01*dm)).exp();
                let crack = self.noise() * (-t*2000.0).exp() * 0.3;
                let wire = self.svf1.bandpass(self.noise()*nm, 4200.0, 0.7, sr) * (-t/(0.12*dm)).exp() * 0.3;
                head + rim + crack + wire
            }
            52 => { // Snare: Sizzle — 290Hz, snare wires very present, buzzy
                let f = 290.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.25 * (-t/(0.07*dm)).exp();
                let wire = self.svf1.bandpass(self.noise()*nm, 3800.0, 0.4, sr) * (-t/(0.28*dm)).exp() * 0.5;
                let wire2 = self.svf2.bandpass(self.noise()*nm, 7000.0, 1.0, sr) * (-t/(0.15*dm)).exp() * 0.2;
                head + wire + wire2
            }
            53 => { // Snare: Soft Roll — very gentle, like a snare roll sustain
                let f = 275.0 * tm;
                advance_phase(&mut self.phase1, f, sr);
                let head = osc_sine(self.phase1) * 0.15 * (-t/(0.15*dm)).exp();
                // Simulated roll: amplitude modulated noise
                let roll_mod = (t * 18.0 * TAU).sin().abs();
                let wire = self.svf1.bandpass(self.noise()*nm, 4000.0, 0.6, sr) * roll_mod * (-t/(0.3*dm)).exp() * 0.3;
                head + wire
            }

            // ══ CLAPS/SNAPS: 54-59 (6 unique) ══

            54 => { // Clap: Group Tight — 5 clappers, tight timing
                let mut env = 0.0;
                for k in 0..5u32 {
                    let off = (self.hit_rand(k*3) * 0.008 + self.hit_rand(k*3+1).abs() * 0.005).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 200.0).exp() * (0.75 + self.hit_rand(k*3+2) * 0.25) * 0.18; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 2400.0 + self.hit_rand(60)*400.0, 1.5, sr);
                let hp = self.hp1.tick_hp(f, 700.0, sr);
                let tail = (-t/(0.1*dm)).exp() * 0.25;
                hp * (env + tail)
            }
            55 => { // Clap: Loose Group — 7 clappers, wide timing spread
                let mut env = 0.0;
                for k in 0..7u32 {
                    let off = (self.hit_rand(k*4) * 0.02 + self.hit_rand(k*4+1).abs() * 0.01).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 150.0).exp() * (0.6 + self.hit_rand(k*4+2) * 0.4) * 0.13; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 1800.0 + self.hit_rand(80)*600.0, 1.2, sr);
                let tail = (-t/(0.15*dm)).exp() * 0.3;
                f * (env + tail)
            }
            56 => { // Snap: Finger Snap — single, sharp, high
                let snap = self.noise() * (t/0.0002).min(1.0) * (-t*1200.0).exp();
                let f = self.svf1.bandpass(snap, 3200.0, 2.5, sr);
                let hp = self.hp1.tick_hp(f, 1500.0, sr);
                hp * 0.5
            }
            57 => { // Slap: Hand on Thigh — mid-frequency thump + skin noise
                advance_phase(&mut self.phase1, 180.0*tm, sr);
                let body = osc_sine(self.phase1) * 0.25 * (-t/(0.04*dm)).exp();
                let slap = self.noise() * (-t*400.0).exp();
                let sf = self.svf1.bandpass(slap, 1500.0, 1.5, sr) * 0.3;
                body + sf
            }
            58 => { // Clap: Single — one person clap
                let snap = self.noise() * (t/0.0004).min(1.0) * (-t*250.0).exp();
                let f = self.svf1.bandpass(snap, 2000.0, 1.8, sr);
                let hp = self.hp1.tick_hp(f, 500.0, sr);
                let tail = (-t/(0.06*dm)).exp() * 0.15;
                hp * 0.4 + tail * self.noise() * 0.03
            }
            59 => { // Clap: Reverb Hall — group clap with long room tail
                let mut env = 0.0;
                for k in 0..4u32 {
                    let off = (self.hit_rand(k*5) * 0.01).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 180.0).exp() * 0.22; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 2200.0, 1.3, sr);
                let tail = (-t/(0.25*dm)).exp() * 0.35; // long reverb
                f * (env + tail)
            }

            // ══ HI-HATS: 60-71 (12 unique, modal synthesis) ══

            60 => { // Hat: Closed Tight — very short, bright tick
                let freqs = [380.0*tm, 950.0*tm, 1600.0*tm, 2500.0*tm, 3700.0*tm, 5100.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 5500.0, sr);
                let stick = self.noise() * (-t*2500.0).exp() * 0.12;
                let env = (-t/(0.025*dm)).exp();
                (hp * 0.35 + stick) * env
            }
            61 => { // Hat: Closed Medium — standard closed hat
                let freqs = [342.0*tm, 817.0*tm, 1453.0*tm, 2298.0*tm, 3419.0*tm, 4735.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 7000.0, 1.5, sr);
                let hp = self.hp1.tick_hp(f, 4500.0, sr);
                let env = (-t/(0.04*dm)).exp();
                hp * env * 0.35
            }
            62 => { // Hat: Closed Dark — lower modal content, muted
                let freqs = [300.0*tm, 720.0*tm, 1280.0*tm, 2050.0*tm, 3100.0*tm, 4200.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.lowpass(m, 6000.0, 0.5, sr);
                let env = (-t/(0.05*dm)).exp();
                f * env * 0.3
            }
            63 => { // Hat: Closed Sizzle — contact buzz between cymbals
                let freqs = [365.0*tm, 870.0*tm, 1520.0*tm, 2400.0*tm, 3550.0*tm, 4900.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 5000.0, sr);
                let sizzle = self.noise() * (-t/(0.03*dm)).exp();
                let sz = self.svf1.bandpass(sizzle, 8500.0, 5.0, sr) * 0.1;
                let env = (-t/(0.045*dm)).exp();
                (hp * 0.3 + sz) * env
            }
            64 => { // Hat: Half-Open — cymbals barely touching, medium ring
                let freqs = [348.0*tm, 835.0*tm, 1480.0*tm, 2340.0*tm, 3480.0*tm, 4800.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 6000.0, 1.0, sr);
                let hp = self.hp1.tick_hp(f, 3500.0, sr);
                let sizzle = self.noise() * (-t/(0.15*dm)).exp();
                let sz = self.svf2.bandpass(sizzle, 7500.0, 4.0, sr) * 0.08;
                let env = (-t/(0.18*dm)).exp();
                (hp * 0.3 + sz) * env
            }
            65 => { // Hat: Open Bright — full shimmer, long ring
                let freqs = [355.0*tm, 850.0*tm, 1500.0*tm, 2380.0*tm, 3530.0*tm, 4880.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                advance_phase(&mut self.modal_phases[0], 6400.0*tm, sr);
                advance_phase(&mut self.modal_phases[1], 8300.0*tm, sr);
                let upper = osc_sine(self.modal_phases[0]) * 0.15 * (-t/(0.6*dm)).exp()
                          + osc_sine(self.modal_phases[1]) * 0.08 * (-t/(0.8*dm)).exp();
                let hp = self.hp1.tick_hp(m, 3000.0, sr);
                let env = (-t/(0.6*dm)).exp();
                (hp * 0.3 + upper) * env
            }
            66 => { // Hat: Open Dark — fewer high modes, warmer sustain
                let freqs = [310.0*tm, 740.0*tm, 1300.0*tm, 2100.0*tm, 3150.0*tm, 4350.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.lowpass(m, 7000.0, 0.4, sr);
                let env = (-t/(0.55*dm)).exp();
                f * env * 0.3
            }
            67 => { // Hat: Pedal Chick — foot pedal, no stick
                let freqs = [360.0*tm, 860.0*tm, 1520.0*tm, 2400.0*tm, 3560.0*tm, 4920.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let env = (-t/(0.018*dm)).exp();
                let chick = self.noise() * (-t*600.0).exp();
                let cf = self.svf1.bandpass(chick, 1400.0, 2.5, sr) * 0.12;
                m * env * 0.2 + cf
            }
            68 => { // Hat: Open Washy — very long, wash-like
                let freqs = [330.0*tm, 790.0*tm, 1400.0*tm, 2220.0*tm, 3300.0*tm, 4560.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 5500.0, 0.8, sr);
                let env = (-t/(1.0*dm)).exp();
                f * env * 0.25
            }
            69 => { // Hat: Closed Thin — very little body, all tick
                let freqs = [420.0*tm, 1020.0*tm, 1750.0*tm, 2750.0*tm, 4000.0*tm, 5500.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 7000.0, sr);
                let env = (-t/(0.02*dm)).exp();
                hp * env * 0.3
            }
            70 => { // Hat: Half-Open Bright — brighter than 64
                let freqs = [370.0*tm, 900.0*tm, 1580.0*tm, 2480.0*tm, 3680.0*tm, 5080.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 4000.0, sr);
                let env = (-t/(0.22*dm)).exp();
                hp * env * 0.32
            }
            71 => { // Hat: Open Sizzle — riveted cymbal character
                let freqs = [345.0*tm, 825.0*tm, 1460.0*tm, 2310.0*tm, 3430.0*tm, 4740.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let rattle = self.noise() * (t * 28.0 * TAU).sin().abs() * (-t * 5.0).exp();
                let rf = self.svf1.bandpass(rattle, 9000.0, 3.0, sr) * 0.08;
                let env = (-t/(0.7*dm)).exp();
                m * env * 0.25 + rf
            }

            // ══ TOMS: 72-79 (8 unique, different depths and characters) ══

            72 => { // Tom: Floor Deep — 80Hz, long decay, big body
                let f = 80.0*tm; let sw = f*0.15*(-t*25.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
                let body = osc_sine(self.phase1) * 0.55 * (0.2*(-t/0.015).exp() + 0.8*(-t/(0.32*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.15 * (-t/(0.1*dm)).exp();
                let stick = self.noise() * (t/0.001).min(1.0) * (-t*250.0).exp();
                let sf = self.svf1.bandpass(stick, 2500.0, 1.3, sr) * 0.1;
                body + m1 + sf
            }
            73 => { // Tom: Floor Medium — 105Hz
                let f = 105.0*tm; let sw = f*0.12*(-t*30.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*2.136, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.25*(-t/0.01).exp() + 0.75*(-t/(0.26*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.1 * (-t/(0.07*dm)).exp();
                let stick = self.noise() * (-t*280.0).exp() * 0.08;
                body + m1 + stick
            }
            74 => { // Tom: Low Rack — 130Hz
                let f = 130.0*tm; let sw = f*0.1*(-t*35.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                let body = osc_sine(self.phase1) * 0.5 * (0.3*(-t/0.008).exp() + 0.7*(-t/(0.22*dm)).exp());
                let stick = self.noise() * (-t*320.0).exp();
                let sf = self.svf1.bandpass(stick, 3000.0, 1.5, sr) * 0.1;
                body + sf
            }
            75 => { // Tom: Mid Rack — 165Hz
                let f = 165.0*tm; let sw = f*0.1*(-t*38.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
                let body = osc_sine(self.phase1) * 0.48 * (0.3*(-t/0.007).exp() + 0.7*(-t/(0.2*dm)).exp());
                let m1 = osc_sine(self.phase2) * 0.12 * (-t/(0.06*dm)).exp();
                body + m1
            }
            76 => { // Tom: High Rack — 210Hz
                let f = 210.0*tm; let sw = f*0.08*(-t*40.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                let body = osc_sine(self.phase1) * 0.45 * (0.35*(-t/0.006).exp() + 0.65*(-t/(0.17*dm)).exp());
                let stick = self.noise() * (-t*350.0).exp();
                let sf = self.svf1.bandpass(stick, 3500.0, 1.5, sr) * 0.12;
                body + sf
            }
            77 => { // Tom: Concert — 145Hz, big resonant, orchestral
                let f = 145.0*tm; let sw = f*0.1*(-t*22.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                advance_phase(&mut self.phase2, (f+sw)*1.593, sr);
                advance_phase(&mut self.phase3, (f+sw)*2.296, sr);
                let body = osc_sine(self.phase1) * 0.5 * (-t/(0.35*dm)).exp();
                let m1 = osc_sine(self.phase2) * 0.18 * (-t/(0.15*dm)).exp();
                let m2 = osc_sine(self.phase3) * 0.1 * (-t/(0.1*dm)).exp();
                body + m1 + m2
            }
            78 => { // Tom: Roto High — 280Hz, bright, synthetic-ish
                let f = 280.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.4 * (-t/(0.12*dm)).exp();
                let ring = osc_triangle(self.phase1 * 2.5) * 0.1 * (-t/(0.06*dm)).exp();
                body + ring
            }
            79 => { // Tom: Timbale-ish — 350Hz, metallic shell ring
                let f = 350.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.35 * (-t/(0.15*dm)).exp();
                let shell = self.noise() * (-t*30.0).exp();
                let sf = self.svf1.bandpass(shell, f*2.5, 12.0, sr) * 0.15;
                body + sf
            }

            // ══ CYMBALS: 80-87 (8 unique) ══

            80 => { // Crash: Dark — lower modal content, warm
                let freqs = [300.0*tm, 710.0*tm, 1250.0*tm, 2000.0*tm, 3000.0*tm, 4150.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.lowpass(m, 7000.0, 0.3, sr);
                let n = self.noise() * 0.12;
                let env = (t/0.003).min(1.0) * (-t/(1.2*dm)).exp();
                (f * 0.35 + n) * env
            }
            81 => { // Crash: Bright — higher modal content, cutting
                let freqs = [400.0*tm, 960.0*tm, 1680.0*tm, 2650.0*tm, 3900.0*tm, 5400.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 2500.0, sr);
                let env = (t/0.002).min(1.0) * (-t/(1.5*dm)).exp();
                hp * env * 0.3
            }
            82 => { // Ride: Ping — defined stick sound, controlled wash
                let freqs = [420.0*tm, 1000.0*tm, 1720.0*tm, 2800.0*tm, 4150.0*tm, 5700.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let f = self.svf1.bandpass(m, 5500.0, 1.0, sr);
                let ping = (-t*120.0).exp() * 0.15;
                let env = (-t/(0.8*dm)).exp();
                (f * env + ping) * 0.28
            }
            83 => { // Ride: Wash — loose, washy
                let freqs = [380.0*tm, 910.0*tm, 1580.0*tm, 2520.0*tm, 3750.0*tm, 5180.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let env = (-t/(1.5*dm)).exp();
                m * env * 0.22
            }
            84 => { // Ride Bell — defined tonal bell hit
                advance_phase(&mut self.phase1, 750.0*tm, sr);
                advance_phase(&mut self.phase2, 1125.0*tm, sr);
                advance_phase(&mut self.phase3, 1688.0*tm, sr);
                let bell = osc_sine(self.phase1)*0.3 + osc_sine(self.phase2)*0.25 + osc_sine(self.phase3)*0.15;
                let env = (-t/(0.65*dm)).exp();
                bell * env
            }
            85 => { // Splash — fast, bright, short
                let freqs = [450.0*tm, 1080.0*tm, 1850.0*tm, 2900.0*tm, 4300.0*tm, 5900.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(m, 3500.0, sr);
                let env = (t/0.001).min(1.0) * (-t/(0.45*dm)).exp();
                hp * env * 0.3
            }
            86 => { // China — trashy, aggressive overtones
                let freqs = [280.0*tm, 670.0*tm, 1180.0*tm, 1900.0*tm, 2850.0*tm, 3950.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                // Extra distortion for trashy character
                let dist = (m * 2.0).tanh() * 0.5;
                let env = (t/0.002).min(1.0) * (-t/(0.9*dm)).exp();
                dist * env * 0.3
            }
            87 => { // Cymbal: Sizzle — riveted, continuous rattle
                let freqs = [350.0*tm, 840.0*tm, 1480.0*tm, 2350.0*tm, 3500.0*tm, 4830.0*tm];
                let m = self.hat_oscs.tick(sr, &freqs);
                let rattle = self.noise() * (t * 30.0 * TAU).sin().abs() * (-t*4.0).exp();
                let rf = self.svf1.bandpass(rattle, 8500.0, 3.5, sr) * 0.1;
                let env = (-t/(1.2*dm)).exp();
                m * env * 0.22 + rf
            }

            // ══ PERCUSSION: 88-99 (12 unique) ══

            88 => { // Tambourine — jingles
                let freqs = [4500.0*tm, 6200.0*tm, 7800.0*tm, 9500.0*tm, 11200.0*tm, 13000.0*tm];
                let j = self.hat_oscs.tick(sr, &freqs);
                let hp = self.hp1.tick_hp(j, 4000.0, sr);
                let shake = (t * 24.0 * TAU).sin().abs() * (-t*7.0).exp();
                let env = (-t/(0.18*dm)).exp();
                hp * (env + shake * 0.2) * 0.25
            }
            89 => { // Shaker — dry seeds
                let n = self.noise();
                let f = self.svf1.bandpass(n, 7200.0, 1.3, sr);
                let hp = self.hp1.tick_hp(f, 5000.0, sr);
                hp * (-t/(0.06*dm)).exp() * 0.3
            }
            90 => { // Cowbell — two tones
                advance_phase(&mut self.phase1, 575.0*tm, sr);
                advance_phase(&mut self.phase2, 862.0*tm, sr);
                let body = osc_sine(self.phase1)*0.35 + osc_sine(self.phase2)*0.3;
                let f = self.svf1.bandpass(body, 720.0, 4.0, sr);
                f * (-t/(0.06*dm)).exp()
            }
            91 => { // Woodblock — sharp woody click
                advance_phase(&mut self.phase1, 1950.0*tm, sr);
                advance_phase(&mut self.phase2, 3200.0*tm, sr);
                let click = osc_sine(self.phase1)*0.3 + osc_sine(self.phase2)*0.15;
                let n = self.noise() * (-t*1200.0).exp() * 0.1;
                (click + n) * (-t/(0.012*dm)).exp()
            }
            92 => { // Clave — resonant wood
                advance_phase(&mut self.phase1, 2500.0*tm, sr);
                osc_sine(self.phase1) * 0.4 * (-t/(0.02*dm)).exp()
            }
            93 => { // Triangle — metallic ring
                advance_phase(&mut self.phase1, 1200.0*tm, sr);
                advance_phase(&mut self.phase2, 3600.0*tm, sr);
                let body = osc_sine(self.phase1)*0.3 + osc_sine(self.phase2)*0.2;
                body * (-t/(0.8*dm)).exp()
            }
            94 => { // Cabasa — scratchy beads
                let n = self.noise();
                let f = self.svf1.bandpass(n, 8800.0, 2.0, sr);
                f * (-t/(0.1*dm)).exp() * 0.28
            }
            95 => { // Guiro — scraping stick
                let n = self.noise();
                let f = self.svf1.bandpass(n, 4200.0, 3.0, sr);
                let scrape = (t * 40.0 * TAU).sin().abs() * (-t*5.0).exp();
                f * (scrape * 0.4 + 0.2) * (-t/(0.2*dm)).exp()
            }
            96 => { // Vibraslap — rattle
                let n = self.noise();
                let f = self.svf1.bandpass(n, 3300.0, 5.5, sr);
                let rattle = (t * 38.0 * TAU).sin().abs() * (-t*3.0).exp();
                f * rattle * (-t/(0.45*dm)).exp() * 0.25
            }
            97 => { // Maracas — short shake
                let n = self.noise();
                let hp = self.hp1.tick_hp(n, 6000.0, sr);
                hp * (-t/(0.04*dm)).exp() * 0.25
            }
            98 => { // Agogo High — metallic bell
                advance_phase(&mut self.phase1, 920.0*tm, sr);
                advance_phase(&mut self.phase2, 1384.0*tm, sr);
                (osc_sine(self.phase1)*0.35 + osc_sine(self.phase2)*0.25) * (-t/(0.15*dm)).exp()
            }
            99 => { // Agogo Low
                advance_phase(&mut self.phase1, 660.0*tm, sr);
                advance_phase(&mut self.phase2, 992.0*tm, sr);
                (osc_sine(self.phase1)*0.35 + osc_sine(self.phase2)*0.25) * (-t/(0.15*dm)).exp()
            }

            // ══ MORE PERCUSSION: 100-111 (12 unique) ══

            100 => { // Conga: Open High
                let f = 340.0*tm; let sw = f*0.06*(-t*45.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                let slap = self.noise() * (-t*500.0).exp() * 0.12;
                osc_sine(self.phase1) * 0.5 * (-t/(0.2*dm)).exp() + slap
            }
            101 => { // Conga: Muted
                let f = 320.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                osc_sine(self.phase1) * 0.45 * (-t/(0.06*dm)).exp()
            }
            102 => { // Conga: Low Open
                let f = 220.0*tm; let sw = f*0.05*(-t*35.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                osc_sine(self.phase1) * 0.5 * (-t/(0.22*dm)).exp()
            }
            103 => { // Conga: Slap
                let f = 350.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.3 * (-t/(0.04*dm)).exp();
                let slap = self.noise() * (-t*800.0).exp();
                let sf = self.svf1.bandpass(slap, 2800.0, 2.0, sr) * 0.3;
                body + sf
            }
            104 => { // Bongo: High
                let f = 420.0*tm; let sw = f*0.08*(-t*70.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                osc_sine(self.phase1) * 0.45 * (-t/(0.1*dm)).exp()
            }
            105 => { // Bongo: Low
                let f = 310.0*tm; let sw = f*0.07*(-t*55.0).exp();
                advance_phase(&mut self.phase1, f+sw, sr);
                osc_sine(self.phase1) * 0.5 * (-t/(0.12*dm)).exp()
            }
            106 => { // Timbale: High — metallic shell ring
                let f = 520.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.4;
                let ring = self.noise() * (-t*20.0).exp();
                let rf = self.svf1.bandpass(ring, f*3.0, 10.0, sr) * 0.12;
                (body + rf) * (-t/(0.2*dm)).exp()
            }
            107 => { // Timbale: Low
                let f = 360.0*tm;
                advance_phase(&mut self.phase1, f, sr);
                let body = osc_sine(self.phase1) * 0.45;
                let shell = self.noise() * (-t*25.0).exp();
                let sf = self.svf1.bandpass(shell, f*2.5, 8.0, sr) * 0.1;
                (body + sf) * (-t/(0.22*dm)).exp()
            }
            108 => { // Cuica: High — squeaky friction drum
                let f = 600.0 + 400.0 * (-t*8.0).exp();
                advance_phase(&mut self.phase1, f*tm, sr);
                osc_sine(self.phase1) * 0.35 * (-t/(0.15*dm)).exp()
            }
            109 => { // Cuica: Low
                let f = 350.0 + 200.0 * (-t*6.0).exp();
                advance_phase(&mut self.phase1, f*tm, sr);
                osc_sine(self.phase1) * 0.35 * (-t/(0.2*dm)).exp()
            }
            110 => { // Whistle — pitched sine with vibrato
                let vib = (t * 6.0 * TAU).sin() * 25.0;
                advance_phase(&mut self.phase1, 2300.0*tm + vib, sr);
                osc_sine(self.phase1) * 0.3 * (-t/(0.08*dm)).exp()
            }
            111 => { // Clap: Vinyl Room — warm room clap
                let mut env = 0.0;
                for k in 0..4u32 {
                    let off = (self.hit_rand(k*6) * 0.01).abs();
                    let to = t - off;
                    if to >= 0.0 { env += (-to * 160.0).exp() * 0.2; }
                }
                let n = self.noise() * nm;
                let f = self.svf1.bandpass(n, 1900.0, 1.5, sr);
                let lp = self.svf2.lowpass(f, 5000.0, 0.5, sr); // tape warmth extra
                let tail = (-t/(0.18*dm)).exp() * 0.3;
                lp * (env + tail)
            }

            // Anything outside 24-111 = silence
            _ => 0.0,
        }
    }

    // ══════════════════════════════════════════════════════════════════════════
}
