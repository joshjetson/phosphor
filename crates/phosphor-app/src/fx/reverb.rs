//! The reverb, wearing the insert layer's trait.
//!
//! # Why the adapter is here and not next to the tank
//!
//! The same reason the EQ's is, and the reason is worth repeating rather than
//! cross-referencing: [`phosphor_dsp::fx::reverb::Reverb`] is the reverb, and
//! this is the fifty lines that let a chain slot hold one. The trait lives in
//! `phosphor-core`; `phosphor-dsp` does not depend on `phosphor-core` and must
//! not start, because `phosphor-core` is the engine — cpal, the mixer, the
//! transport — and `phosphor-dsp` is arithmetic that runs anywhere. This
//! crate already depends on both, and the registry beside this file is
//! already where an effect type becomes an effect, so the adapter sits at the
//! junction and is *only* an adapter.
//!
//! # The units on the wire
//!
//! Natural units, as the insert layer requires — a session stores what a
//! control meant, not a knob fraction that re-points the day a range moves.
//!
//! | index | control | unit | travel |
//! |---|---|---|---|
//! | 0 | `alg` | — | 0..3, a position in `Algorithm::ALL` |
//! | 1 | `predly` | ms | 0 .. 500 |
//! | 2 | `decay` | s | 0.2 .. 20 |
//! | 3 | `size` | — | 0.25 .. 2.0 |
//! | 4 | `damp` | Hz | 1 000 .. 20 000 |
//! | 5 | `locut` | Hz | 20 .. 1 000 |
//! | 6 | `early` | % | 0 .. 100 |
//! | 7 | `diff` | % | 0 .. 100 |
//! | 8 | `mrate` | Hz | 0.05 .. 5 |
//! | 9 | `mdepth` | % | 0 .. 100 |
//! | 10 | `width` | % | 0 .. 100 |
//! | 11 | `mix` | % | 0 .. 100 |
//!
//! Twelve controls, and the order is the reverb's, not this file's.

use phosphor_core::fx::{Effect, FxContext, FxParamInfo};
use phosphor_dsp::fx::reverb::{natural_param, Reverb as ReverbCore, PARAM_COUNT};

/// The rate a reverb is built at before it reaches a slot.
///
/// A placeholder, and never the rate it runs at: the mixer calls
/// [`Effect::init`] with the device's rate before the effect is in the signal
/// path, and that rebuilds every delay line. It exists because a reverb has
/// to allocate its buffers at *some* rate, and building at a plausible one
/// keeps a chain that is inspected before it is installed from reading as
/// nonsense.
const DESIGN_RATE: f64 = 48_000.0;

/// A reverb in an insert slot.
///
/// A newtype and nothing else. What this adds is the trait, the stable name a
/// session stores it under, and the promise that the parameters crossing the
/// boundary are in the units the insert layer says they are.
pub struct Reverb(ReverbCore);

impl Reverb {
    /// The stable name a session stores this under, and the same string
    /// [`crate::state::FxType::Reverb`] answers `key()` with. The two have to
    /// match: one is what the file says and the other is what the audio
    /// thread answers to.
    pub const NAME: &'static str = "reverb";

