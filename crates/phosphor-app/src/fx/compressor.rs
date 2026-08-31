//! The compressor, wearing the insert layer's trait.
//!
//! # Why the adapter is here and not next to the detector
//!
//! The same reason the EQ's, the reverb's and the delay's are:
//! [`phosphor_dsp::fx::compressor::Compressor`] is the compressor, and this is
//! the hundred lines that let a chain slot hold one. The trait lives in
//! `phosphor-core`; `phosphor-dsp` does not depend on `phosphor-core` and must
//! not start, because `phosphor-core` is the engine — cpal, the mixer, the
//! transport — and `phosphor-dsp` is arithmetic that runs anywhere. This crate
//! already depends on both, so the adapter sits at the junction.
//!
//! # The two things it does that the other three do not
//!
//! **It reads the key.** [`phosphor_core::fx::FxContext::key`] is another
//! track's signal as its instrument produced it, resolved by the mixer this
//! block, and it goes straight to the detector. [`Effect::wants_key`] answers
//! `true`, which is what makes the mixer bother resolving it at all.
//!
//! **It owns the gain-reduction meter.** The DSP side tracks the worst gain it
//! applied anywhere in the block and stops there; the ballistics — the 300 ms
//! visual release and the 1.5 s peak hold — live here, on the audio thread,
//! and publish through an [`Arc<GrMeter>`] the front end holds the other end
//! of. That split is deliberate: the arithmetic belongs in the DSP crate and
//! the *meter* is an application concept, and putting the ballistics on this
//! side means the compressor and the master limiter draw through exactly the
//! same widget from exactly the same two atomics.
//!
//! # The units on the wire
//!
//! Natural units, as the insert layer requires — a session stores what a
//! control meant, not a knob fraction that re-points the day a range moves.
//!
//! | index | control | unit | travel |
//! |---|---|---|---|
//! | 0 | `char` | — | 0..8, a position in `CHARACTERS` |
//! | 1 | `thresh` | dB | −60 .. 0 |
//! | 2 | `ratio` | % | 0 .. 100, the *slope*: 0 is 1:1 and 100 is ∞:1 |
//! | 3 | `knee` | dB | 0 .. 24 |
//! | 4 | `attack` | ms | 0.05 .. 100 |
//! | 5 | `releas` | ms | 5 .. 3000 |
//! | 6 | `arel` | — | 0..2, off / auto / auto 2 |
//! | 7 | `makeup` | dB | −30 .. +30 |
//! | 8 | `mkauto` | — | 0 or 1 |
//! | 9 | `mix` | % | 0 .. 100 |
//! | 10 | `sense` | — | 0 or 1, peak / rms |
//! | 11 | `schpf` | Hz | 0 (off), else 20 .. 300 |
//!
//! Twelve controls, and the order is the compressor's, not this file's.
//!
//! **The `ratio` control stores a percentage and the panel never shows one.**
//! A session file cannot hold infinity, and the top of a compressor's ratio
//! travel *is* infinity, so what is stored is the slope — `S = 1/R − 1` as a
//! percentage of full limiting — and the panel reads it back out as `3.0:1`
//! and `∞:1`. Both ends are exact numbers rather than clamps, which is what
//! makes the 1:1 null bit-identical and the limiter a real limiter.
//!
//! # What is *not* a parameter here
//!
//! The sidechain **key** and the **key listen** switch. Neither is the
//! compressor's business: which track feeds the detector is routing, which the
//! mixer resolves from a stored track identity every block, and monitoring the
//! key replaces the whole track's output, which a slot cannot do. Both live on
//! the track and on the mixer, which is why they do not appear in the table
//! above and why the panel draws them as two extra rows rather than as
//! controls.

use std::sync::Arc;

use phosphor_core::fx::{Effect, FxContext, FxParamInfo, GrBallistics, GrMeter};
use phosphor_dsp::fx::compressor::{
    natural_param, Compressor as CompressorCore, PARAM_COUNT,
};

/// The rate a compressor is built at before it reaches a slot.
///
/// A placeholder, and never the rate it runs at: the mixer calls
/// [`Effect::init`] with the device's rate before the effect is in the signal
/// path, and that rebuilds every coefficient. It exists because a compressor
/// has to have ballistics at *some* rate, and building at a plausible one
/// keeps a chain that is inspected before it is installed from reading as
/// nonsense.
const DESIGN_RATE: f64 = 48_000.0;

/// A compressor in an insert slot.
pub struct Compressor {
    core: CompressorCore,
    /// The audio thread's half of the meter: the visual release and the peak
    /// hold, folded one block at a time.
    ballistics: GrBallistics,
    /// The UI's half. Cloned once, at construction.
    meter: Arc<GrMeter>,
}

