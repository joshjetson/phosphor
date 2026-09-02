//! One monotonic clock for MIDI arrival times.
//!
//! Messages are stamped at receipt and aged on the audio thread, and both
//! sides must read the same clock for the arithmetic to mean anything.
//! midir's own timestamps come from a backend-specific epoch that nothing
//! else in the process can reproduce, so they are discarded in favour of
//! this one.

use std::sync::OnceLock;
use std::time::Instant;

static ANCHOR: OnceLock<Instant> = OnceLock::new();

/// Pin the anchor. Called once at startup, before the audio stream exists,
/// so that `now_micros` on the audio thread is a plain read with no
/// first-call initialisation race to wait on.
pub fn init() {
    let _ = ANCHOR.get_or_init(Instant::now);
}

/// Microseconds since the anchor. Monotonic, lock-free after `init`.
pub fn now_micros() -> u64 {
    ANCHOR.get_or_init(Instant::now).elapsed().as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_only_moves_forward() {
        init();
        let a = now_micros();
        let b = now_micros();
        assert!(b >= a);
    }
}
