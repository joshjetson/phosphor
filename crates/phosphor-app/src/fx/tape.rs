//! The tape machine, wearing the insert layer's trait.
//!
//! # Why the adapter is here and not next to the medium
//!
//! The same reason the other four are: [`phosphor_dsp::fx::tape::Tape`] is
//! the tape, and this is the sixty lines that let a chain slot hold one. The
//! trait lives in `phosphor-core`; `phosphor-dsp` does not depend on
//! `phosphor-core` and must not start, because `phosphor-core` is the engine
//! — cpal, the mixer, the transport — and `phosphor-dsp` is arithmetic that
//! runs anywhere. This crate already depends on both, so the adapter sits at
//! the junction and is *only* an adapter.
//!
//! # It reads nothing from the context
//!
//! A tape machine has no grid and no sidechain: it does not care what the
//! tempo is, whether the transport is rolling, or what any other track is
//! doing. [`Effect::wants_key`] therefore stays false and the mixer skips
//! resolving a key for this chain.
//!
//! # The units on the wire
//!
//! Natural units, as the insert layer requires — a session stores what a
//! control meant, not a knob fraction that re-points the day a range moves.
//!
//! | index | control | unit | travel |
//! |---|---|---|---|
//! | 0 | `speed` | — | 0..2, a position in `Speed::ALL` |
//! | 1 | `drive` | % | 0 .. 100 |
//! | 2 | `sat` | % | 0 .. 100 |
//! | 3 | `bias` | % | 0 .. 100 |
//! | 4 | `wow` | % | 0 .. 100 |
//! | 5 | `flutr` | % | 0 .. 100 |
//! | 6 | `bump` | dB | 0 .. 3 |
//! | 7 | `azimth` | ° | 0 .. 1 |
//! | 8 | `hiss` | % | 0 .. 100 |
//! | 9 | `trim` | dB | −24 .. +24 |
//! | 10 | `mkauto` | — | 0 or 1 |
//! | 11 | `mix` | % | 0 .. 100 |
//!
//! Twelve controls, and the order is the tape's, not this file's.

use phosphor_core::fx::{Effect, FxContext, FxParamInfo};
use phosphor_dsp::fx::tape::{natural_param, Tape as TapeCore, PARAM_COUNT};

/// The rate a tape is built at before it reaches a slot.
///
/// A placeholder, and never the rate it runs at: the mixer calls
/// [`Effect::init`] with the device's rate before the effect is in the signal
/// path, and that rebuilds the line and redesigns every filter. It exists
/// because the record EQ has to be designed at *some* rate, and designing it
/// at a plausible one keeps a chain that is inspected before it is installed
/// from reading as nonsense.
const DESIGN_RATE: f64 = 48_000.0;

/// A tape machine in an insert slot.
///
/// A newtype and nothing else. What this adds is the trait, the stable name a
/// session stores it under, and the promise that the parameters crossing the
/// boundary are in the units the insert layer says they are.
pub struct Tape(TapeCore);

impl Tape {
    /// The stable name a session stores this under, and the same string
    /// [`crate::state::FxType::Tape`] answers `key()` with. The two have to
    /// match: one is what the file says and the other is what the audio
    /// thread answers to.
    pub const NAME: &'static str = "tape";

    #[must_use]
    pub fn new() -> Self {
        Self(TapeCore::new(DESIGN_RATE))
    }
}

impl Default for Tape {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Tape {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    /// The block size is not used: the line is sized from the *rate* and from
    /// the deepest wobble the controls allow, not from how the device happens
    /// to cut the audio up.
    ///
    /// `snap` after it on purpose, and it is the same argument the other four
    /// make: a session load sets the controls before the slot exists, and
    /// those controls are glide targets. Snapping means the first block a
    /// loaded session renders is the tape that was saved rather than the
    /// factory one gliding towards it. Nothing is audible yet — the effect is
    /// not in the chain — so there is nothing to protect from the jump.
    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.0.set_sample_rate(sample_rate);
        self.0.reset();
        self.0.snap();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &FxContext<'_>) {
        self.0.process(left, right);
    }

    fn reset(&mut self) {
        self.0.reset();
    }

    fn parameter_count(&self) -> usize {
        PARAM_COUNT
    }

    fn parameter_info(&self, index: usize) -> Option<FxParamInfo> {
        natural_param(index).map(|p| FxParamInfo {
            name: p.name,
            unit: p.unit,
            min: p.min,
            max: p.max,
            default: p.default,
        })
    }

    fn get_parameter(&self, index: usize) -> f32 {
        self.0.param_natural(index)
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        self.0.set_param_natural(index, value);
    }

