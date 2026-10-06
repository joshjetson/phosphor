//! Remembering the last answer an expensive, pure calculation gave.
//!
//! A lot of what the instruments compute every sample depends only on a
//! knob: a filter coefficient that is a `tan` of the panel's cutoff, a pan
//! law that is a `sin` and a `cos` of the panel's spread, a resonance that is
//! a `powf` of the panel's knob. The knob moves when a finger does, and the
//! sample clock runs 48 000 times a second, so almost every one of those
//! calls returns exactly what it returned a sample ago.
//!
//! [`Memo`] keeps the last inputs beside the answer and only calls the
//! calculation again when the inputs differ. It is keyed on the inputs
//! themselves rather than invalidated by the code that changes them, which
//! is what makes it safe to drop in: there is no setter, no program change,
//! no snap and no rate change that has to remember to clear it, because a
//! different input *is* the invalidation. And since the answer it returns is
//! the one the very same expression produced for the very same inputs, the
//! output is bit for bit what it was without it.
//!
//! Floating-point inputs go in as their bits (`f64::to_bits`), never as
//! values: `0.0 == -0.0` is true but `tan(-0.0)` is `-0.0`, and a key that
//! treated the two as one input could hand back the wrong sign. A NaN's bits
//! compare equal to themselves too, so a NaN input is computed once rather
//! than on every sample — and whatever it computes is what it computed
//! before.

/// The last answer, and the inputs that gave it. See the module notes.
#[derive(Debug, Clone, Copy)]
pub struct Memo<K, V> {
    last: Option<(K, V)>,
}

impl<K, V> Memo<K, V> {
    /// Remembering nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { last: None }
    }
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq + Copy, V: Copy> Memo<K, V> {
    /// What `compute` gives for `key`: remembered if `key` is the key it was
    /// last asked for, computed and remembered if not. `compute` must depend
    /// on nothing but what `key` carries.
    #[inline]
    pub fn get(&mut self, key: K, compute: impl FnOnce() -> V) -> V {
        match self.last {
            Some((seen, value)) if seen == key => value,
            _ => {
                let value = compute();
                self.last = Some((key, value));
                value
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_once_per_distinct_key() {
        let mut memo = Memo::new();
        let mut calls = 0;
        let mut ask = |x: f64| {
            memo.get(x.to_bits(), || {
                calls += 1;
                x.tan()
            })
        };
        assert_eq!(ask(0.25), 0.25f64.tan());
        assert_eq!(ask(0.25), 0.25f64.tan());
        assert_eq!(ask(0.5), 0.5f64.tan());
        assert_eq!(ask(0.25), 0.25f64.tan());
        assert_eq!(calls, 3);
    }

    /// The reason keys are bits: the two zeros are one value and two inputs.
    #[test]
    fn negative_zero_is_its_own_input() {
        let mut memo = Memo::new();
        let mut ask = |x: f64| memo.get(x.to_bits(), || x.tan());
        assert!(ask(0.0).is_sign_positive());
        assert!(ask(-0.0).is_sign_negative());
    }
}
