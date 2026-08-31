//! Mixer effects.
//!
//! These are the processors that sit in a track, bus or master insert chain,
//! as distinct from the instruments in the crate root. They share three
//! properties that the instruments do not need:
//!
//! * **Zero latency.** Nothing here reports a delay, so the mixer never has
//!   to compensate for one. That rules out linear-phase filtering and
//!   lookahead, and both were given up deliberately.
//! * **Bit-exact bypass.** An effect that is bypassed, or whose wet/dry sits
//!   at zero, must return the input untouched — not "run with neutral
//!   settings", which is only neutral to within a rounding error. The dry
//!   path inside every effect therefore touches no filter and no gain stage.
//! * **No allocation, no locks, no logging in `process`.** Every buffer an
//!   effect needs is sized when it is constructed or when the sample rate
//!   changes.
//!
//! Each effect is a plain struct with inherent methods. The chain that runs
//! them lives in the mixer.

pub mod compressor;
pub mod delay;
pub mod eq;
pub mod reverb;
pub mod tape;
