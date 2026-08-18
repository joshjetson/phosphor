//! Jazz kit — a small bebop set, close-miked and dry.
//!
//! Eighteen-inch kick with the front head on and tuned, a fourteen by five
//! wood snare at high tension with a light strainer, three toms tuned up and
//! left to ring, and thin cymbals: a twenty-inch ride that is dark and busy
//! rather than pingy, and a bell you can find without hunting for it. Brushes
//! where they belong.
//!
//! This is a *voicing sheet*, not a synthesis engine. The physics is in
//! [`acoustic`](super::acoustic) and the voice that assembles it is in
//! [`acoustic_voice`](super::acoustic_voice); what is here is the set of
//! drums, the tunings they were left at, and how dead or open each of them is.
//! The other two acoustic kits are the same file with different drums in it,
//! which is exactly what the difference between two kits is.
//!
//! What makes this one *jazz* and not a retune of the others:
//!
//! * the kick keeps its resonant head, and keeps it tight — the strongest
//!   cavity coupling of the three, so its two low modes sit a fourth apart and
//!   the drum has an audible pitch rather than a thud;
//! * nothing is muffled. Every ring time here is the longest of the three
//!   kits, and the mode tilts are the shallowest, so the heads keep their
//!   upper modes instead of losing them in the first twenty milliseconds;
//! * the cymbals are thin, which is low `spread` — more modes in the same
//!   band, so the ride washes and the crash is a spray rather than a hit;
//! * and it is the only kit with brushes, which are not a filter setting: a
//!   brush drives the head over an area and over time, so there is no impulse
//!   in the excitation at all.

use super::acoustic::*;
use super::super::*;

