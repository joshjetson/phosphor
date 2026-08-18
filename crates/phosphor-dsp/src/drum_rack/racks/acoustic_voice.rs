//! One hit on an acoustic kit, from the strike to the microphone.
//!
//! [`super::acoustic`] holds the physics and the pieces; this is the voice
//! that assembles them. Everything that costs a transcendental — the Bessel
//! amplitudes, the eigen-solve for the two heads, the modal coefficients — is
//! done once, on the first sample of the hit. The per-sample path is the
//! modal bank, three filters, the wire bouncer and an envelope, and none of it
//! allocates, locks or branches on anything a knob can change mid-note.
//!
//! A hit is built from the panel as it stands when the note fires and does not
//! read it again, which is what an acoustic drum does: turning a tuning key
//! while the drum is ringing does not retune what is already in the air. The
//! level fader is the exception, because it is a fader — it is applied by the
//! rack, after this.

use super::acoustic::*;
use super::super::*;

/// Where the contact-time scaling is referenced from. A stroke at this contact
/// time is the one the voicing levels were set at; a harder or softer beater
/// keeps the same impulse and changes only how far up the modes it reaches.
const CONTACT_REF: f64 = 0.0012;

/// How rough the contact is — a stick on a head is not a mathematical
/// impulse, and the noise under the strike is a real part of the attack.
const CONTACT_ROUGH: f64 = 0.16;

/// The grace note of a flam, ahead of the main stroke.
const FLAM_SECONDS: f64 = 0.024;
const FLAM_FORCE: f64 = 0.55;

/// How many modes follow the attack pitch drop.
///
/// The whole head is at higher tension, so in principle every mode glides. In
/// practice the modes above the eighth are gone inside the glide's own time
/// constant and are broadband besides, so rebuilding their coefficients is
/// paying a `sin` and a `cos` for something nothing can hear. Eight is what
/// the ear follows.
const RETUNE_MODES: usize = 8;

/// Where the strands sit against the drum they are under.
///
/// The wire model's output is in impacts per unit time against a reference
/// landing speed, which is a physical quantity and not a mix level; this is
/// the one number that turns it into one. Set by ear against the body of the
/// snare and then held: at the default strainer setting the strands are the
/// louder half of a backbeat and the quieter half of a ghost note, which is
/// the right way round.
const WIRE_LEVEL: f64 = 0.115;

/// Where the two plates of a half-open hat meet the strand model.
///
/// The strands under a snare ride the resonant head directly, in the units the
/// modal bank writes — the gaps in [`Wires`] are set against a membrane's
/// amplitude and a membrane is what drives them. A cymbal's forty modes carry
/// several times that, so the plates need a working point of their own or they
/// would throw each other clear and never come back.
const RATTLE_HEAD: f64 = 0.8;

/// Everything one hit needs that is not the modal bank itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Acoustic {
    /// Contact time and force of the strike.
    contact: f64,
    force: f64,
    /// What one unit of strike force is worth to the modal bank.
    ///
    /// A complex one-pole driven by a pulse ends up at the pulse's area *in
    /// samples*, so a bank driven by a force in newtons would be as loud as
    /// the host's sample rate is high. This is the inverse of the reference
    /// stroke's area, so a drum is the same drum at 44.1 kHz and at 96.
    excite: f64,
    /// Seconds from the grace note to the main stroke, or zero for a single
    /// stroke.
    flam: f64,
    /// A brush rather than a stick, which is a different excitation and not a
    /// filter setting.
    brush: bool,
    /// How rough the contact is — a stick on a head is not a mathematical
    /// impulse, and a stiff hand at the edge of a conga is rougher again,
    /// which is most of what makes a slap crack.
    rough: f64,
    /// The attack pitch drop and how fast it falls away.
    drop: f64,
    drop_tau: f64,
    /// Shell resonance under the head.
    shell_hz: f64,
    shell_mix: f64,
    /// Strands under the bottom head.
    wire_mix: f64,
    /// The port in the front head, as a Helmholtz resonance.
    port_hz: f64,
    port_q: f64,
    port_mix: f64,
    /// The stick's own contact resonance on a cymbal — the ping.
    ping_hz: f64,
    ping_mix: f64,
    /// Two plates rattling on each other, which is a half-open hat.
    rattle: f64,
    /// The gate, for the kit that has one.
    gate_threshold: f64,
    gate_release: f64,
    gate_peak: f64,
    gate_gain: f64,
    /// Output trim for this voice.
    out: f64,
}

