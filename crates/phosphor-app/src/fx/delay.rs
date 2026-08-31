//! The delay, wearing the insert layer's trait.
//!
//! # Why the adapter is here and not next to the line
//!
//! The same reason the EQ's and the reverb's are:
//! [`phosphor_dsp::fx::delay::Delay`] is the delay, and this is the sixty
//! lines that let a chain slot hold one. The trait lives in `phosphor-core`;
//! `phosphor-dsp` does not depend on `phosphor-core` and must not start,
//! because `phosphor-core` is the engine — cpal, the mixer, the transport —
//! and `phosphor-dsp` is arithmetic that runs anywhere. This crate already
//! depends on both, so the adapter sits at the junction and is *only* an
//! adapter.
//!
//! # The one thing it does that the other two do not
//!
//! It reads the context. [`phosphor_core::fx::FxContext::tempo_bpm`] is the
//! transport's tempo for this block, and it is handed straight to the delay's
//! own `process` so the grid follows a tempo ramp rather than lagging it by
//! however long a UI would take to notice. That parameter is the reason
//! `FxContext` exists.
//!
//! # The units on the wire
//!
//! Natural units, as the insert layer requires — a session stores what a
//! control meant, not a knob fraction that re-points the day a range moves.
//!
//! | index | control | unit | travel |
//! |---|---|---|---|
//! | 0 | `mode` | — | 0..2, a position in `Mode::ALL` |
//! | 1 | `route` | — | 0..2, a position in `Routing::ALL` |
//! | 2 | `sync` | — | 0 or 1 |
//! | 3 | `div` | — | 0..15, a position in `SYNC_LABELS` |
//! | 4 | `time` | ms | 1 .. 5000 |
//! | 5 | `offset` | % | −50 .. +50 |
//! | 6 | `tmode` | — | 0..3, a position in `TimeMode::ALL` |
//! | 7 | `fb` | % | 0 .. 200 |
//! | 8 | `freeze` | — | 0 or 1 |
//! | 9 | `locut` | Hz | 20 .. 2 000 |
//! | 10 | `hicut` | Hz | 200 .. 20 000 |
//! | 11 | `duck` | % | 0 .. 100 |
//! | 12 | `width` | % | 0 .. 200 |
//! | 13 | `heads` | — | 0..6, a position in `HEAD_LABELS` |
//! | 14 | `wander` | % | 0 .. 100 |
//! | 15 | `mix` | % | 0 .. 100 |
//!
//! Sixteen controls, and the order is the delay's, not this file's.

use phosphor_core::fx::{Effect, FxContext, FxParamInfo};
use phosphor_dsp::fx::delay::{natural_param, Delay as DelayCore, PARAM_COUNT};

/// The rate a delay is built at before it reaches a slot.
///
/// A placeholder, and never the rate it runs at: the mixer calls
/// [`Effect::init`] with the device's rate before the effect is in the signal
/// path, and that rebuilds the lines. It exists because a delay has to
/// allocate five seconds of buffer at *some* rate, and building at a plausible
/// one keeps a chain that is inspected before it is installed from reading as
/// nonsense.
const DESIGN_RATE: f64 = 48_000.0;

/// A delay in an insert slot.
///
/// A newtype and nothing else. What this adds is the trait, the stable name a
/// session stores it under, the tempo out of the context, and the promise that
/// the parameters crossing the boundary are in the units the insert layer says
/// they are.
pub struct Delay(DelayCore);

impl Delay {
    /// The stable name a session stores this under, and the same string
    /// [`crate::state::FxType::Delay`] answers `key()` with. The two have to
    /// match: one is what the file says and the other is what the audio thread
    /// answers to.
    pub const NAME: &'static str = "delay";

