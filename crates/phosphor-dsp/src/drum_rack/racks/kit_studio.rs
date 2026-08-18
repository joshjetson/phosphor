//! Studio kit — very dry, damped, modern, gated.
//!
//! A twenty-four-inch kick with a pillow inside it against both heads, a snare
//! with a full muffling ring on the batter and the strands pulled tight, three
//! toms taped and gated, and cymbals bought for not getting in the way. The
//! sound of a well-treated room with a gate across every close mic, which was
//! the point of gating drums in the first place: not an effect, a way of
//! getting a tom out of the snare mic.
//!
//! What makes this one *studio* and not a retune of the other two:
//!
//! * **the pillow**, which is the strongest single difference between any two
//!   kits in this rack. It is resting against the resonant head, so the air
//!   spring between the heads is nearly gone — [`KIT`]`.kick.air_spring` is
//!   0.05 against the jazz kit's 0.42 — and the drum's two low modes collapse
//!   into one. This kick is a two-headed drum being played as a one-headed
//!   one, which is exactly what a pillow does and is measurable: see
//!   `the_two_heads_split_the_kick_into_two_modes`;
//! * **the gate**. A downward expander on every voice, keyed off that voice's
//!   own peak so a quiet hit is gated the way a loud one is. Nothing else in
//!   this rack has one, and it is the reason the toms here stop rather than
//!   fade;
//! * every ring time is the shortest of the three kits and every tilt the
//!   steepest, which is tape, gel and a muffling ring doing what they do.

use super::acoustic::*;
use super::super::*;