impl Acoustic {
    pub(crate) const fn new() -> Self {
        Self {
            contact: CONTACT_REF,
            force: 1.0,
            excite: 1.0,
            flam: 0.0,
            brush: false,
            rough: CONTACT_ROUGH,
            drop: 0.0,
            drop_tau: 0.01,
            shell_hz: 0.0,
            shell_mix: 0.0,
            wire_mix: 0.0,
            port_hz: 0.0,
            port_q: 2.0,
            port_mix: 0.0,
            ping_hz: 0.0,
            ping_mix: 0.0,
            rattle: 0.0,
            gate_threshold: 0.0,
            gate_release: 0.05,
            gate_peak: 0.0,
            gate_gain: 1.0,
            out: 1.0,
        }
    }
}

/// A knob across a ring time: an eighth of the voicing at the bottom, near
/// three times it at the top, and the voicing itself at the centre detent.
fn ring_knob(knob: f64) -> f64 {
    geometric(0.35, 2.857, knob)
}

/// A knob across a beater: two and a half times the contact time at the bottom
/// — a soft felt beater — and two fifths of it at the top, which is a hard
/// plastic one.
fn contact_knob(knob: f64) -> f64 {
    geometric(2.6, 0.385, knob)
}

/// A knob across a damping ring, as a multiplier on how much faster the high
/// modes go than the low ones. Down is muffled, up is open.
fn damp_knob(knob: f64) -> f64 {
    geometric(2.2, 0.455, knob)
}

impl DrumVoice {
    /// One sample of an acoustic kit.
    pub(crate) fn synth_acoustic(&mut self, sr: f64, c: &Controls, kit: &Kit) -> f64 {
        // The first sample of the hit is where the hit is built. Everything
        // with a transcendental in it lives here and nowhere else.
        if self.noise_counter <= 1 {
            self.build_acoustic(sr, c, kit);
        }
        let dt = 1.0 / sr;
        let t = self.time;

        // The head starts sharp and falls. Rebuilt at a control rate over the
        // first few time constants, which is the only place in this voice a
        // `sin` runs after the hit is built.
        if self.acoustic.drop > 0.0
            && t < self.acoustic.drop_tau * 7.0
            && self.noise_counter % RETUNE_INTERVAL == 0
        {
            let ratio = 1.0 + self.acoustic.drop * (-t / self.acoustic.drop_tau).exp();
            self.bank.retune(ratio, RETUNE_MODES);
        }

        let x = self.acoustic_exciter(t);
        let (near, far) = self.bank.tick(x);

        let a = self.acoustic;
        let mut out = near;

        // The shell: a light resonance under the head, and the whole of a
        // cross-stick.
        if a.shell_mix > 0.0 {
            out += self.svf1.bandpass(x, a.shell_hz, 7.0, sr) * a.shell_mix;
        }
        // The port, if the front head has one cut in it: the air in the hole
        // is a mass and the air in the shell a spring.
        if a.port_mix > 0.0 {
            out += self.svf2.bandpass(far, a.port_hz, a.port_q, sr) * a.port_mix;
        }
        // The stick's contact resonance on a cymbal — the ping.
        if a.ping_mix > 0.0 {
            out += self.svf1.bandpass(x, a.ping_hz, 3.5, sr) * a.ping_mix * (-t / 0.06).exp();
        }
        // The strands, and the two plates of a half-open hat, which rattle for
        // the same reason and through the same model.
        if a.wire_mix > 0.0 {
            let energy = self.wires.tick(far, dt);
            out += self.wire_voice(sr, energy) * a.wire_mix;
        } else if a.rattle > 0.0 {
            // Two plates instead of a head and a strand: the same contact
            // model, driven by the plate's own motion rather than a
            // membrane's — and by its *bulk* motion, because what makes and
            // breaks contact between two cymbals is the whole plate rising
            // and falling, not the ten-kilohertz ripple riding on it.
            let bulk = self.lp1.tick_lp(near, 350.0, sr);
            let energy = self.wires.tick(bulk * RATTLE_HEAD, dt);
            out += self.wire_voice(sr, energy) * a.rattle;
        }

        // The DRIVE knob sees each close mic at its own working level, before
        // the fader that balances it against the rest of the kit. That is what
        // the knob's reference level means — it is calibrated against a bare
        // voice in this rack, and a bare voice here is a drum, not a drum with
        // a kit balance already on it. Applied after the trim instead, the
        // knob would find every one of these voices below its reference and
        // *lift* them, which is the one thing `drive_stage` exists not to do.
        if c.drive > 0.01 {
            out = drive_stage(out, c.drive * 2.0);
        }
        out *= a.out;

        // The studio kit's gate, keyed off the voice's own peak so that a
        // quiet hit is gated the way a loud one is.
        if a.gate_threshold > 0.0 {
            let mag = out.abs();
            self.acoustic.gate_peak = if mag > self.acoustic.gate_peak {
                mag
            } else {
                self.acoustic.gate_peak * (-dt / 0.05).exp()
            };
            let open = self.acoustic.gate_peak
                > self.acoustic.gate_threshold * self.acoustic.force.max(0.05);
            self.acoustic.gate_gain = if open {
                1.0
            } else {
                self.acoustic.gate_gain * (-dt / a.gate_release).exp()
            };
            out *= self.acoustic.gate_gain;
        }
        out
    }