    #[must_use]
    pub fn new() -> Self {
        Self(DelayCore::new(DESIGN_RATE))
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Delay {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    /// The block size is not used: a delay's line is sized from the *rate* and
    /// from the longest delay its controls allow, not from how the device
    /// happens to cut the audio up.
    ///
    /// `snap` after it on purpose, and it is the same argument the EQ and the
    /// reverb make: a session load sets the controls before the slot exists,
    /// and those controls are glide targets. Snapping means the first block a
    /// loaded session renders is the delay that was saved rather than the
    /// factory one gliding towards it. Nothing is audible yet — the effect is
    /// not in the chain — so there is nothing to protect from the jump.
    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.0.set_sample_rate(sample_rate);
        self.0.reset();
        self.0.snap();
    }

    /// The tempo comes from the context, once per block, and the grid is
    /// resolved from it.
    ///
    /// The alternative — pushing the BPM in as a parameter through
    /// `SetParameter` — works and lags a tempo automation ramp by however long
    /// the UI takes to notice, and a delay whose grid lags the ramp is worse
    /// than one that does not sync at all.
    fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &FxContext<'_>) {
        self.0.process(left, right, f64::from(ctx.tempo_bpm));
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

    /// **Zero, and it is load-bearing.** The wet path is parallel to the dry
    /// one, so the device adds no latency at all — and reporting the delay
    /// *time* as latency, which is the classic mistake, would shift the whole
    /// track by it.
    fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::fx::delay::{
        default_natural_params, Mode, Routing, SYNC_DEFAULT, PARAM_DIVISION, PARAM_FEEDBACK,
        PARAM_HEADS, PARAM_MIX, PARAM_MODE, PARAM_ROUTING, PARAM_SYNC, PARAM_TIME_MS,
    };

    const FS: f64 = 44_100.0;

    fn installed() -> Delay {
        let mut delay = Delay::new();
        delay.init(FS, 512);
        delay
    }

    fn context(bpm: f32) -> FxContext<'static> {
        FxContext {
            sample_rate: FS as f32,
            tempo_bpm: bpm,
            playing: true,
            key: None,
        }
    }

