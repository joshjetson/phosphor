//! The eight-band parametric, wearing the insert layer's trait.
//!
//! # Why the adapter is here and not next to the filter
//!
//! [`phosphor_dsp::fx::eq::ParametricEq`] is the EQ; this is the fifty lines
//! that let a chain slot hold one. The obvious home for those lines is beside
//! the filter, and that is where they would be if the dependency graph
//! allowed it: the trait lives in `phosphor-core`, `phosphor-dsp` does not
//! depend on `phosphor-core`, and it must not start. `phosphor-core` is the
//! engine — cpal, the mixer, the transport — and `phosphor-dsp` is arithmetic
//! that runs anywhere. Pointing the arithmetic at the engine to satisfy one
//! trait impl would drag an audio device driver into the crate the DX7 lives
//! in, and would put the two crates in a cycle held open only by
//! `phosphor-core`'s dev-dependency on `phosphor-dsp`.
//!
//! This crate already depends on both, and the registry beside this file is
//! already the place where an effect type becomes an effect. So the adapter
//! sits at the junction, and it is *only* an adapter: every number in it
//! comes from the EQ's own published laws. There is no unit arithmetic here
//! to get wrong.
//!
//! # The units on the wire
//!
//! The insert layer's contract is natural units — "a session stores what a
//! control *meant*". The EQ's flat surface has two views of every control and
//! this one takes the natural one, [`ParametricEq::set_param_natural`], which
//! is hertz, decibels and dB per octave. The 0..1 view exists for presets and
//! automation and is deliberately not what a slot's parameters are.
//!
//! | flat index | control | unit | travel |
//! |---|---|---|---|
//! | `b*6 + 0` | type | — | 0..7, a position in `BandType::ALL` |
//! | `b*6 + 1` | freq | Hz | 10 .. 30 000 |
//! | `b*6 + 2` | gain | dB | −18 .. +18 |
//! | `b*6 + 3` | q | — | 0.1 .. 40 |
//! | `b*6 + 4` | slope | dB/oct | 6, 12 or 24, whichever the type offers |
//! | `b*6 + 5` | on | — | 0 or 1 |
//! | 48 | trim | dB | −24 .. +24 |
//!
//! Eight bands of six, then the output trim: forty-nine controls, and the
//! order is the EQ's, not this file's.

use phosphor_core::fx::{Effect, FxContext, FxParamInfo};
use phosphor_dsp::fx::eq::{natural_param, ParametricEq, PARAM_COUNT};

/// The rate an EQ is designed at before it reaches a slot.
///
/// A placeholder, and never the rate it runs at: the mixer calls
/// [`Effect::init`] with the device's rate before the effect is in the signal
/// path, and that redesigns every band. It exists because a filter has to be
/// built at *some* rate, and building at a plausible one keeps a chain that
/// is inspected before it is installed from reading as nonsense.
const DESIGN_RATE: f64 = 48_000.0;

/// An eight-band parametric EQ in an insert slot.
///
/// A newtype and nothing else. Everything it does is
/// [`ParametricEq`]'s; what this adds is the trait, the stable name a session
/// stores it under, and the promise that the parameters crossing the boundary
/// are in the units the insert layer says they are.
pub struct Eq(ParametricEq);

impl Eq {
    /// The stable name a session stores this under, and the same string
    /// [`crate::state::FxType::Eq`] answers `key()` with. The two have to
    /// match: one is what the file says and the other is what the audio
    /// thread answers to.
    pub const NAME: &'static str = "eq";

    #[must_use]
    pub fn new() -> Self {
        Self(ParametricEq::new(DESIGN_RATE))
    }
}

impl Default for Eq {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Eq {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    /// The block size is not used: the EQ works in fixed 32-sample control
    /// blocks on the stack, so there is no buffer to size and nothing to
    /// allocate. The rate is used, and it is the whole of what this call is
    /// for — every coefficient in the instance is wrong until it happens.
    ///
    /// `reset` after it on purpose. A session load sets an EQ's parameters
    /// before the slot exists, and those parameters are smoothed targets; the
    /// reset snaps them and designs from them, so the first block a loaded
    /// session renders is the EQ that was saved rather than the factory one
    /// gliding towards it over 15 ms. Nothing is audible yet — the effect is
    /// not in the chain — so there is nothing to protect from the jump.
    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.0.set_sample_rate(sample_rate);
        self.0.reset();
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

    /// The band number is not in the name. A caller that wants "band 3 freq"
    /// has the band from `phosphor_dsp::fx::eq::param_address` and can format
    /// it without this call allocating on its behalf — the same convention
    /// the EQ's own `param_name` follows, for the same reason.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::fx::eq::{
        default_natural_params, eq_response_db, BandParam, BandType, BAND_COUNT,
    };

    const FS: f64 = 44_100.0;

    fn installed() -> Eq {
        let mut eq = Eq::new();
        eq.init(FS, 512);
        eq
    }