    /// The strands' own sound: a bright band and the air above it, gated by
    /// the impacts rather than by an envelope.
    fn wire_voice(&mut self, sr: f64, energy: f64) -> f64 {
        let n = self.noise();
        let band = self.svf3.bandpass(n * energy, 3900.0, 1.4, sr);
        let air = self.hp1.tick_hp(n * energy, 6500.0, sr);
        (band * 0.85 + air * 0.5) * WIRE_LEVEL
    }

    /// The force the stick puts into the head.
    ///
    /// A half-sine over the contact time, which is what a Hertzian contact
    /// gives and what decides how far up the mode series the strike reaches:
    /// the pulse has no energy above about `1/contact`, so a wood tip at half
    /// a millisecond drives everything and a felt beater at four milliseconds
    /// rolls off a few hundred Hz up. The area is held constant against
    /// [`CONTACT_REF`], so changing the beater changes the *spectrum* of the
    /// strike and not the momentum in it, which is the physical statement.
    fn acoustic_exciter(&mut self, t: f64) -> f64 {
        let a = self.acoustic;
        if a.brush {
            // A brush is many strands touching over an area and over time, so
            // there is no impulse in it at all: the head is driven by a
            // continuous rough contact that dies away.
            let rise = (t / 0.004).min(1.0);
            return self.noise() * rise * (-t / 0.05).exp() * a.force * a.excite * 0.55;
        }
        // The grace note first, then the stroke: a flam is a quiet stroke
        // ahead of a loud one, not the other way round.
        let mut force = if a.flam > 0.0 {
            strike(t, a.contact) * FLAM_FORCE + strike(t - a.flam, a.contact)
        } else {
            strike(t, a.contact)
        } * a.force;
        if force != 0.0 {
            force += self.noise() * force.abs() * a.rough;
        }
        force * a.excite
    }

    /// Build the hit: the modal bank, the strike, and everything else the
    /// voice needs, from the panel as it stands right now.
    fn build_acoustic(&mut self, sr: f64, c: &Controls, kit: &Kit) {
        let art = articulation(self.sound);
        let mut s = strike_of(art);
        if matches!(s.kind, Kind::Brush) && !kit.brushes {
            // No brushes in this bag. The nearest stroke a stick player has is
            // the one played with the butt of the hand: short of a brush, long
            // of a stick, and soft.
            s.contact = 0.0035;
            s.force *= 2.6;
            s.kind = Kind::Stick;
        }
        self.bank.clear();
        self.wires.arm(0.5);
        self.acoustic = Acoustic::new();
        self.acoustic.contact = s.contact;
        self.acoustic.force = s.force * (0.35 + 0.65 * f64::from(self.velocity));
        // The reference stroke's impulse, in samples: `∫ sin(πt/T) dt` over the
        // contact is `2T/π`, and the height scaling holds that constant at the
        // reference contact time whatever beater is used.
        self.acoustic.excite = 1.0 / (2.0 * CONTACT_REF / std::f64::consts::PI * sr);
        self.acoustic.flam = if matches!(s.kind, Kind::Flam) { FLAM_SECONDS } else { 0.0 };
        self.acoustic.brush = matches!(s.kind, Kind::Brush);
        if matches!(s.kind, Kind::Slap) {
            self.acoustic.rough = CONTACT_ROUGH * 4.0;
        }
        self.acoustic.out = kit.out;
        if let Some(g) = kit.gate {
            self.acoustic.gate_threshold = g.threshold;
            self.acoustic.gate_release = g.release;
            self.acoustic.gate_gain = 1.0;
        }

        match s.on {
            Piece::Kick => {
                let port = kit.port;
                self.build_membrane(sr, c, &kit.kick, &s, port);
            }
            Piece::Snare => self.build_membrane(sr, c, &kit.snare, &s, None),
            Piece::Tom(i) => self.build_membrane(sr, c, &kit.toms[i], &s, None),
            Piece::Conga(i) => self.build_membrane(sr, c, &kit.congas[i], &s, None),
            Piece::Ride => self.build_plate(sr, c, &kit.ride, &s, c.tune_ratio),
            Piece::Crash(i) => self.build_plate(sr, c, &kit.crash[i], &s, c.tune_ratio),
            Piece::Splash => self.build_plate(sr, c, &kit.splash, &s, c.tune_ratio),
            Piece::China => self.build_plate(sr, c, &kit.china, &s, c.tune_ratio),
            Piece::Hat => self.build_hat(sr, c, &kit.hat, &s),
            Piece::Cowbell => self.build_bar(sr, &kit.cowbell, &s),
        }
    }

