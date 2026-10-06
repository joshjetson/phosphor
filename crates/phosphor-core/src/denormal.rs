//! Keeping the audio threads out of subnormal arithmetic.
//!
//! A filter, a delay's feedback or a reverb tail that is left to fade decays
//! towards zero forever and never arrives: below about 1e-38 (an `f32`) or
//! 1e-308 (an `f64`) its values become *subnormal*, numbers so small that the
//! floating-point format runs out of exponent and spends mantissa bits
//! instead. Many CPUs handle those in microcode rather than in the ordinary
//! pipeline — on x86 every operation that touches one can cost tens to a
//! hundred times an ordinary one — so a sound that has faded to nothing can
//! make the audio thread suddenly slower than it was while the sound was
//! playing. That is the CPU spike as a Prophet-6 delay dies away.
//!
//! Every audio framework answers it the same way: while the audio is being
//! made, the CPU is told to treat subnormal inputs as zero and to write zero
//! wherever a result would have been subnormal. [`NoDenormals`] does that
//! for as long as it is held, and puts the thread's previous setting back
//! when it is dropped, so nothing outside the audio work is affected.
//!
//! What it costs the sound: every value it changes was already smaller than
//! about 1e-38 — some 760 dB below full scale, and far beneath the noise of
//! any converter that will ever play it.
//!
//! * x86-64: the MXCSR register's flush-to-zero (bit 15) and
//!   denormals-are-zero (bit 6) bits, for every SSE operation in either
//!   precision.
//! * AArch64 (Apple Silicon, the Raspberry Pi 5): FPCR.FZ (bit 24), which
//!   flushes subnormal inputs and outputs of both precisions.
//! * Anything else: nothing, and [`NoDenormals::available`] says so.
//!
//! The setting is per thread, so every thread that makes audio has to hold
//! one: the callback holds one through each block (in
//! [`crate::engine::EngineAudio::process`] and [`crate::mixer::Mixer::process`]),
//! and the worker threads that share its tracks hold one for their whole
//! lives (in [`crate::parallel`]). They must agree, because a track rendered
//! on a worker has to come out bit for bit the same as on the callback.

use std::marker::PhantomData;

/// The thread's floating-point control word, as the hardware holds it.
#[cfg(target_arch = "x86_64")]
type Mode = u32;
#[cfg(target_arch = "aarch64")]
type Mode = u64;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
type Mode = ();

/// MXCSR: flush-to-zero and denormals-are-zero.
#[cfg(target_arch = "x86_64")]
const FLUSH: Mode = (1 << 15) | (1 << 6);
/// FPCR: flush-to-zero.
#[cfg(target_arch = "aarch64")]
const FLUSH: Mode = 1 << 24;

#[cfg(target_arch = "x86_64")]
fn read() -> Mode {
    let mut mode: Mode = 0;
    // SAFETY: stores the 32-bit MXCSR to a local; every x86-64 CPU has SSE.
    unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut mode, options(nostack, preserves_flags)) };
    mode
}

#[cfg(target_arch = "x86_64")]
fn write(mode: Mode) {
    // SAFETY: loads MXCSR from a value that came from `read`, with at most
    // the two flush bits changed — both defined on every x86-64 CPU.
    unsafe { std::arch::asm!("ldmxcsr [{}]", in(reg) &mode, options(nostack, preserves_flags)) };
}

#[cfg(target_arch = "aarch64")]
fn read() -> Mode {
    let mode: Mode;
    // SAFETY: FPCR is readable at EL0 on every AArch64 CPU.
    unsafe { std::arch::asm!("mrs {}, fpcr", out(reg) mode, options(nomem, nostack, preserves_flags)) };
    mode
}

#[cfg(target_arch = "aarch64")]
fn write(mode: Mode) {
    // SAFETY: FPCR is writable at EL0, and the value came from `read` with at
    // most the FZ bit changed.
    unsafe { std::arch::asm!("msr fpcr, {}", in(reg) mode, options(nostack, preserves_flags)) };
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn read() -> Mode {}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn write(_: Mode) {}

/// Flush-to-zero on this thread for as long as it is held. See the module
/// notes.
///
/// Cheap enough to take once per block — one register read and at most one
/// write each way — and real-time safe: no allocation, no lock, no system
/// call. It is deliberately not `Send`: the mode belongs to the thread that
/// set it, and restoring it on another thread would change that thread's
/// mode instead.
#[must_use = "the mode is restored when the guard is dropped"]
pub struct NoDenormals {
    previous: Mode,
    _thread: PhantomData<*const ()>,
}

impl NoDenormals {
    /// Turn flushing on for the calling thread.
    pub fn new() -> Self {
        let previous = read();
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if previous & FLUSH != FLUSH {
            write(previous | FLUSH);
        }
        Self { previous, _thread: PhantomData }
    }

    /// Whether this platform has a flush-to-zero mode for the guard to set.
    #[must_use]
    pub const fn available() -> bool {
        cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
    }
}

impl Default for NoDenormals {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NoDenormals {
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        if read() != self.previous {
            write(self.previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    /// Half the smallest normal `f32`, computed at run time so that the
    /// compiler cannot fold it under its own rules.
    fn half_smallest_f32() -> f32 {
        black_box(f32::MIN_POSITIVE) * black_box(0.5)
    }

    fn half_smallest_f64() -> f64 {
        black_box(f64::MIN_POSITIVE) * black_box(0.5)
    }

    /// Inside the guard a result that would be subnormal is zero, in both
    /// precisions; outside it, it is the subnormal it always was.
    #[test]
    fn the_guard_flushes_and_restores() {
        assert!(half_smallest_f32().is_subnormal(), "flushing was already on before the guard");
        {
            let _guard = NoDenormals::new();
            if NoDenormals::available() {
                assert_eq!(half_smallest_f32(), 0.0);
                assert_eq!(half_smallest_f64(), 0.0);
            }
            // A normal value is untouched.
            assert_eq!(black_box(f32::MIN_POSITIVE) * black_box(2.0), f32::MIN_POSITIVE * 2.0);
        }
        assert!(half_smallest_f32().is_subnormal(), "the guard left flushing on");
        assert!(half_smallest_f64().is_subnormal());
    }

    /// Guards nest: an inner one finds flushing already on and leaves it on
    /// when it goes, and only the outermost puts the old mode back.
    #[test]
    fn guards_nest() {
        let outer = NoDenormals::new();
        {
            let _inner = NoDenormals::new();
        }
        if NoDenormals::available() {
            assert_eq!(half_smallest_f32(), 0.0, "the inner guard turned flushing off");
        }
        drop(outer);
        assert!(half_smallest_f32().is_subnormal());
    }

    /// Subnormal *inputs* read as zero too, not only results — the case of a
    /// delay line that was filled before the guard existed.
    #[test]
    fn subnormal_inputs_read_as_zero() {
        let stored = half_smallest_f32();
        assert!(stored.is_subnormal());
        let _guard = NoDenormals::new();
        if NoDenormals::available() {
            assert_eq!(black_box(stored) * black_box(4.0), 0.0);
        }
    }
}
