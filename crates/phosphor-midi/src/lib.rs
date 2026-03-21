//! MIDI I/O, controller detection, and message routing.
//!
//! Uses midir for cross-platform MIDI port access and wmidi for
//! zero-allocation message parsing in the audio thread.

pub mod message;
pub mod ports;
pub mod ring;

pub use message::{MidiMessage, MidiMessageType};
pub use ring::MidiRingBuffer;
