//! Per-kit drum synthesis modules.
//!
//! Each module extends `DrumVoice` with synthesis methods for one kit.

pub mod kit_808;
pub mod kit_909;
pub mod kit_707;
pub mod kit_606;
pub mod kit_777;
pub mod kit_tsty1;
pub mod kit_tsty2;
pub mod kit_tsty3;
pub mod kit_tsty4;
pub mod kit_tsty5;
pub mod kit_linn;
pub mod kit_dmx;
pub mod kit_sdsv;
pub mod kit_727;
pub mod kit_cr78;

// The three acoustic kits share one engine, because they are three sets of
// drums and not three synthesis methods.
pub mod acoustic;
pub mod acoustic_voice;
pub mod kit_jazz;
pub mod kit_funk;
pub mod kit_studio;
