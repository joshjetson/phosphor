//! The one effect that ships with the chain itself.

use super::{db_to_gain, Effect, FxContext, FxParamInfo, SILENT_DB};

/// A level trim: one control, in decibels.
///
/// Not in the effect menu, and not meant to be. It is the chain's
/// proof-of-life — the thing that proves a slot runs, a parameter arrives, a
/// bypass crossfades and a session round-trips, without any of that resting
/// on an effect that is still being written. It is also the null-test
/// vehicle: at 0 dB it is required to be a wire, sample for sample, which is
/// a property no reverb can be asked for.
///
/// It stays after the real effects land. A trim in front of a compressor is
/// worth having, and a utility that is exactly one multiply is the cheapest
/// possible regression test for every part of the layer around it.
pub struct Gain {
    db: f32,
    /// `db` as a linear multiplier, computed when the parameter is set so
    /// that `process` is a multiply and nothing else.
    gain: f32,
}

impl Gain {
    /// The stable name a session stores this under.
    pub const NAME: &'static str = "gain";

    /// Top of the control. Above a track fader's +6 dB because this one sits
    /// in front of the fader and is what a quiet source is brought up with.
    pub const MAX_DB: f32 = 24.0;

    #[must_use]
    pub fn new() -> Self {
        Self { db: 0.0, gain: 1.0 }
    }

    /// A trim already set to `db`.
    #[must_use]
    pub fn at(db: f32) -> Self {
        let mut gain = Self::new();
        gain.set_parameter(0, db);
        gain
    }

    #[must_use]
    pub fn db(&self) -> f32 {
        self.db
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::new()
    }
}

impl Effect for Gain {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}

    fn process(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &FxContext<'_>) {
        // Unity is a wire. Multiplying by 1.0 is exact in IEEE-754 and would
        // give the same answer, but not reading the buffer at all is a
        // guarantee rather than a fact about rounding — and it is free.
        if self.gain == 1.0 {
            return;
        }
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s *= self.gain;
        }
    }

    fn reset(&mut self) {}

    fn parameter_count(&self) -> usize {
        1
    }

    fn parameter_info(&self, index: usize) -> Option<FxParamInfo> {
        (index == 0).then_some(FxParamInfo {
            name: "gain",
            unit: "dB",
            min: SILENT_DB,
            max: Self::MAX_DB,
            default: 0.0,
        })
    }

    fn get_parameter(&self, index: usize) -> f32 {
        if index == 0 {
            self.db
        } else {
            0.0
        }
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        if index != 0 || value.is_nan() {
            return;
        }
        self.db = value.clamp(SILENT_DB, Self::MAX_DB);
        self.gain = db_to_gain(self.db);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(gain: &mut Gain, source: &[f32]) -> Vec<f32> {
        let mut l = source.to_vec();
        let mut r = source.to_vec();
        gain.process(&mut l, &mut r, &FxContext::bare(44_100.0));
        assert_eq!(l, r, "a trim is not a panner");
        l
    }

    /// The null test. At 0 dB the trim is not a processor, it is a wire —
    /// and the whole insert layer's "an empty chain changes nothing" claim is
    /// tested through this one.
    #[test]
    fn unity_is_bit_identical() {
        let mut gain = Gain::new();
        let source: Vec<f32> = (0..512)
            .map(|i| (i as f32 * 0.031).sin() * 0.7)
            .chain([0.0, -0.0, f32::MIN_POSITIVE, -f32::MIN_POSITIVE])
            .collect();
        let out = render(&mut gain, &source);
        for (i, (a, b)) in source.iter().zip(&out).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "sample {i}: {a} -> {b}");
        }
    }

    #[test]
    fn decibels_are_the_unit() {
        let mut gain = Gain::at(-6.0);
        assert!((gain.db() - (-6.0)).abs() < 1.0e-6);
        let out = render(&mut gain, &[1.0]);
        assert!((out[0] - 0.501_187).abs() < 1.0e-5, "-6 dB gave {}", out[0]);

        let mut gain = Gain::at(6.0);
        let out = render(&mut gain, &[0.5]);
        assert!((out[0] - 0.997_63).abs() < 1.0e-4, "+6 dB gave {}", out[0]);
    }

    /// Two trims at opposite settings are the identity to within a rounding
    /// step — which is what makes the trim usable as a measuring stick.
    #[test]
    fn opposite_trims_cancel() {
        let mut up = Gain::at(12.0);
        let mut down = Gain::at(-12.0);
        let source: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).cos() * 0.3).collect();
        let mid = render(&mut up, &source);
        let back = render(&mut down, &mid);
        for (a, b) in source.iter().zip(&back) {
            assert!((a - b).abs() < 1.0e-6, "{a} came back as {b}");
        }
    }

    #[test]
    fn the_control_is_clamped_and_survives_nonsense() {
        let mut gain = Gain::new();
        gain.set_parameter(0, 200.0);
        assert_eq!(gain.db(), Gain::MAX_DB);
        gain.set_parameter(0, -900.0);
        assert_eq!(gain.db(), SILENT_DB);
        assert_eq!(render(&mut gain, &[1.0])[0], 0.0, "the bottom of the control is silence");

        gain.set_parameter(0, 0.0);
        gain.set_parameter(0, f32::NAN);
        assert_eq!(gain.db(), 0.0, "a NaN moved the control");
        gain.set_parameter(7, 5.0);
        assert_eq!(gain.db(), 0.0, "an unknown parameter moved the control");
        assert_eq!(gain.get_parameter(7), 0.0);
    }

    #[test]
    fn it_describes_its_one_control() {
        let gain = Gain::new();
        assert_eq!(gain.parameter_count(), 1);
        let info = gain.parameter_info(0).expect("the trim has a control");
        assert_eq!(info.name, "gain");
        assert_eq!(info.unit, "dB");
        assert_eq!(info.default, 0.0);
        assert!(gain.parameter_info(1).is_none());
        assert_eq!(gain.name(), Gain::NAME);
        assert_eq!(gain.latency(), 0);
        assert!(!gain.wants_key());
    }
}