    #[must_use]
    pub fn new() -> Self {
        Self(ReverbCore::new(DESIGN_RATE))
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Reverb {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    /// The block size is not used: a reverb's buffers are sized from the
    /// *rate* and from the longest predelay, size and modulation depth its
    /// controls allow, not from how the device happens to cut the audio up.
    /// The rate is used, and it is the whole of what this call is for —
    /// every delay length in the instance is wrong until it happens.
    ///
    /// `snap` after it on purpose, and it is the same argument the EQ makes:
    /// a session load sets the controls before the slot exists, and those
    /// controls are crossfade targets. Snapping them means the first block a
    /// loaded session renders is the reverb that was saved rather than the
    /// factory one crossfading towards it. Nothing is audible yet — the
    /// effect is not in the chain — so there is nothing to protect from the
    /// jump.
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

    /// A parameter arriving from the UI takes the knob path: geometry moves
    /// crossfade in 30 ms and coefficients glide. This is what a player
    /// turning a control gets, and it is why `size` can be dragged across its
    /// whole travel under a running tail without a comb.
    fn set_parameter(&mut self, index: usize, value: f32) {
        self.0.set_param_natural(index, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::fx::reverb::{
        default_natural_params, Algorithm, PARAM_ALGORITHM, PARAM_DECAY_S, PARAM_MIX,
        PARAM_PREDELAY_MS, PARAM_SIZE,
    };

    const FS: f64 = 44_100.0;

    fn installed() -> Reverb {
        let mut verb = Reverb::new();
        verb.init(FS, 512);
        verb
    }

    /// One impulse, then `blocks` of silence.
    ///
    /// Every caller sets the wet/dry to 100%, where the mix crossfade has
    /// already taken the dry out, so what comes back is the wet.
    fn tail(effect: &mut dyn Effect, blocks: usize) -> Vec<f32> {
        let ctx = FxContext::bare(FS as f32);
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        left[0] = 1.0;
        right[0] = 1.0;
        let mut out = Vec::with_capacity(blocks * 256);
        for block in 0..blocks {
            if block > 0 {
                left.fill(0.0);
                right.fill(0.0);
            }
            effect.process(&mut left, &mut right, &ctx);
            out.extend_from_slice(&left);
        }
        out
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len().max(1) as f64)
            .sqrt()
    }

    /// The name is the session's key, and the two have to be the same string
    /// or a saved chain loads as nothing.
    #[test]
    fn the_name_is_the_session_key() {
        assert_eq!(Reverb::new().name(), Reverb::NAME);
        assert_eq!(Reverb::NAME, crate::state::FxType::Reverb.key());
    }

    /// The twelve controls are the ones the reverb declares, in its order,
    /// with its defaults — read from the effect rather than from a table
    /// here, so a factory setting that moves cannot leave a stale copy.
    #[test]
    fn it_declares_the_reverbs_own_controls() {
        let verb = installed();
        assert_eq!(verb.parameter_count(), PARAM_COUNT);
        assert_eq!(verb.parameter_count(), 12);
        assert!(verb.parameter_info(PARAM_COUNT).is_none());
        assert!(!verb.wants_key(), "a reverb has no use for a sidechain");
        assert_eq!(verb.latency(), 0, "an insert reverb that delays the mix is a bug");

        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = verb.parameter_info(index).expect("a control at every index");
            assert_eq!(verb.get_parameter(index), default, "index {index}");
            assert_eq!(info.default, default, "index {index} default");
        }
        // The house frame, spot-checked in the units a person reads.
        let info = verb.parameter_info(PARAM_DECAY_S).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("decay", "s", 1.8));
        let info = verb.parameter_info(PARAM_PREDELAY_MS).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("predly", "ms", 20.0));
        let info = verb.parameter_info(PARAM_MIX).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("mix", "%", 25.0));
        assert_eq!(verb.get_parameter(PARAM_ALGORITHM), 0.0, "the plate is the default");
    }

    /// A reverb in a slot makes a tail — and the tail is longer when the knob
    /// says it should be.
    #[test]
    fn a_reverb_in_a_slot_rings() {
        let mut verb = installed();
        verb.set_parameter(PARAM_MIX, 100.0);
        // Straight to the destination: nothing is in the signal path yet.
        verb.init(FS, 512);
        let short = tail(&mut verb, 400);

        let mut verb = installed();
        verb.set_parameter(PARAM_MIX, 100.0);
        verb.set_parameter(PARAM_DECAY_S, 8.0);
        verb.init(FS, 512);
        let long = tail(&mut verb, 400);

        let window = 300 * 256..400 * 256;
        let quiet = rms(&short[window.clone()]);
        let loud = rms(&long[window]);
        assert!(quiet > 0.0, "the reverb made no sound at all");
        assert!(
            loud > quiet * 4.0,
            "an 8 s decay is only {:.1}x the 1.8 s one two seconds in",
            loud / quiet
        );
    }

    /// A reverb nobody has turned up is inaudible, sample for sample. Adding
    /// one to a chain while the transport is rolling must not change the mix.
    #[test]
    fn a_reverb_at_wet_zero_is_bit_identical_to_no_effect() {
        let mut verb = installed();
        verb.set_parameter(PARAM_MIX, 0.0);
        verb.init(FS, 512);
        let ctx = FxContext::bare(FS as f32);
        let source: Vec<f32> = (0..2048)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let mut left = source.clone();
        let mut right = source.clone();
        verb.process(&mut left, &mut right, &ctx);
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
            (PARAM_ALGORITHM, Algorithm::Hall.index() as f32),
            (PARAM_PREDELAY_MS, 137.0),
            (PARAM_DECAY_S, 6.25),
            (PARAM_SIZE, 1.65),
            (4, 9_500.0),
            (5, 180.0),
            (6, 62.0),
            (7, 44.0),
            (8, 2.5),
            (9, 71.0),
            (10, 80.0),
            (PARAM_MIX, 100.0),
        ];
        for (index, value) in written {
            source.set_parameter(index, value);
        }
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.get_parameter(i)).collect();

        let mut restored = Reverb::new();
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
    /// propagated into a delay length.
    #[test]
    fn it_survives_nonsense() {
        let mut verb = installed();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| verb.get_parameter(i)).collect();
        verb.set_parameter(PARAM_COUNT, 1.0);
        verb.set_parameter(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            verb.set_parameter(index, f32::NAN);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| verb.get_parameter(i)).collect();
        assert_eq!(before, after);
        assert_eq!(verb.get_parameter(PARAM_COUNT), 0.0);

        // A rate the device could not have asked for leaves the reverb built
        // at the last one it was given, and still ringing.
        verb.init(0.0, 64);
        verb.init(f64::NAN, 64);
        verb.set_parameter(PARAM_MIX, 100.0);
        verb.init(FS, 512);
        let out = tail(&mut verb, 200);
        assert!(rms(&out[100 * 256..]) > 0.0, "the reverb went silent");
        assert!(out.iter().all(|s| s.is_finite()));
    }
}
