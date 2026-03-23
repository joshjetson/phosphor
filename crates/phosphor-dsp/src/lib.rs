//! Built-in DSP: oscillators, filters, effects.
//!
//! Every DSP component implements the `Plugin` trait from phosphor-plugin.
//! These are the same plugins users can build — no special treatment.

pub mod drum_rack;
pub mod dx7;
pub mod jupiter;
pub mod odyssey;
pub mod oscillator;
pub mod synth;