pub(crate) const KIT: Kit = Kit {
    // ── 24" kick with a pillow against both heads. The cavity is all but
    // dead, so the two-head split this rack's other acoustic kicks have is
    // gone and what is left is one low mode and a beater.
    kick: Drum {
        batter: 56.0,
        reso: 62.0,
        air_spring: 0.05,
        air_load: 1.15,
        ring: 0.135,
        tilt: 3.4,
        drop: 0.15,
        drop_tau: 0.009,
        shell_hz: 140.0,
        shell_mix: 0.03,
        reso_mic: 0.14,
        wires: 0.0,
        out: 1.35,
    },
    // ── Snare with a full muffling ring on the batter head.
    snare: Drum {
        batter: 224.0,
        reso: 268.0,
        air_spring: 0.22,
        air_load: 0.46,
        ring: 0.092,
        tilt: 3.4,
        drop: 0.08,
        drop_tau: 0.006,
        shell_hz: 700.0,
        shell_mix: 0.07,
        reso_mic: 0.26,
        wires: 1.05,
        out: 0.89,
    },
    // ── Taped toms, and the gate below takes what the tape leaves.
    toms: [
        Drum {
            batter: 96.0,
            reso: 108.0,
            air_spring: 0.18,
            air_load: 0.86,
            ring: 0.175,
            tilt: 3.6,
            drop: 0.22,
            drop_tau: 0.014,
            shell_hz: 285.0,
            shell_mix: 0.03,
            reso_mic: 0.12,
            wires: 0.0,
            out: 1.05,
        },
        Drum {
            batter: 138.0,
            reso: 156.0,
            air_spring: 0.17,
            air_load: 0.76,
            ring: 0.150,
            tilt: 3.7,
            drop: 0.24,
            drop_tau: 0.012,
            shell_hz: 380.0,
            shell_mix: 0.03,
            reso_mic: 0.12,
            wires: 0.0,
            out: 1.05,
        },
        Drum {
            batter: 192.0,
            reso: 218.0,
            air_spring: 0.16,
            air_load: 0.66,
            ring: 0.128,
            tilt: 3.8,
            drop: 0.26,
            drop_tau: 0.010,
            shell_hz: 490.0,
            shell_mix: 0.03,
            reso_mic: 0.12,
            wires: 0.0,
            out: 1.05,
        },
    ],
    congas: [
        Drum {
            batter: 120.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.62,
            ring: 0.20,
            tilt: 3.0,
            drop: 0.18,
            drop_tau: 0.012,
            shell_hz: 275.0,
            shell_mix: 0.05,
            reso_mic: 0.0,
            wires: 0.0,
            out: 3.3,
        },
        Drum {
            batter: 164.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.54,
            ring: 0.165,
            tilt: 3.1,
            drop: 0.20,
            drop_tau: 0.010,
            shell_hz: 360.0,
            shell_mix: 0.05,
            reso_mic: 0.0,
            wires: 0.0,
            out: 3.3,
        },
        Drum {
            batter: 214.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.48,
            ring: 0.140,
            tilt: 3.2,
            drop: 0.22,
            drop_tau: 0.009,
            shell_hz: 450.0,
            shell_mix: 0.05,
            reso_mic: 0.0,
            wires: 0.0,
            out: 3.3,
        },
    ],
    // ── A dry ride: sparse, short, and with the cascade almost off, so the
    // stick lands and nothing blooms behind it.
    ride: Plate {
        lowest: 340.0,
        spread: 1.24,
        scatter: 0.7,
        ring: 1.75,
        tilt: 0.54,
        gate_from: 21,
        gate_open: 0.42,
        gate_full: 0.94,
        cascade: 0.14,
        cascade_span: 7,
        modes: MODES,
        out: 0.239,
    },
    crash: [
        Plate {
            lowest: 448.0,
            spread: 1.13,
            scatter: 0.8,
            ring: 1.15,
            tilt: 0.60,
            gate_from: 19,
            gate_open: 0.38,
            gate_full: 0.88,
            cascade: 0.18,
            cascade_span: 5,
            modes: MODES,
            out: 0.216,
            },
        Plate {
            lowest: 362.0,
            spread: 1.10,
            scatter: 0.9,
            ring: 1.45,
            tilt: 0.57,
            gate_from: 18,
            gate_open: 0.36,
            gate_full: 0.86,
            cascade: 0.20,
            cascade_span: 5,
            modes: MODES,
            out: 0.208,
            },
    ],
    splash: Plate {
        lowest: 862.0,
        spread: 1.33,
        scatter: 0.7,
        ring: 0.40,
        tilt: 0.74,
        gate_from: 14,
        gate_open: 0.44,
        gate_full: 0.94,
        cascade: 0.12,
        cascade_span: 4,
        modes: 28,
        out: 0.270,
    },
    china: Plate {
        lowest: 350.0,
        spread: 1.05,
        scatter: 2.0,
        ring: 0.85,
        tilt: 0.58,
        gate_from: 15,
        gate_open: 0.34,
        gate_full: 0.84,
        cascade: 0.26,
        cascade_span: 4,
        modes: MODES,
        out: 0.130,
    },
    hat: Plate {
        lowest: 688.0,
        spread: 1.22,
        scatter: 0.75,
        ring: 0.46,
        tilt: 0.66,
        gate_from: 12,
        gate_open: 0.40,
        gate_full: 0.90,
        cascade: 0.00,
        cascade_span: MODES / 2,
        modes: MODES / 2,
        out: 0.202,
    },
    cowbell: Bar { hz: 486.0, ring: 0.22, out: 1.16 },
    // The one thing on this kit that is not a drum: a downward expander across
    // every voice, threshold relative to that voice's own peak so a ghost note
    // is gated the way a backbeat is.
    gate: Some(Gate { threshold: 0.10, release: 0.014 }),
    port: None,
    brushes: false,
    out: 0.572,
};

/// What this kit rings for, in seconds to −20 dB. Measured off the render —
/// see the note in `kit_jazz`. These are shorter than the two kits above at
/// every knob position, and the gate is why.
pub(crate) fn decay_seconds(index: usize, knob: f64) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => interpolate3(&[0.035, 0.110, 0.315], knob),
        P_LT_DECAY => interpolate3(&[0.036, 0.091, 0.276], knob),
        P_MT_DECAY => interpolate3(&[0.035, 0.089, 0.232], knob),
        P_HT_DECAY => interpolate3(&[0.029, 0.075, 0.197], knob),
        P_CY_DECAY => interpolate3(&[0.147, 0.419, 1.245], knob),
        P_OH_DECAY => interpolate3(&[0.078, 0.173, 0.477], knob),
        P_CH_DECAY => interpolate3(&[0.007, 0.023, 0.057], knob),
        _ => return None,
    })
}

impl DrumVoice {
    pub(crate) fn synth_studio(&mut self, sr: f64, c: &Controls) -> f64 {
        self.synth_acoustic(sr, c, &KIT)
    }
}