    /// **Zero, and it is a design constraint rather than a happy accident.**
    ///
    /// The oversampling filters are minimum phase so that the pair carries
    /// 2.4 samples of group delay instead of the 15 a linear-phase halfband
    /// would, and the wobbling line is bypassed entirely when the transport
    /// controls are at zero. Fifty microseconds is below any sensible
    /// threshold for delay compensation, and there is no delay compensation
    /// in this box to report it to.
    ///
    /// With the transport running the read head sits a wow excursion further
    /// back — a quarter of a millisecond at the factory setting — and that is
    /// still not latency, because it *moves*. Reporting a moving offset as a
    /// fixed one would shift the whole track by the average of a wobble.
    fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::fx::tape::{
        default_natural_params, Speed, PARAM_AUTO_MAKEUP, PARAM_AZIMUTH_DEG, PARAM_BIAS,
        PARAM_BUMP_DB, PARAM_DRIVE, PARAM_FLUTTER, PARAM_HISS, PARAM_MIX, PARAM_SAT, PARAM_SPEED,
        PARAM_TRIM_DB, PARAM_WOW,
    };

    const FS: f64 = 44_100.0;

    fn installed() -> Tape {
        let mut tape = Tape::new();
        tape.init(FS, 512);
        tape
    }

    fn context() -> FxContext<'static> {
        FxContext::bare(FS as f32)
    }

    /// A tone through the slot, in blocks.
    fn render(effect: &mut dyn Effect, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        let ctx = context();
        let mut at = 0;
        while at < input.len() {
            let end = (at + 256).min(input.len());
            let mut left = input[at..end].to_vec();
            let mut right = input[at..end].to_vec();
            effect.process(&mut left, &mut right, &ctx);
            out.extend_from_slice(&left);
            at = end;
        }
        out
    }

    fn tone(hz: f64, amplitude: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| (amplitude * (std::f64::consts::TAU * hz * n as f64 / FS).sin()) as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    /// The name is the session's key, and the two have to be the same string
    /// or a saved chain loads as nothing.
    #[test]
    fn the_name_is_the_session_key() {
        assert_eq!(Tape::new().name(), Tape::NAME);
        assert_eq!(Tape::NAME, crate::state::FxType::Tape.key());
    }

    /// The twelve controls are the ones the tape declares, in its order, with
    /// its defaults — read from the effect rather than from a table here, so
    /// a factory setting that moves cannot leave a stale copy.
    #[test]
    fn it_declares_the_tapes_own_controls() {
        let tape = installed();
        assert_eq!(tape.parameter_count(), PARAM_COUNT);
        assert_eq!(tape.parameter_count(), 12);
        assert!(tape.parameter_info(PARAM_COUNT).is_none());
        assert!(!tape.wants_key(), "the tape has no sidechain");
        assert_eq!(tape.latency(), 0, "an insert that shifts the track is a bug");
        assert!(tape.gr_meter().is_none(), "the tape does not reduce gain");

        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = tape.parameter_info(index).expect("a control at every index");
            assert_eq!(tape.get_parameter(index), default, "index {index}");
            assert_eq!(info.default, default, "index {index} default");
        }
        // The house frame, spot-checked in the units a person reads.
        let info = tape.parameter_info(PARAM_SPEED).unwrap();
        assert_eq!((info.name, info.default), ("speed", Speed::Studio.index() as f32));
        let info = tape.parameter_info(PARAM_DRIVE).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("drive", "%", 50.0));
        let info = tape.parameter_info(PARAM_BUMP_DB).unwrap();
        assert_eq!((info.name, info.unit, info.default, info.max), ("bump", "dB", 1.5, 3.0));
        let info = tape.parameter_info(PARAM_AZIMUTH_DEG).unwrap();
        assert_eq!((info.name, info.default), ("azimth", 0.0), "azimuth ships true");
        let info = tape.parameter_info(PARAM_HISS).unwrap();
        assert_eq!((info.name, info.default), ("hiss", 0.0), "the noise ships off");
        let info = tape.parameter_info(PARAM_MIX).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("mix", "%", 100.0));
    }

    /// **A tape in a slot saturates**, and it does it without changing the
    /// level — which is the whole of what the automatic makeup is for.
    #[test]
    fn a_tape_in_a_slot_saturates_without_getting_louder() {
        let mut tape = installed();
        // The transport is stopped for this one, and it has to be: the
        // factory wow is 0.1% at 0.6 Hz, which moves a 1 kHz tone by a hertz
        // either way over a cycle a second and a half long, and a single DFT
        // bin measured over half a second of that reads whatever part of the
        // wow cycle it happened to catch. The medium is what is under test
        // here; the transport is measured where it lives.
        tape.set_parameter(PARAM_WOW, 0.0);
        tape.set_parameter(PARAM_FLUTTER, 0.0);
        tape.init(FS, 512);
        let input = tone(1000.0, 0.25, 32_768);
        let out = render(&mut tape, &input);
        let settled = &out[8_192..];
        let source = &input[8_192..];

        // Within a decibel, and the decibel is real: the makeup is lined up
        // on *programme material*, and a sine at −12 dBFS has a third of the
        // crest factor of music at the same peak, so it drives the medium far
        // harder and loses more of the fundamental to harmonics. The
        // programme-level match is measured where it means something, in the
        // tape's own tests.
        let level = 20.0 * (rms(settled) / rms(source)).log10();
        assert!(level.abs() < 1.0, "the tape moved the level by {level:+.3} dB");

        // Third harmonic, and no second: a magnetised medium is odd.
        let bin = |x: &[f32], hz: f64| {
            let n = x.len();
            let (mut re, mut im, mut norm) = (0.0f64, 0.0, 0.0);
            for (i, s) in x.iter().enumerate() {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                let p = std::f64::consts::TAU * hz * i as f64 / FS;
                re += f64::from(*s) * w * p.cos();
                im -= f64::from(*s) * w * p.sin();
                norm += w;
            }
            2.0 * (re * re + im * im).sqrt() / norm
        };
        let fundamental = bin(settled, 1000.0);
        assert!(bin(settled, 3000.0) / fundamental > 0.002, "no third harmonic at all");
        assert!(bin(settled, 2000.0) / fundamental < 0.0001, "a second harmonic appeared");
    }

    /// A tape nobody has turned up is inaudible, sample for sample. Adding
    /// one to a chain while the transport is rolling must not change the mix.
    #[test]
    fn a_tape_at_wet_zero_is_bit_identical_to_no_effect() {
        let mut tape = installed();
        tape.set_parameter(PARAM_MIX, 0.0);
        tape.set_parameter(PARAM_DRIVE, 100.0);
        tape.set_parameter(PARAM_HISS, 100.0);
        tape.init(FS, 512);
        let ctx = context();
        let source: Vec<f32> = (0..2048)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let mut left = source.clone();
        let mut right = source.clone();
        tape.process(&mut left, &mut right, &ctx);
        for (index, (before, after)) in source.iter().zip(&left).enumerate() {
            assert_eq!(before.to_bits(), after.to_bits(), "sample {index}: {before} -> {after}");
        }
        assert_eq!(right, source);
    }

    /// A whole vector written back in index order restores the instance,
    /// which is the session load path.
    #[test]
    fn a_parameter_vector_round_trips_in_index_order() {
        let mut source = installed();
        let written = [
            (PARAM_SPEED, Speed::Slow.index() as f32),
            (PARAM_DRIVE, 82.0),
            (PARAM_SAT, 17.0),
            (PARAM_BIAS, 64.0),
            (PARAM_WOW, 21.0),
            (PARAM_FLUTTER, 93.0),
            (PARAM_BUMP_DB, 2.5),
            (PARAM_AZIMUTH_DEG, 0.6),
            (PARAM_HISS, 40.0),
            (PARAM_TRIM_DB, -3.5),
            (PARAM_AUTO_MAKEUP, 0.0),
            (PARAM_MIX, 55.0),
        ];
        assert_eq!(written.len(), PARAM_COUNT, "a control was left out of the round trip");
        for (index, value) in written {
            source.set_parameter(index, value);
        }
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.get_parameter(i)).collect();

        let mut restored = Tape::new();
        for (index, &value) in saved.iter().enumerate() {
            restored.set_parameter(index, value);
        }
        restored.init(FS, 512);
        let read_back: Vec<f32> = (0..PARAM_COUNT).map(|i| restored.get_parameter(i)).collect();
        assert_eq!(saved, read_back, "the vector did not survive a round trip");
        for (index, value) in written {
            assert_eq!(read_back[index], value, "index {index}");
        }
    }

    /// Nonsense from a UI or a hand-edited session file is refused, not
    /// propagated into a differential equation.
    #[test]
    fn it_survives_nonsense() {
        let mut tape = installed();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| tape.get_parameter(i)).collect();
        tape.set_parameter(PARAM_COUNT, 1.0);
        tape.set_parameter(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            tape.set_parameter(index, f32::NAN);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| tape.get_parameter(i)).collect();
        assert_eq!(before, after);
        assert_eq!(tape.get_parameter(PARAM_COUNT), 0.0);

        // A rate the device could not have asked for leaves the tape built at
        // the last one it was given, and still sounding.
        tape.init(0.0, 64);
        tape.init(f64::NAN, 64);
        tape.init(FS, 512);
        let out = render(&mut tape, &tone(440.0, 0.3, 4096));
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(out.iter().any(|s| s.abs() > 0.1), "the tape went silent");
    }

    /// Reset drops the tail and keeps the controls, which is what a transport
    /// stop and a panic both need.
    #[test]
    fn reset_demagnetises_and_keeps_the_controls() {
        let mut tape = installed();
        tape.set_parameter(PARAM_DRIVE, 100.0);
        tape.init(FS, 512);
        let _ = render(&mut tape, &tone(220.0, 0.9, 8192));

        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| tape.get_parameter(i)).collect();
        tape.reset();
        let ctx = context();
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        tape.process(&mut left, &mut right, &ctx);
        assert_eq!(left.iter().fold(0.0f32, |a, v| a.max(v.abs())), 0.0, "the tail survived");
        assert_eq!(right.iter().fold(0.0f32, |a, v| a.max(v.abs())), 0.0);
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| tape.get_parameter(i)).collect();
        assert_eq!(before, after, "the flush moved a control");
    }
}