    /// A drum: one or two heads, the cavity between them, and the shell.
    fn build_membrane(
        &mut self,
        sr: f64,
        c: &Controls,
        d: &Drum,
        s: &Strike,
        port: Option<Port>,
    ) {
        let stretch = self.accent_stretch();
        // The panel. TUNE is head tension, DECAY is how much the drum is
        // muffled, TONE is what is against the far head, ATTACK is the beater.
        let tune = c.tune_ratio;
        let ring = d.ring * ring_knob(c.decay) * s.ring * stretch;
        let tilt = d.tilt * damp_knob(1.0 - c.decay) * (1.0 + s.damp);
        // A muffled head loses its coupling with its damping: a pillow against
        // the front head is the same object that stops it moving.
        let air_spring = d.air_spring * (0.15 + 1.7 * c.tone).clamp(0.05, 2.0);
        if s.contact > 0.0 {
            self.acoustic.contact = s.contact * contact_knob(c.attack);
        }

        let batter = d.batter * tune;
        let reso = d.reso * tune;
        if matches!(s.kind, Kind::CrossStick) {
            // The shoulder of the stick on the rim with its tip resting on
            // the head. The hand holding it down stops the membrane, so what
            // is left is the shell — a short wooden cylinder, whose bending
            // modes are these — and a thud from the head under the palm.
            const SHELL: [f64; 4] = [1.0, 1.62, 2.31, 3.08];
            const LEVEL: [f64; 4] = [1.0, 0.52, 0.28, 0.15];
            let hz0 = d.shell_hz * tune;
            for (k, (&r, &l)) in SHELL.iter().zip(LEVEL.iter()).enumerate() {
                let ring = 0.085 * stretch / (1.0 + 0.55 * (r - 1.0));
                self.bank.set(k, hz0 * r, ring, l, 1.0, 0.0, sr);
            }
            self.bank.set(4, batter * loaded_ratio(0, d.air_load), 0.05 * stretch, 0.4, 0.55, 0.0, sr);
            // The stick lands on the rim as well, and that is the click on the
            // front of the sound.
            self.bank.set(5, (hz0 * 4.2).min(sr * 0.45), 0.012 * stretch, 1.4, 1.0, 0.0, sr);
            self.acoustic.out *= d.out;
            return;
        }
        // Air loading, which lowers every mode and lowers the bottom of the
        // series hardest. `loaded[k]` is where mode k really sits.
        let mut loaded = [0.0f64; 8];
        for (k, l) in loaded.iter_mut().enumerate() {
            *l = loaded_ratio(k, d.air_load);
        }
        // Where the stick lands decides which modes it reaches: mode (n,m) is
        // driven by J_n(j_nm · at), so a beater in the dead centre of the head
        // reaches the two axisymmetric modes and nothing else.
        let mut shape = [0.0f64; 8];
        for (k, sh) in shape.iter_mut().enumerate() {
            *sh = bessel_j(BESSEL_ORDER[k], BESSEL_ZERO[k] * s.at.clamp(0.0, 1.0));
        }
        let mut k = 0usize;
        let fundamental;
        if d.reso > 0.0 && air_spring > 0.0 {
            // Two heads, coupled through the air in the shell. The (0,1) pair
            // is the only one the cavity touches — every mode with n ≥ 1 has
            // as much head rising as falling and changes the enclosed volume
            // by nothing.
            let pair = couple(batter * loaded[0], reso * loaded[0], air_spring);
            fundamental = pair[0].0;
            for &(hz, batter_share) in &pair {
                let reso_share = (1.0 - batter_share * batter_share).max(0.0).sqrt();
                self.bank.set(
                    k,
                    hz,
                    ring * mode_ring(hz / fundamental.max(1.0), tilt),
                    shape[0] * batter_share,
                    batter_share + reso_share * d.reso_mic,
                    reso_share,
                    sr,
                );
                k += 1;
            }
        } else {
            // One head over an open shell: no cavity, so nothing to couple to.
            fundamental = batter * loaded[0];
            self.bank.set(k, fundamental, ring, shape[0], 1.0, 0.0, sr);
            k += 1;
        }
        // The rest of the batter head's series.
        for m in 1..8 {
            let hz = batter * loaded[m];
            self.bank.set(
                k,
                hz,
                ring * mode_ring(hz / fundamental.max(1.0), tilt),
                shape[m],
                0.75,
                0.0,
                sr,
            );
            k += 1;
        }
        // And the resonant head's, which the strike never touches directly —
        // it arrives through the air, weakly, and is the second of the two
        // groups a two-headed drum's spectrum falls into.
        if d.reso > 0.0 {
            for m in 1..5 {
                let hz = reso * loaded[m];
                self.bank.set(
                    k,
                    hz,
                    ring * mode_ring(hz / fundamental.max(1.0), tilt) * 0.8,
                    shape[m] * air_spring * 0.5,
                    d.reso_mic,
                    0.55,
                    sr,
                );
                k += 1;
            }
        }

        self.acoustic.drop = d.drop;
        self.acoustic.drop_tau = d.drop_tau;
        self.acoustic.shell_hz = d.shell_hz;
        self.acoustic.shell_mix = d.shell_mix;
        if matches!(s.kind, Kind::Rimshot) {
            // The rim is a hoop of steel with the head's tension on it, and
            // the stick is on both at once. Its partials go in the same bank
            // as the head's, at the same scaling, so they are as loud beside
            // the drum as the model says and not as a mix knob says — and
            // they are gone in a few tens of milliseconds, because the hand
            // holding the stick against the hoop is what damps them.
            // Ratio to the head's tension, ring time, and how hard the stick
            // reaches it.
            const RIM: [(f64, f64, f64); 3] =
                [(13.0, 0.034, 7.5), (24.7, 0.022, 4.1), (40.3, 0.014, 2.2)];
            for (i, &(ratio, ring, drive)) in RIM.iter().enumerate() {
                let hz = d.batter * tune * ratio;
                if hz < sr * 0.45 {
                    self.bank.set(k + i, hz, ring * stretch, drive, 1.0, 0.0, sr);
                }
            }
        }
        if d.wires > 0.0 {
            // SNAPPY is the strainer, and on a real drum that one control does
            // two things: it sets how much of the strands you hear, which is
            // the level here, and how freely they bounce, which is `arm`.
            // Tighter is louder *and* less free — see the note on [`Wires`].
            self.acoustic.wire_mix = d.wires * (0.35 + 1.3 * c.snappy);
            self.wires.arm(c.snappy);
        }
        if let Some(p) = port {
            self.acoustic.port_hz = p.hz * tune;
            self.acoustic.port_q = p.q;
            self.acoustic.port_mix = p.mix;
        }
        self.acoustic.out *= d.out;
    }