impl Compressor {
    /// The stable name a session stores this under, and the same string
    /// [`crate::state::FxType::Compressor`] answers `key()` with. The two have
    /// to match: one is what the file says and the other is what the audio
    /// thread answers to.
    pub const NAME: &'static str = "comp";

    #[must_use]
    pub fn new() -> Self {
        Self {
            core: CompressorCore::new(DESIGN_RATE),
            ballistics: GrBallistics::new(),
            meter: Arc::new(GrMeter::new()),
        }
    }

    /// The meter this one publishes to. The front end keeps a clone and reads
    /// two atomics to draw the bar.
    #[must_use]
    pub fn meter(&self) -> Arc<GrMeter> {
        self.meter.clone()
    }
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Compressor {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    /// The block size is not used: a compressor has no buffers, only
    /// coefficients, and those come from the *rate*.
    ///
    /// `snap` after it on purpose, and it is the same argument the other three
    /// make: a session load sets the controls before the slot exists, and the
    /// makeup and the parallel mix are ramp targets. Snapping means the first
    /// block a loaded session renders is the compressor that was saved rather
    /// than the factory one walking towards it. Nothing is audible yet — the
    /// effect is not in the chain — so there is nothing to protect from the
    /// jump.
    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.core.set_sample_rate(sample_rate);
        self.core.reset();
        self.core.snap();
        self.ballistics.reset();
        self.meter.reset();
    }

