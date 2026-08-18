//! Funk kit — punchy, tight, close and controlled.
//!
//! Twenty-two-inch kick with a felt strip against both heads and a port cut in
//! the front one, a fourteen by five and a half metal snare cranked up over
//! twenty tight strands, and three small toms with gel on them. Cymbals that
//! define rather than wash: a heavier ride with a ping you can hear across a
//! stage, and crashes that get out of the way.
//!
//! What makes this one *funk* and not a retune of the jazz kit:
//!
//! * **the port**. A hole in the front head bleeds the cavity, so the air
//!   spring between the two heads is two thirds gone and the drum's two low
//!   modes close up towards each other — but the hole is a Helmholtz
//!   resonator in its own right, a mass of air in the neck against the spring
//!   of the air behind it, and it puts a resonance back under the drum that
//!   the sealed jazz kick does not have. Two structural differences from one
//!   hole, in opposite directions, which is why a ported kick sounds like a
//!   different drum rather than a duller one;
//! * the felt strip is the fastest high-mode tilt of the three kits on the
//!   kick, so the drum is a punch and not a note;
//! * the snare is the tightest strainer here — more strands, closer to the
//!   head — which the wire model reads as more contacts per unit time and a
//!   shorter choke after a hard hit;
//! * the toms lose their upper modes three times as fast as the jazz kit's,
//!   which is what a gel pad on the head does.

use super::acoustic::*;
use super::super::*;

