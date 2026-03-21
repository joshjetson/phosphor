//! Built-in DSP: oscillators, filters, effects.
//!
//! Every DSP component implements the `Plugin` trait from phosphor-plugin.
//! These are the same plugins users can build — no special treatment.

pub mod oscillator;
pub mod synth;