    /// One impulse, then `blocks` of silence, at a tempo.
    fn tail(effect: &mut dyn Effect, blocks: usize, bpm: f32) -> Vec<f32> {
        let ctx = context(bpm);
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

    fn peak(x: &[f32]) -> f64 {
        x.iter().map(|v| f64::from(v.abs())).fold(0.0, f64::max)
    }

    /// Where the loudest sample in a window is.
    fn argmax(x: &[f32], from: usize, to: usize) -> usize {
        let to = to.min(x.len());
        let mut best = from;
        for index in from..to {
            if x[index].abs() > x[best].abs() {
                best = index;
            }
        }
        best
    }

    /// The name is the session's key, and the two have to be the same string
    /// or a saved chain loads as nothing.
    #[test]
    fn the_name_is_the_session_key() {
        assert_eq!(Delay::new().name(), Delay::NAME);
        assert_eq!(Delay::NAME, crate::state::FxType::Delay.key());
    }

    /// The sixteen controls are the ones the delay declares, in its order,
    /// with its defaults — read from the effect rather than from a table here,
    /// so a factory setting that moves cannot leave a stale copy.
    #[test]
    fn it_declares_the_delays_own_controls() {
        let delay = installed();
        assert_eq!(delay.parameter_count(), PARAM_COUNT);
        assert_eq!(delay.parameter_count(), 16);
        assert!(delay.parameter_info(PARAM_COUNT).is_none());
        assert!(!delay.wants_key(), "the delay keys off its own input, not a sidechain");
        assert_eq!(delay.latency(), 0, "an insert delay that shifts the track is a bug");

        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = delay.parameter_info(index).expect("a control at every index");
            assert_eq!(delay.get_parameter(index), default, "index {index}");
            assert_eq!(info.default, default, "index {index} default");
        }
        // The house frame, spot-checked in the units a person reads.
        let info = delay.parameter_info(PARAM_SYNC).unwrap();
        assert_eq!((info.name, info.default), ("sync", 1.0), "sync ships on");
        let info = delay.parameter_info(PARAM_DIVISION).unwrap();
        assert_eq!((info.name, info.default), ("div", SYNC_DEFAULT as f32), "a dotted eighth");
        let info = delay.parameter_info(PARAM_FEEDBACK).unwrap();
        assert_eq!((info.name, info.unit, info.default, info.max), ("fb", "%", 30.0, 200.0));
        let info = delay.parameter_info(9).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("locut", "Hz", 200.0));
        let info = delay.parameter_info(10).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("hicut", "Hz", 6_000.0));
        let info = delay.parameter_info(PARAM_MIX).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("mix", "%", 22.0));
        assert_eq!(delay.get_parameter(PARAM_MODE), 0.0, "digital is the default");
        assert_eq!(delay.get_parameter(PARAM_ROUTING), 0.0, "ping-pong ships off");
    }

    /// **A delay in a slot repeats, on the grid the transport is running.**
    ///
    /// The tempo arrives in the context, so the same effect with the same
    /// controls puts its echo in a different place at a different tempo —
    /// which is the whole reason the context has a tempo in it.
    #[test]
    fn a_delay_in_a_slot_repeats_on_the_transports_grid() {
        for bpm in [90.0f32, 120.0, 174.0] {
            let mut delay = installed();
            delay.set_parameter(PARAM_MIX, 100.0);
            delay.init(FS, 512);
            let out = tail(&mut delay, 200, bpm);
            // A dotted eighth is 0.75 beats.
            let wanted = (0.75 * 60.0 / f64::from(bpm) * FS) as usize;
            let landed = argmax(&out, wanted - 400, wanted + 400);
            assert!(
                (landed as isize - wanted as isize).abs() < 32,
                "{bpm} bpm: the echo landed at {landed}, not {wanted}"
            );
            assert!(peak(&out[wanted - 400..wanted + 400]) > 0.1, "{bpm} bpm: no echo at all");
        }
    }

    /// A delay nobody has turned up is inaudible, sample for sample. Adding
    /// one to a chain while the transport is rolling must not change the mix.
    #[test]
    fn a_delay_at_wet_zero_is_bit_identical_to_no_effect() {
        let mut delay = installed();
        delay.set_parameter(PARAM_MIX, 0.0);
        delay.set_parameter(PARAM_FEEDBACK, 150.0);
        delay.init(FS, 512);
        let ctx = context(120.0);
        let source: Vec<f32> = (0..2048)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let mut left = source.clone();
        let mut right = source.clone();
        delay.process(&mut left, &mut right, &ctx);
        for (index, (before, after)) in source.iter().zip(&left).enumerate() {
            assert_eq!(before.to_bits(), after.to_bits(), "sample {index}: {before} -> {after}");
        }
        assert_eq!(right, source);
    }

    /// A whole vector written back in index order restores the instance, which
    /// is the session load path.
    #[test]
    fn a_parameter_vector_round_trips_in_index_order() {
        let mut source = installed();
        let written = [
            (PARAM_MODE, Mode::Tape.index() as f32),
            (PARAM_ROUTING, Routing::PingPong.index() as f32),
            (PARAM_SYNC, 0.0),
            (PARAM_DIVISION, 11.0),
            (PARAM_TIME_MS, 462.0),
            (5, -35.0),
            (6, 2.0),
            (PARAM_FEEDBACK, 145.0),
            (8, 1.0),
            (9, 90.0),
            (10, 3_500.0),
            (11, 40.0),
            (12, 150.0),
            (PARAM_HEADS, 6.0),
            (14, 55.0),
            (PARAM_MIX, 100.0),
        ];
        assert_eq!(written.len(), PARAM_COUNT, "a control was left out of the round trip");
        for (index, value) in written {
            source.set_parameter(index, value);
        }
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.get_parameter(i)).collect();

        let mut restored = Delay::new();
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
        let mut delay = installed();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.get_parameter(i)).collect();
        delay.set_parameter(PARAM_COUNT, 1.0);
        delay.set_parameter(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            delay.set_parameter(index, f32::NAN);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.get_parameter(i)).collect();
        assert_eq!(before, after);
        assert_eq!(delay.get_parameter(PARAM_COUNT), 0.0);

        // A rate the device could not have asked for leaves the delay built at
        // the last one it was given, and still repeating.
        delay.init(0.0, 64);
        delay.init(f64::NAN, 64);
        delay.set_parameter(PARAM_MIX, 100.0);
        delay.init(FS, 512);
        let out = tail(&mut delay, 100, 120.0);
        assert!(peak(&out) > 0.0, "the delay went silent");
        assert!(out.iter().all(|s| s.is_finite()));

        // ...and a context with no tempo at all still resolves to something
        // finite. `FxContext::bare` reports 120, which is the transport's own
        // default, but a corrupt one must not become a delay of infinity.
        let ctx = FxContext { tempo_bpm: f32::NAN, ..context(120.0) };
        let mut left = vec![0.1f32; 256];
        let mut right = vec![0.1f32; 256];
        delay.process(&mut left, &mut right, &ctx);
        assert!(left.iter().all(|s| s.is_finite()));
    }

    /// Reset drops the tail and keeps the controls, which is what a transport
    /// stop and a panic both need.
    #[test]
    fn reset_flushes_the_line_and_keeps_the_controls() {
        let mut delay = installed();
        delay.set_parameter(PARAM_MIX, 100.0);
        delay.set_parameter(PARAM_FEEDBACK, 190.0);
        delay.init(FS, 512);
        let ringing = tail(&mut delay, 200, 120.0);
        assert!(peak(&ringing[100 * 256..]) > 0.01, "the delay was not ringing to begin with");

        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.get_parameter(i)).collect();
        delay.reset();
        let ctx = context(120.0);
        let mut left = vec![0.0f32; 256];
        let mut right = vec![0.0f32; 256];
        for _ in 0..200 {
            delay.process(&mut left, &mut right, &ctx);
            assert_eq!(peak(&left), 0.0, "the tail survived the flush");
            assert_eq!(peak(&right), 0.0);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| delay.get_parameter(i)).collect();
        assert_eq!(before, after, "the flush moved a control");
    }
}
