//! What the Phosphor Deck's controls mean, without the hardware.
//!
//! Everything a judgement goes into lives here, as plain Rust that tests on
//! any computer: when a bouncing contact counts as a press ([`debounce`]),
//! how an encoder's two pins become clicks ([`encoder`]), how hard a pad was
//! hit ([`pad`]), when a fader has really moved ([`fader`]), what the LEDs
//! show ([`lights`]) and the MIDI bytes that go out ([`midi`]). [`deck`] ties
//! them together: the firmware hands it raw readings and gets messages back.
//!
//! The table of which input is which control is generated at build time
//! from `firmware/deck-layout.json`, Phosphor's own control table, so the
//! firmware and the app cannot disagree about a number.
#![no_std]

pub mod debounce;
pub mod deck;
pub mod encoder;
pub mod fader;
pub mod lights;
pub mod midi;
pub mod pad;

/// One shift-register input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A button: a note while held, and maybe a light.
    Button { note: u8, light: Option<u8> },
    /// An encoder's push switch.
    Push { note: u8 },
    /// An encoder's two rotation pins.
    EncoderA { encoder: usize },
    EncoderB { encoder: usize },
    /// The sustain pedal jack.
    Sustain,
}

/// One pad: the note it sends and the LED under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pad {
    pub note: u8,
    pub light: u8,
}

/// One LED's colour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// The generated table: see `build.rs`.
pub mod table {
    use super::{Input, Pad, Rgb};
    include!(concat!(env!("OUT_DIR"), "/table.rs"));
}

/// Bytes read from the shift-register chain per scan, one per chip.
pub const CHIPS: usize = 12;
const _: () = assert!(table::DIGITAL.len() <= CHIPS * 8, "more inputs than the chain has");