    /// A cymbal: one plate, or two of them clamped together.
    fn build_plate(&mut self, sr: f64, c: &Controls, p: &Plate, s: &Strike, tune: f64) {
        let stretch = self.accent_stretch();
        // DECAY is a hand on the cymbal and TONE is how bright it was bought:
        // a dark cymbal is one whose high modes go first, which is the tilt.
        let ring = p.ring * s.ring * stretch * ring_knob(c.decay);
        let tilt = p.tilt * damp_knob(c.tone);
        let lowest = p.lowest * tune;
        // The energy in the strike, which is what the frequency gating reads.
        let energy = f64::from(self.velocity) * s.force;
        let n = p.modes.min(MODES);
        for k in 0..n {
            let ratio = plate_ratio(k, p.spread, p.scatter);
            let hz = lowest * ratio;
            // A mode nobody can hear is a mode nobody has to compute, and the
            // series climbs fast enough that a big plate runs out of audio
            // band before it runs out of modes.
            if hz > sr * 0.44 {
                break;
            }
            // Higher modes go first, which is most of what a cymbal's decay
            // is — as a power law, because a plate is not a head.
            let tau = ring / ratio.powf(tilt);
            let reach = plate_reach(k, s.at);
            // Frequency gating: above `gate_from` a mode is not quiet below
            // the threshold, it is absent.
            let g = if k >= p.gate_from {
                gate(energy, p.gate_open, p.gate_full)
            } else {
                1.0
            };
            self.bank.set(k, hz, tau, reach * g, 1.0 / (1.0 + 0.02 * k as f64), 0.0, sr);
        }
        self.bank.couple(p.cascade_span, p.cascade);
        // The stick's own contact resonance, which is the ping. Loudest at the
        // bow, which is where a ride is played and where the ping is.
        self.acoustic.ping_hz = (lowest * plate_ratio(n / 6, p.spread, 0.0)).min(sr * 0.4);
        self.acoustic.ping_mix = 0.4 * (1.0 - (s.at - 0.6).abs() * 1.6).max(0.05);
        self.acoustic.out *= p.out;
    }