    /// The key comes from the context, and the gain reduction goes back out
    /// through the meter.
    ///
    /// The meter is published every block including the silent ones, because
    /// the visual release is a *time* and a meter that only decayed on blocks
    /// where something happened would freeze on the last transient.
    fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &FxContext<'_>) {
        let frames = left.len().min(right.len());
        self.core.process(left, right, ctx.key);
        self.ballistics.publish(
            &self.meter,
            self.core.block_min_gain(),
            frames,
            ctx.sample_rate,
        );
    }

    fn reset(&mut self) {
        self.core.reset();
        self.ballistics.reset();
        self.meter.reset();
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
        self.core.param_natural(index)
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        self.core.set_param_natural(index, value);
    }

    /// **Yes**, and it is the only effect in the box that says so. The mixer
    /// only resolves a key for chains that ask.
    fn wants_key(&self) -> bool {
        true
    }

    fn gr_meter(&self) -> Option<Arc<GrMeter>> {
        Some(self.meter())
    }

    /// **Zero, and it is load-bearing.** There is no lookahead, and there is
    /// no lookahead because the mixer has no delay compensation: one insert
    /// that shifted its track by five milliseconds would smear it against
    /// every other track, against both sends, and against the dry half of its
    /// own parallel mix.
    fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::fx::compressor::{
        character_params, default_natural_params, percent_to_ratio, ratio_to_percent, Sense,
        PARAM_ATTACK_MS, PARAM_AUTO_MAKEUP, PARAM_AUTO_RELEASE, PARAM_CHARACTER, PARAM_KNEE_DB,
        PARAM_MAKEUP_DB, PARAM_MIX, PARAM_RATIO, PARAM_RELEASE_MS, PARAM_SC_HPF_HZ, PARAM_SENSE,
        PARAM_THRESHOLD_DB,
    };

    const FS: f64 = 44_100.0;

    fn installed() -> Compressor {
        let mut comp = Compressor::new();
        comp.init(FS, 512);
        comp
    }

    fn context<'a>(key: Option<(&'a [f32], &'a [f32])>) -> FxContext<'a> {
        FxContext {
            sample_rate: FS as f32,
            tempo_bpm: 120.0,
            playing: true,
            key,
        }
    }

    fn peak(x: &[f32]) -> f64 {
        x.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max)
    }

    /// The name is the session's key, and the two have to be the same string
    /// or a saved chain loads as nothing.
    #[test]
    fn the_name_is_the_session_key() {
        assert_eq!(Compressor::new().name(), Compressor::NAME);
        assert_eq!(Compressor::NAME, crate::state::FxType::Compressor.key());
    }

    /// The twelve controls are the ones the compressor declares, in its
    /// order, with its defaults — read from the effect rather than from a
    /// table here, so a factory setting that moves cannot leave a stale copy.
    #[test]
    fn it_declares_the_compressors_own_controls() {
        let comp = installed();
        assert_eq!(comp.parameter_count(), PARAM_COUNT);
        assert_eq!(comp.parameter_count(), 12);
        assert!(comp.parameter_info(PARAM_COUNT).is_none());
        assert!(comp.wants_key(), "the compressor is the one effect with a sidechain");
        assert_eq!(comp.latency(), 0, "an insert that shifts the track is a bug");
        assert!(comp.gr_meter().is_some(), "a compressor with no meter");

        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = comp.parameter_info(index).expect("a control at every index");
            assert_eq!(comp.get_parameter(index), default, "index {index}");
            assert_eq!(info.default, default, "index {index} default");
        }
        // The house frame, spot-checked in the units a person reads.
        let info = comp.parameter_info(PARAM_THRESHOLD_DB).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("thresh", "dB", -18.0));
        let info = comp.parameter_info(PARAM_RATIO).unwrap();
        assert_eq!(info.name, "ratio");
        assert_eq!(percent_to_ratio(info.default).round(), 3.0, "3:1 ships");
        let info = comp.parameter_info(PARAM_KNEE_DB).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("knee", "dB", 6.0));
        let info = comp.parameter_info(PARAM_ATTACK_MS).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("attack", "ms", 10.0));
        let info = comp.parameter_info(PARAM_RELEASE_MS).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("releas", "ms", 120.0));
        let info = comp.parameter_info(PARAM_AUTO_MAKEUP).unwrap();
        assert_eq!((info.name, info.default), ("mkauto", 1.0), "auto makeup ships on");
        let info = comp.parameter_info(PARAM_MIX).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("mix", "%", 100.0));
    }

    /// **A compressor in a slot compresses, and the key it is handed is what
    /// it listens to.**
    ///
    /// A quiet track and a loud key: the track ducks. The same quiet track
    /// with no key at all: nothing happens, because the track is nowhere near
    /// the threshold.
    #[test]
    fn a_compressor_in_a_slot_keys_off_the_context() {
        let mut comp = installed();
        comp.set_parameter(PARAM_THRESHOLD_DB, -40.0);
        comp.set_parameter(PARAM_RATIO, ratio_to_percent(8.0));
        comp.set_parameter(PARAM_KNEE_DB, 0.0);
        comp.set_parameter(PARAM_ATTACK_MS, 1.0);
        comp.set_parameter(PARAM_AUTO_MAKEUP, 0.0);
        comp.init(FS, 512);

        // The pad sits at −54 dBFS, well under the −40 dB threshold, so it
        // asks for nothing on its own. The kick is at −0.9 dBFS, which is
        // 39 dB over, and 8:1 turns that into 34 dB of reduction.
        let pad = vec![0.002f32; 512];
        let kick = vec![0.9f32; 512];
        let (mut left, mut right) = (pad.clone(), pad.clone());
        for _ in 0..40 {
            left.copy_from_slice(&pad);
            right.copy_from_slice(&pad);
            comp.process(&mut left, &mut right, &context(Some((&kick, &kick))));
        }
        assert!(
            peak(&left) < 0.0002,
            "the key never ducked the pad: {} of {}",
            peak(&left),
            pad[0]
        );

        comp.reset();
        let (mut left, mut right) = (pad.clone(), pad.clone());
        for _ in 0..40 {
            left.copy_from_slice(&pad);
            right.copy_from_slice(&pad);
            comp.process(&mut left, &mut right, &context(None));
        }
        assert!(
            (peak(&left) - 0.002).abs() < 1.0e-9,
            "with no key the pad was still touched: {}",
            peak(&left)
        );
    }

    /// **The gain reduction reaches the meter, and comes back.**
    ///
    /// The ballistics are on this side of the boundary, so a transient inside
    /// one block is visible even though a UI redraw timer never sampled it.
    #[test]
    fn the_meter_is_published_from_the_audio_side() {
        let mut comp = installed();
        comp.set_parameter(PARAM_THRESHOLD_DB, -40.0);
        comp.set_parameter(PARAM_RATIO, 100.0);
        comp.set_parameter(PARAM_ATTACK_MS, 0.05);
        comp.set_parameter(PARAM_AUTO_MAKEUP, 0.0);
        comp.init(FS, 512);
        let meter = comp.meter();
        assert_eq!(meter.get(), (0.0, 0.0));

        // One full-scale sample at the top of an otherwise silent block: only
        // a block *minimum* can see it, and only audio-side ballistics can
        // hold it long enough to draw.
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        left[0] = 1.0;
        right[0] = 1.0;
        comp.process(&mut left, &mut right, &context(None));
        let (current, peak) = meter.get();
        assert!(current < -20.0, "a full-scale transient published {current:.2} dB");
        assert!(peak <= current);
        assert!(meter.is_active());

        // ...and it releases on silence, which is the other half of a meter.
        let mut left = vec![0.0f32; 512];
        let mut right = vec![0.0f32; 512];
        for _ in 0..400 {
            left.fill(0.0);
            right.fill(0.0);
            comp.process(&mut left, &mut right, &context(None));
        }
        assert_eq!(meter.current_db(), 0.0, "the meter never came home");

        // A reset drops it with the tail.
        comp.reset();
        assert_eq!(meter.get(), (0.0, 0.0));
    }

    /// A compressor at 1:1 is a wire, sample for sample — which is what makes
    /// adding one to a chain while the transport is rolling safe.
    #[test]
    fn a_compressor_at_one_to_one_is_bit_identical_to_no_effect() {
        let mut comp = installed();
        comp.set_parameter(PARAM_RATIO, 0.0);
        comp.init(FS, 512);
        let source: Vec<f32> = (0..2048)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let mut left = source.clone();
        let mut right = source.clone();
        comp.process(&mut left, &mut right, &context(None));
        for (index, (before, after)) in source.iter().zip(&left).enumerate() {
            assert_eq!(before.to_bits(), after.to_bits(), "sample {index}: {before} -> {after}");
        }
        assert_eq!(right, source);
    }

    /// A whole vector written back in index order restores the instance,
    /// which is the session load path — and writing the character selector
    /// first does not overwrite what comes after it.
    #[test]
    fn a_parameter_vector_round_trips_in_index_order() {
        let mut source = installed();
        let written = [
            (PARAM_CHARACTER, 4.0),
            (PARAM_THRESHOLD_DB, -27.5),
            (PARAM_RATIO, 87.5),
            (PARAM_KNEE_DB, 3.5),
            (PARAM_ATTACK_MS, 0.4),
            (PARAM_RELEASE_MS, 640.0),
            (PARAM_AUTO_RELEASE, 2.0),
            (PARAM_MAKEUP_DB, -4.5),
            (PARAM_AUTO_MAKEUP, 0.0),
            (PARAM_MIX, 35.0),
            (PARAM_SENSE, 1.0),
            (PARAM_SC_HPF_HZ, 120.0),
        ];
        assert_eq!(written.len(), PARAM_COUNT, "a control was left out of the round trip");
        for (index, value) in written {
            source.set_parameter(index, value);
        }
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.get_parameter(i)).collect();

        let mut restored = Compressor::new();
        for (index, &value) in saved.iter().enumerate() {
            restored.set_parameter(index, value);
        }
        restored.init(FS, 512);
        let read_back: Vec<f32> = (0..PARAM_COUNT).map(|i| restored.get_parameter(i)).collect();
        assert_eq!(saved, read_back, "the vector did not survive a round trip");
        for (index, value) in written {
            assert_eq!(read_back[index], value, "index {index}");
        }

        // The character selector is a label and not a macro *here*: writing it
        // stores which one and touches nothing else, so a session load that
        // writes index 0 first cannot clobber the eleven values behind it.
        assert_eq!(read_back[PARAM_CHARACTER], 4.0);
        assert_ne!(
            read_back[PARAM_THRESHOLD_DB],
            character_params(4)[PARAM_THRESHOLD_DB],
            "setting the character rewrote the threshold"
        );
    }

    /// Nonsense from a UI or a hand-edited session file is refused, not
    /// propagated into a gain.
    #[test]
    fn it_survives_nonsense() {
        let mut comp = installed();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| comp.get_parameter(i)).collect();
        comp.set_parameter(PARAM_COUNT, 1.0);
        comp.set_parameter(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            comp.set_parameter(index, f32::NAN);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| comp.get_parameter(i)).collect();
        assert_eq!(before, after);
        assert_eq!(comp.get_parameter(PARAM_COUNT), 0.0);

        // A rate the device could not have asked for leaves it built at the
        // last one it was given, and still compressing.
        comp.init(0.0, 64);
        comp.init(f64::NAN, 64);
        comp.set_parameter(PARAM_THRESHOLD_DB, -50.0);
        comp.set_parameter(PARAM_SENSE, Sense::Rms.index() as f32);
        comp.init(FS, 512);
        let mut left = vec![0.5f32; 256];
        let mut right = vec![0.5f32; 256];
        comp.process(&mut left, &mut right, &context(None));
        assert!(left.iter().all(|s| s.is_finite()));

        // A key shorter than the block it is keying is ignored rather than
        // read past the end.
        let short = vec![1.0f32; 8];
        comp.process(&mut left, &mut right, &context(Some((&short, &short))));
        assert!(left.iter().all(|s| s.is_finite()));
    }
}