pub(crate) const KIT: Kit = Kit {
    // ── 22" kick, felt strip, ported front head. Low air spring because the
    // port bleeds it; see `port` below for what the hole gives back.
    kick: Drum {
        batter: 63.0,
        reso: 70.0,
        air_spring: 0.15,
        air_load: 0.95,
        ring: 0.20,
        tilt: 2.6,
        drop: 0.13,
        drop_tau: 0.010,
        shell_hz: 150.0,
        shell_mix: 0.04,
        reso_mic: 0.55,
        wires: 0.0,
        out: 1.25,
    },
    // ── 14"×5.5" metal snare, cranked, twenty strands pulled tight.
    snare: Drum {
        batter: 238.0,
        reso: 292.0,
        air_spring: 0.34,
        air_load: 0.44,
        ring: 0.125,
        tilt: 2.4,
        drop: 0.09,
        drop_tau: 0.007,
        shell_hz: 780.0,
        shell_mix: 0.09,
        reso_mic: 0.34,
        wires: 1.15,
        out: 0.85,
    },
    // ── 14" floor, 12" and 10" racks, gel on all three.
    toms: [
        Drum {
            batter: 104.0,
            reso: 118.0,
            air_spring: 0.24,
            air_load: 0.80,
            ring: 0.24,
            tilt: 3.0,
            drop: 0.20,
            drop_tau: 0.016,
            shell_hz: 300.0,
            shell_mix: 0.04,
            reso_mic: 0.16,
            wires: 0.0,
            out: 1.0,
        },
        Drum {
            batter: 150.0,
            reso: 170.0,
            air_spring: 0.22,
            air_load: 0.70,
            ring: 0.20,
            tilt: 3.1,
            drop: 0.22,
            drop_tau: 0.014,
            shell_hz: 400.0,
            shell_mix: 0.04,
            reso_mic: 0.16,
            wires: 0.0,
            out: 1.0,
        },
        Drum {
            batter: 206.0,
            reso: 234.0,
            air_spring: 0.20,
            air_load: 0.60,
            ring: 0.17,
            tilt: 3.2,
            drop: 0.24,
            drop_tau: 0.012,
            shell_hz: 510.0,
            shell_mix: 0.04,
            reso_mic: 0.16,
            wires: 0.0,
            out: 1.0,
        },
    ],
    congas: [
        Drum {
            batter: 126.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.58,
            ring: 0.26,
            tilt: 2.5,
            drop: 0.16,
            drop_tau: 0.013,
            shell_hz: 285.0,
            shell_mix: 0.06,
            reso_mic: 0.0,
            wires: 0.0,
            out: 3.12,
        },
        Drum {
            batter: 172.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.50,
            ring: 0.21,
            tilt: 2.6,
            drop: 0.18,
            drop_tau: 0.011,
            shell_hz: 375.0,
            shell_mix: 0.06,
            reso_mic: 0.0,
            wires: 0.0,
            out: 3.12,
        },
        Drum {
            batter: 224.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.44,
            ring: 0.175,
            tilt: 2.7,
            drop: 0.20,
            drop_tau: 0.010,
            shell_hz: 465.0,
            shell_mix: 0.06,
            reso_mic: 0.0,
            wires: 0.0,
            out: 3.12,
        },
    ],
    // ── 20" heavy ride. High `spread` is a sparse mode set, which is a thick
    // plate: the modes are far apart, so the stick's contact reads as a pitch
    // instead of disappearing into a wash. That is the ping.
    ride: Plate {
        lowest: 308.0,
        spread: 1.18,
        scatter: 0.8,
        ring: 2.5,
        tilt: 0.44,
        gate_from: 19,
        gate_open: 0.38,
        gate_full: 0.90,
        cascade: 0.22,
        cascade_span: 6,
        modes: MODES,
        out: 0.203,
    },
    crash: [
        // 16" medium.
        Plate {
            lowest: 412.0,
            spread: 1.08,
            scatter: 0.9,
            ring: 1.55,
            tilt: 0.50,
            gate_from: 17,
            gate_open: 0.34,
            gate_full: 0.84,
            cascade: 0.26,
            cascade_span: 5,
            modes: MODES,
            out: 0.168,
            },
        // 18" medium.
        Plate {
            lowest: 334.0,
            spread: 1.05,
            scatter: 1.0,
            ring: 1.95,
            tilt: 0.47,
            gate_from: 16,
            gate_open: 0.32,
            gate_full: 0.82,
            cascade: 0.28,
            cascade_span: 5,
            modes: MODES,
            out: 0.162,
            },
    ],
    splash: Plate {
        lowest: 800.0,
        spread: 1.28,
        scatter: 0.8,
        ring: 0.50,
        tilt: 0.64,
        gate_from: 13,
        gate_open: 0.42,
        gate_full: 0.92,
        cascade: 0.17,
        cascade_span: 4,
        modes: 28,
        out: 0.260,
    },
    china: Plate {
        lowest: 322.0,
        spread: 1.00,
        scatter: 2.3,
        ring: 1.05,
        tilt: 0.50,
        gate_from: 14,
        gate_open: 0.30,
        gate_full: 0.80,
        cascade: 0.32,
        cascade_span: 4,
        modes: MODES,
        out: 0.105,
    },
    // ── 14" heavy hats: higher and sparser than the jazz pair, and shorter.
    hat: Plate {
        lowest: 566.0,
        spread: 1.16,
        scatter: 0.85,
        ring: 0.78,
        tilt: 0.52,
        gate_from: 11,
        gate_open: 0.36,
        gate_full: 0.88,
        cascade: 0.00,
        cascade_span: MODES / 2,
        modes: MODES / 2,
        out: 0.169,
    },
    cowbell: Bar { hz: 508.0, ring: 0.28, out: 1.12 },
    gate: None,
    // The hole in the front head: an air mass in the neck against the air
    // spring behind it, at the frequency that pair resonates.
    port: Some(Port { hz: 86.0, q: 1.7, mix: 0.42 }),
    brushes: false,
    out: 0.585,
};

/// What this kit rings for, in seconds to −20 dB. Measured off the render —
/// see the note in `kit_jazz`.
pub(crate) fn decay_seconds(index: usize, knob: f64) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => interpolate3(&[0.039, 0.145, 0.402], knob),
        P_LT_DECAY => interpolate3(&[0.051, 0.138, 0.377], knob),
        P_MT_DECAY => interpolate3(&[0.038, 0.109, 0.313], knob),
        P_HT_DECAY => interpolate3(&[0.033, 0.103, 0.298], knob),
        P_CY_DECAY => interpolate3(&[0.225, 0.673, 1.930], knob),
        P_OH_DECAY => interpolate3(&[0.149, 0.402, 1.257], knob),
        P_CH_DECAY => interpolate3(&[0.012, 0.035, 0.107], knob),
        _ => return None,
    })
}

impl DrumVoice {
    pub(crate) fn synth_funk(&mut self, sr: f64, c: &Controls) -> f64 {
        self.synth_acoustic(sr, c, &KIT)
    }
}