pub(crate) const KIT: Kit = Kit {
    // ── 18" bass drum, felt beater, front head on and tuned a tone above the
    // batter. Sealed, which is the strongest air spring on any of the three
    // kits and the reason this kick has a note in it.
    kick: Drum {
        batter: 78.0,
        reso: 88.0,
        air_spring: 0.42,
        air_load: 0.75,
        ring: 0.30,
        tilt: 1.5,
        drop: 0.10,
        drop_tau: 0.012,
        shell_hz: 190.0,
        shell_mix: 0.05,
        reso_mic: 0.40,
        wires: 0.0,
        out: 1.05,
    },
    // ── 14"×5" wood snare, thin single-ply heads, high tension, a light
    // strainer with the strands slack enough to sizzle.
    snare: Drum {
        batter: 210.0,
        reso: 250.0,
        air_spring: 0.30,
        air_load: 0.50,
        ring: 0.16,
        tilt: 1.9,
        drop: 0.07,
        drop_tau: 0.008,
        shell_hz: 620.0,
        shell_mix: 0.10,
        reso_mic: 0.30,
        wires: 0.95,
        out: 0.81,
    },
    // ── 14" floor, 12" rack, 10" rack. Tuned up and left open.
    toms: [
        Drum {
            batter: 118.0,
            reso: 132.0,
            air_spring: 0.28,
            air_load: 0.70,
            ring: 0.42,
            tilt: 1.6,
            drop: 0.16,
            drop_tau: 0.020,
            shell_hz: 330.0,
            shell_mix: 0.05,
            reso_mic: 0.18,
            wires: 0.0,
            out: 0.92,
        },
        Drum {
            batter: 165.0,
            reso: 186.0,
            air_spring: 0.26,
            air_load: 0.62,
            ring: 0.34,
            tilt: 1.7,
            drop: 0.18,
            drop_tau: 0.017,
            shell_hz: 430.0,
            shell_mix: 0.05,
            reso_mic: 0.18,
            wires: 0.0,
            out: 0.92,
        },
        Drum {
            batter: 212.0,
            reso: 240.0,
            air_spring: 0.24,
            air_load: 0.55,
            ring: 0.28,
            tilt: 1.8,
            drop: 0.20,
            drop_tau: 0.015,
            shell_hz: 540.0,
            shell_mix: 0.05,
            reso_mic: 0.18,
            wires: 0.0,
            out: 0.92,
        },
    ],
    // ── Congas: one head over an open shell, so there is no cavity, no
    // resonant head and none of the coupling above. `reso` is zero and the
    // voice takes the single-headed branch.
    congas: [
        Drum {
            batter: 132.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.55,
            ring: 0.30,
            tilt: 2.1,
            drop: 0.14,
            drop_tau: 0.014,
            shell_hz: 300.0,
            shell_mix: 0.07,
            reso_mic: 0.0,
            wires: 0.0,
            out: 2.97,
        },
        Drum {
            batter: 178.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.48,
            ring: 0.24,
            tilt: 2.2,
            drop: 0.16,
            drop_tau: 0.012,
            shell_hz: 390.0,
            shell_mix: 0.07,
            reso_mic: 0.0,
            wires: 0.0,
            out: 2.97,
        },
        Drum {
            batter: 230.0,
            reso: 0.0,
            air_spring: 0.0,
            air_load: 0.42,
            ring: 0.20,
            tilt: 2.3,
            drop: 0.18,
            drop_tau: 0.011,
            shell_hz: 480.0,
            shell_mix: 0.07,
            reso_mic: 0.0,
            wires: 0.0,
            out: 2.97,
        },
    ],
    // ── 20" thin ride. Low `spread` is a dense mode set, which is a big thin
    // plate: the wash under the stick is most of what this cymbal is, and the
    // cascade is what keeps it there instead of letting it decay.
    ride: Plate {
        lowest: 235.0,
        spread: 1.10,
        scatter: 1.0,
        ring: 3.0,
        tilt: 0.34,
        gate_from: 17,
        gate_open: 0.34,
        gate_full: 0.86,
        cascade: 0.30,
        cascade_span: 6,
        modes: MODES,
        out: 0.177,
    },
    crash: [
        // 16" thin.
        Plate {
            lowest: 382.0,
            spread: 1.02,
            scatter: 1.1,
            ring: 1.9,
            tilt: 0.40,
            gate_from: 15,
            gate_open: 0.30,
            gate_full: 0.80,
            cascade: 0.34,
            cascade_span: 5,
            modes: MODES,
            out: 0.134,
            },
        // 18" thin — bigger, so lower and denser again.
        Plate {
            lowest: 306.0,
            spread: 1.00,
            scatter: 1.2,
            ring: 2.4,
            tilt: 0.37,
            gate_from: 14,
            gate_open: 0.28,
            gate_full: 0.78,
            cascade: 0.36,
            cascade_span: 5,
            modes: MODES,
            out: 0.129,
            },
    ],
    // ── 10" splash: small and thick, so the modes climb fast and there are
    // few of them in the band that matters.
    splash: Plate {
        lowest: 740.0,
        spread: 1.22,
        scatter: 0.9,
        ring: 0.62,
        tilt: 0.55,
        gate_from: 12,
        gate_open: 0.40,
        gate_full: 0.90,
        cascade: 0.22,
        cascade_span: 4,
        modes: 28,
        out: 0.242,
    },
    // ── 18" china: barely a plate at all, which is what the scatter is for.
    china: Plate {
        lowest: 296.0,
        spread: 0.96,
        scatter: 2.6,
        ring: 1.3,
        tilt: 0.42,
        gate_from: 13,
        gate_open: 0.26,
        gate_full: 0.76,
        cascade: 0.40,
        cascade_span: 4,
        modes: MODES,
        out: 0.097,
    },
    // ── 14" thin hats. Half the bank per plate; the bottom one is tuned 6%
    // above the top, which is the beat you hear in an open hat.
    hat: Plate {
        lowest: 512.0,
        spread: 1.10,
        scatter: 1.0,
        ring: 0.90,
        tilt: 0.46,
        gate_from: 10,
        gate_open: 0.32,
        gate_full: 0.85,
        cascade: 0.00,
        cascade_span: MODES / 2,
        modes: MODES / 2,
        out: 0.158,
    },
    cowbell: Bar { hz: 552.0, ring: 0.34, out: 1.05 },
    gate: None,
    port: None,
    brushes: true,
    out: 0.60,
};

/// What this kit rings for, in seconds to −20 dB, at the three points of each
/// decay knob it has.
///
/// Measured off the render rather than read off the voicing: a drum's −20 dB
/// point is not its lowest mode's time constant, because the strike is louder
/// than the drum it sets ringing and the upper modes are gone long before the
/// bottom one is. `the_panel_reads_back_what_each_machine_renders` holds these
/// to what comes out.
pub(crate) fn decay_seconds(index: usize, knob: f64) -> Option<f64> {
    Some(match index {
        P_BD_DECAY => interpolate3(&[0.086, 0.208, 0.546], knob),
        P_LT_DECAY => interpolate3(&[0.075, 0.261, 0.673], knob),
        P_MT_DECAY => interpolate3(&[0.081, 0.218, 0.565], knob),
        P_HT_DECAY => interpolate3(&[0.065, 0.171, 0.475], knob),
        P_CY_DECAY => interpolate3(&[0.283, 0.855, 2.625], knob),
        P_OH_DECAY => interpolate3(&[0.163, 0.473, 1.428], knob),
        P_CH_DECAY => interpolate3(&[0.017, 0.049, 0.141], knob),
        _ => return None,
    })
}

impl DrumVoice {
    pub(crate) fn synth_jazz(&mut self, sr: f64, c: &Controls) -> f64 {
        self.synth_acoustic(sr, c, &KIT)
    }
}