    /// A block of a sine at `freq`, rendered through the effect, measured
    /// against the same block rendered through nothing.
    ///
    /// A single-bin DFT of a Hann-windowed steady state: the level of one
    /// frequency, which is what a filter's gain means. Peak would do for a
    /// pure tone but not for the comparison this file's callers make, and RMS
    /// of a filtered tone is only the gain if nothing else is in the signal.
    fn gain_db_at(effect: &mut dyn Effect, freq: f64) -> f64 {
        const CHUNK: usize = 256;
        const WARMUP: usize = 20_000;
        const ANALYSIS: usize = 32_768;
        let w = 2.0 * std::f64::consts::PI * freq / FS;
        let ctx = FxContext::bare(FS as f32);
        let mut l = [0.0f32; CHUNK];
        let mut r = [0.0f32; CHUNK];

        let mut n = 0usize;
        while n < WARMUP {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let s = (w * (n + i) as f64).sin() as f32;
                *a = s;
                *b = s;
            }
            effect.process(&mut l, &mut r, &ctx);
            n += CHUNK;
        }

        let (mut in_re, mut in_im, mut out_re, mut out_im) = (0.0, 0.0, 0.0, 0.0);
        let start = n;
        let mut k = 0usize;
        while k < ANALYSIS {
            for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let s = (w * (start + k + i) as f64).sin() as f32;
                *a = s;
                *b = s;
            }
            let dry = l;
            effect.process(&mut l, &mut r, &ctx);
            for i in 0..CHUNK {
                if k + i >= ANALYSIS {
                    break;
                }
                let idx = k + i;
                let win = 0.5
                    - 0.5 * (2.0 * std::f64::consts::PI * idx as f64 / ANALYSIS as f64).cos();
                let phase = w * (start + idx) as f64;
                let (c, s) = (phase.cos() * win, -phase.sin() * win);
                in_re += f64::from(dry[i]) * c;
                in_im += f64::from(dry[i]) * s;
                out_re += f64::from(l[i]) * c;
                out_im += f64::from(l[i]) * s;
            }
            k += CHUNK;
        }
        10.0 * (out_re.mul_add(out_re, out_im * out_im) / in_re.mul_add(in_re, in_im * in_im))
            .log10()
    }

    /// The name is the session's key, and the two have to be the same string
    /// or a saved chain loads as nothing.
    #[test]
    fn the_name_is_the_session_key() {
        assert_eq!(Eq::new().name(), Eq::NAME);
        assert_eq!(Eq::NAME, crate::state::FxType::Eq.key());
    }

    /// An EQ nobody has touched is a wire, sample for sample, through the
    /// trait as well as through the filter. Adding one to a chain and playing
    /// must not change the mix at all — this is what makes an EQ safe to
    /// insert while the transport is rolling.
    #[test]
    fn a_fresh_eq_is_bit_identical_to_no_effect() {
        let mut eq = installed();
        let ctx = FxContext::bare(FS as f32);
        let source: Vec<f32> = (0..2048)
            .map(|i| (i as f32 * 0.021).sin() * 0.6 + (i as f32 * 0.37).cos() * 0.2)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let mut l = source.clone();
        let mut r = source.clone();
        eq.process(&mut l, &mut r, &ctx);
        for (i, (a, b)) in source.iter().zip(&l).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "sample {i}: {a} -> {b}");
        }
        assert_eq!(r, source);
    }

    /// The forty-nine controls are the ones the EQ declares, in its order,
    /// with its defaults — read from the effect rather than from a table
    /// here, so a factory setting that moves cannot leave a stale copy.
    #[test]
    fn it_declares_the_eqs_own_controls() {
        let eq = installed();
        assert_eq!(eq.parameter_count(), PARAM_COUNT);
        assert_eq!(eq.parameter_count(), 49);
        assert!(eq.parameter_info(PARAM_COUNT).is_none());
        assert!(!eq.wants_key(), "an EQ has no use for a sidechain");
        assert_eq!(eq.latency(), 0, "an insert EQ that delays the mix is a bug");

        let defaults = default_natural_params();
        for (index, &default) in defaults.iter().enumerate() {
            let info = eq.parameter_info(index).expect("a control at every index");
            assert_eq!(eq.get_parameter(index), default, "index {index}");
            assert_eq!(info.default, default, "index {index} default");
        }
        // The factory frame, spot-checked in the units a person reads.
        let info = eq.parameter_info(BandParam::Freq.index(4)).unwrap();
        assert_eq!((info.name, info.unit, info.default), ("freq", "Hz", 2500.0));
        let info = eq.parameter_info(BandParam::Gain.index(4)).unwrap();
        assert_eq!((info.name, info.unit, info.min, info.max), ("gain", "dB", -18.0, 18.0));
        assert_eq!(eq.get_parameter(BandParam::Type.index(0)), BandType::HighPass.index() as f32);
        assert_eq!(eq.get_parameter(BandParam::Enabled.index(0)), 0.0, "the HPF ships off");
    }

    /// The units on the trait are the units at the filter. A parameter set to
    /// 12 is twelve decibels of measured level, not a knob fraction that
    /// happens to be twelve times too big.
    #[test]
    fn a_parameter_in_decibels_is_decibels_of_level() {
        let mut eq = installed();
        eq.set_parameter(BandParam::Gain.index(4), 12.0);
        assert_eq!(eq.get_parameter(BandParam::Gain.index(4)), 12.0);

        let measured = gain_db_at(&mut eq, 2500.0);
        assert!(
            (measured - 12.0).abs() < 0.05,
            "+12 dB on the 2.5 kHz band rendered {measured:+.4} dB"
        );

        // ...and the same number backwards: the trim, which is the whole
        // instance rather than one band.
        let mut eq = installed();
        eq.set_parameter(phosphor_dsp::fx::eq::PARAM_OUTPUT_TRIM, -6.0);
        let measured = gain_db_at(&mut eq, 1000.0);
        assert!(
            (measured - (-6.0)).abs() < 0.02,
            "a -6 dB trim rendered {measured:+.4} dB"
        );
    }

    /// What the UI will draw is what the audio thread is running. The mirror
    /// is built from the parameter vector the UI holds — the audio thread's
    /// EQ is deliberately unreadable from here — so this is the test that the
    /// two cannot drift.
    #[test]
    fn the_ui_mirror_matches_the_installed_effect() {
        let mut eq = installed();
        let moves = [
            (BandParam::Gain.index(4), 9.0),
            (BandParam::Q.index(4), 3.0),
            (BandParam::Freq.index(2), 400.0),
            (BandParam::Gain.index(2), -6.0),
            (BandParam::Enabled.index(0), 1.0),
            (BandParam::Freq.index(0), 80.0),
        ];
        for (index, value) in moves {
            eq.set_parameter(index, value);
        }
        let mirror: Vec<f32> = (0..eq.parameter_count()).map(|i| eq.get_parameter(i)).collect();

        for freq in [50.0, 80.0, 400.0, 2500.0, 9000.0] {
            let drawn = eq_response_db(&mirror, FS, freq);
            let rendered = gain_db_at(&mut eq, freq);
            assert!(
                (drawn - rendered).abs() < 0.05,
                "at {freq} Hz the curve says {drawn:+.4} dB and the render is {rendered:+.4} dB"
            );
        }
    }

    /// A whole vector written back in index order restores the instance —
    /// which is the session load path, and the reason `type` sits ahead of
    /// `slope` in the index space.
    #[test]
    fn a_parameter_vector_round_trips_in_index_order() {
        let mut source = installed();
        for band in 0..BAND_COUNT {
            source.set_parameter(BandParam::Type.index(band), (band % 8) as f32);
            source.set_parameter(BandParam::Slope.index(band), 24.0);
            source.set_parameter(BandParam::Freq.index(band), 120.0 * (band + 1) as f32);
            source.set_parameter(BandParam::Gain.index(band), band as f32 - 4.0);
            source.set_parameter(BandParam::Q.index(band), 0.4 + band as f32);
            source.set_parameter(BandParam::Enabled.index(band), 1.0);
        }
        let saved: Vec<f32> = (0..PARAM_COUNT).map(|i| source.get_parameter(i)).collect();

        let mut restored = Eq::new();
        for (index, &value) in saved.iter().enumerate() {
            restored.set_parameter(index, value);
        }
        restored.init(FS, 512);
        let read_back: Vec<f32> = (0..PARAM_COUNT).map(|i| restored.get_parameter(i)).collect();
        assert_eq!(saved, read_back, "the vector did not survive a round trip");

        for freq in [60.0, 500.0, 3000.0, 12000.0] {
            let a = eq_response_db(&saved, FS, freq);
            let b = eq_response_db(&read_back, FS, freq);
            assert!((a - b).abs() < 1.0e-9, "the restored curve differs at {freq} Hz");
        }
    }

    /// Nonsense from a UI or a hand-edited session file is refused, not
    /// propagated into a filter design.
    #[test]
    fn it_survives_nonsense() {
        let mut eq = installed();
        let before: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.get_parameter(i)).collect();
        eq.set_parameter(PARAM_COUNT, 1.0);
        eq.set_parameter(usize::MAX, 1.0);
        for index in 0..PARAM_COUNT {
            eq.set_parameter(index, f32::NAN);
        }
        let after: Vec<f32> = (0..PARAM_COUNT).map(|i| eq.get_parameter(i)).collect();
        assert_eq!(before, after);
        assert_eq!(eq.get_parameter(PARAM_COUNT), 0.0);

        // A rate the device could not have asked for leaves the EQ designed
        // at the last one it was given.
        eq.init(0.0, 64);
        eq.init(f64::NAN, 64);
        let mut probe = eq;
        probe.set_parameter(phosphor_dsp::fx::eq::BandParam::Gain.index(4), 6.0);
        let measured = gain_db_at(&mut probe, 2500.0);
        assert!((measured - 6.0).abs() < 0.05, "rendered {measured:+.4} dB");
    }
}