    /// A hi-hat: two cymbals in one bank, the lower half of it the top plate
    /// and the upper half the bottom one.
    ///
    /// Closing them does three things, all of which closing them really does.
    /// It damps both plates, hard. It dumps the top plate's energy into the
    /// bottom, which is the modal coupling with a span of half the bank so
    /// that mode `k + 20` reads mode `k`. And it takes the low modes away,
    /// because a low mode needs the whole plate free to move — which is why a
    /// closed hat is brighter than an open one and not merely shorter, and is
    /// the reason this reads as two cymbals held together rather than one
    /// cymbal gated.
    ///
    /// Between open and closed there is a fourth thing: the plates touching
    /// and separating, which is a half-open hat and is the same contact model
    /// the snare's strands use.
    fn build_hat(&mut self, sr: f64, c: &Controls, p: &Plate, s: &Strike) {
        let stretch = self.accent_stretch();
        let clamp = s.clamp.clamp(0.0, 1.0);
        // The two decay knobs: the open hat's, and the closed one's.
        let knob = ring_knob(c.decay);
        let ring = p.ring * s.ring * stretch * knob * (1.0 - 0.82 * clamp);
        let half = MODES / 2;
        let energy = f64::from(self.velocity) * s.force;
        for k in 0..MODES {
            let (index, plate_lowest, weight) = if k < half {
                (k, p.lowest, 1.0)
            } else {
                // The bottom plate is heavier and a little differently tuned,
                // which is where an open hat's shimmer comes from: two sets of
                // modes a few Hz apart, beating.
                (k - half, p.lowest * 1.062, 0.8)
            };
            let ratio = plate_ratio(index, p.spread, p.scatter);
            let hz = plate_lowest * ratio;
            if hz > sr * 0.44 {
                continue;
            }
            let tau = ring / ratio.powf(p.tilt);
            // Closing the hats does not damp the two plates evenly. A low mode
            // needs the whole plate free to move and the clamp is exactly what
            // stops that, while a high mode lives in a small enough region of
            // the metal to carry on regardless. That is why a closed hat is
            // bright and an open one has body, and it is not a filter: it is
            // which modes are still there.
            let low = 1.0 - index as f64 / half as f64;
            let clamped = 1.0 - 0.88 * clamp * low * low;
            let reach = plate_reach(index, s.at) * weight * clamped;
            let g = if index >= p.gate_from {
                gate(energy, p.gate_open, p.gate_full)
            } else {
                1.0
            };
            self.bank.set(k, hz, tau, reach * g, 1.0 / (1.0 + 0.02 * index as f64), 0.0, sr);
        }
        // The top plate's energy into the bottom, in proportion to how hard
        // they are pressed together.
        self.bank.couple(half, 0.35 + 0.5 * clamp);
        // A pedal close is the two plates hitting each other rather than a
        // stick hitting one of them, so it has the low air between them in it
        // where a stick on a closed hat has none.
        let pedal = clamp > 0.95 && s.at > 0.85 && s.contact > 0.0006;
        self.acoustic.ping_hz = if pedal { p.lowest * 0.42 } else { p.lowest * 2.4 };
        self.acoustic.ping_mix = if pedal { 1.3 } else { 0.25 };
        // Half-open: the two plates in and out of contact, which is a rattle
        // and not a filter setting.
        self.acoustic.rattle = if (0.1..0.85).contains(&clamp) {
            self.wires.arm(0.8);
            0.14 - 0.28 * (clamp - 0.45).abs()
        } else {
            0.0
        };
        self.acoustic.out *= p.out;
    }

    /// A cowbell: a bent steel bar, no membrane anywhere in it.
    ///
    /// A straight free-free bar rings at 1 : 2.756 : 5.404 : 8.933, which is
    /// what a triangle sounds like. Folding the bar into a cowbell pulls those
    /// together and damps them, and what is left is the pair a cowbell is
    /// heard as — the ratios below are where a struck cowbell measures rather
    /// than where the bar equation puts them.
    fn build_bar(&mut self, sr: f64, b: &Bar, s: &Strike) {
        const RATIOS: [f64; 5] = [1.0, 1.48, 2.31, 3.44, 5.12];
        const LEVELS: [f64; 5] = [1.0, 0.85, 0.45, 0.22, 0.12];
        let stretch = self.accent_stretch();
        for (k, (&r, &l)) in RATIOS.iter().zip(LEVELS.iter()).enumerate() {
            // Where the bar is struck decides which of its partials answer:
            // a node is a node.
            let reach = (std::f64::consts::PI * (k as f64 + 1.0) * s.at).sin().abs().max(0.18);
            self.bank.set(
                k,
                b.hz * r,
                b.ring * s.ring * stretch / (1.0 + 0.55 * (r - 1.0)),
                l * reach,
                1.0,
                0.0,
                sr,
            );
        }
        self.acoustic.out *= b.out;
    }
}

/// The force of one strike, as a half-sine over the contact time.
///
/// The area is held constant against [`CONTACT_REF`], so a harder beater is
/// the same momentum delivered in less time — taller and narrower — rather
/// than a different amount of it. That is what makes contact time a spectral
/// control and not a level control.
fn strike(t: f64, contact: f64) -> f64 {
    // Total: `contact` reaches this from a voicing table through a knob, and a
    // zero would be a division rather than an instantaneous strike.
    if t < 0.0 || contact <= 0.0 || t >= contact {
        return 0.0;
    }
    (std::f64::consts::PI * t / contact).sin() * (CONTACT_REF / contact)
}

/// How much shorter mode `k` rings than the fundamental, as a function of how
/// far above it the mode sits. A head loses its high modes first; `tilt` is
/// how fast, and a coated or double-ply head is much faster than a clear
/// single-ply one.
fn mode_ring(ratio: f64, tilt: f64) -> f64 {
    1.0 / (1.0 + tilt * (ratio - 1.0).max(0.0))
}

/// How strongly a strike at radius `at` reaches mode `k` of a plate.
///
/// A band in mode index, log-spaced, plus a floor: the strike has a centre it
/// couples to best and a width, and under both there is the wash that any
/// strike anywhere on a cymbal sets going.
///
/// Chosen rather than derived, and stated as such — the driving-point mobility
/// of a spun, hammered and lathed plate is not something this file is going to
/// integrate. What it has to get right is the ordering, and that is not in
/// doubt. At the **edge** every low mode is at an antinode, so the band is
/// centred on the bottom of the series and is wide: the whole plate answers,
/// which is a crash. At the **bow** the centre moves up and the band narrows,
/// which is a ride's mix of stick and wash. On the **dome** the low modes are
/// not there to be driven at all — the bell is a stiff local region and its
/// band is narrow, high and clear, with the floor down so the wash under it
/// gets out of the way. One plate, three sounds.
fn plate_reach(k: usize, at: f64) -> f64 {
    let edge = at.clamp(0.0, 1.0);
    let away = 1.0 - edge;
    let centre = 0.6 + 5.0 * away * away;
    let width = 0.35 + 1.3 * edge;
    let floor = 0.06 + 0.62 * edge * edge;
    let l = ((k + 1) as f64).ln() - centre.ln();
    let band = (-(l * l) / (2.0 * width * width)).exp();
    (floor + (1.0 - floor) * band) * (0.45 + 0.55 * edge)
}
