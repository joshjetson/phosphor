//! The whole deck: raw readings in, MIDI out.
//!
//! The firmware calls [`Deck::digital`] with the twelve bytes it clocked out
//! of the shift-register chain, [`Deck::pads`] and [`Deck::second_mux`] with
//! the sixteen readings from each multiplexer, and [`Deck::midi_in`] with
//! whatever Phosphor sent back. Every message to send goes to `out`.
//!
//! The bytes arrive as the chain hands them over: byte `k` is chip U(k+1),
//! bit 0 its input A and bit 7 its input H. An input reads 0 while its
//! switch is closed, because every input idles high through a pull-up.

use crate::debounce::Debounce;
use crate::encoder::Quadrature;
use crate::fader::Fader;
use crate::lights::Lights;
use crate::midi::{self, Packet, PERFORMANCE};
use crate::pad::{PadConfig, PadEvent, PadSensor};
use crate::table::{CHANNEL, DIGITAL, ENCODERS, FADERS, PADS};
use crate::{Input, CHIPS};

/// The controller numbers the pedals send, on the performance channel.
pub const SUSTAIN_CC: u8 = 64;
pub const EXPRESSION_CC: u8 = 11;
/// The second multiplexer's channel the expression pedal is on.
pub const EXPRESSION_CHANNEL: usize = 8;

/// Settings the board may need to change.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Scans a switch must hold a new level for: 5 ms at the scan rate.
    pub settle: u8,
    /// Count every encoder the other way, for a board wired B-for-A.
    pub flip_encoders: bool,
    pub pad: PadConfig,
}

fn closed(bytes: &[u8; CHIPS], input: usize) -> bool {
    bytes[input / 8] & (1 << (input % 8)) == 0
}

pub struct Deck {
    config: Config,
    switches: [Debounce; DIGITAL.len()],
    encoders: [Quadrature; ENCODERS.len()],
    pads: [PadSensor; PADS.len()],
    faders: [Fader; FADERS.len()],
    expression: Fader,
    /// The sustain pedal's level at power-up, which is "up" whichever kind
    /// of pedal is plugged in.
    sustain_up: bool,
    lights: Lights,
}

impl Deck {
    /// A deck whose switches start where the first scan found them, so a
    /// button held at power-up is not a press.
    pub fn new(first: &[u8; CHIPS], config: Config) -> Self {
        let switches = core::array::from_fn(|i| Debounce::new(closed(first, i)));
        let mut pins = [(false, false); ENCODERS.len()];
        for (i, input) in DIGITAL.iter().enumerate() {
            match *input {
                Input::EncoderA { encoder } => pins[encoder].0 = !closed(first, i),
                Input::EncoderB { encoder } => pins[encoder].1 = !closed(first, i),
                _ => {}
            }
        }
        let sustain = DIGITAL.iter().position(|i| *i == Input::Sustain).expect("the table has a sustain input");
        Self {
            config,
            switches,
            encoders: pins.map(|(a, b)| Quadrature::new(a, b)),
            pads: Default::default(),
            faders: Default::default(),
            expression: Fader::default(),
            sustain_up: closed(first, sustain),
            lights: Lights::default(),
        }
    }

    pub fn lights(&self) -> &Lights {
        &self.lights
    }

    /// One scan of the chain.
    pub fn digital(&mut self, bytes: &[u8; CHIPS], out: &mut impl FnMut(Packet)) {
        let mut pins = [(false, false); ENCODERS.len()];
        for (i, input) in DIGITAL.iter().enumerate() {
            let level = closed(bytes, i);
            match *input {
                Input::Button { note, light } => {
                    if let Some(down) = self.switches[i].update(level, self.config.settle) {
                        out(if down { midi::note_on(CHANNEL, note, 127) } else { midi::note_off(CHANNEL, note) });
                        if let (true, Some(light)) = (down, light) {
                            self.lights.pick(light);
                        }
                    }
                }
                Input::Push { note } => {
                    if let Some(down) = self.switches[i].update(level, self.config.settle) {
                        out(if down { midi::note_on(CHANNEL, note, 127) } else { midi::note_off(CHANNEL, note) });
                    }
                }
                Input::EncoderA { encoder } => pins[encoder].0 = !level,
                Input::EncoderB { encoder } => pins[encoder].1 = !level,
                Input::Sustain => {
                    if let Some(level) = self.switches[i].update(level, self.config.settle) {
                        let down = level != self.sustain_up;
                        out(midi::control(PERFORMANCE, SUSTAIN_CC, if down { 127 } else { 0 }));
                    }
                }
            }
        }
        for (e, (a, b)) in pins.into_iter().enumerate() {
            let mut clicks = self.encoders[e].update(a, b);
            if self.config.flip_encoders {
                clicks = -clicks;
            }
            if clicks != 0 {
                out(midi::control(CHANNEL, ENCODERS[e], midi::relative(clicks)));
            }
        }
    }

    /// One reading of every pad.
    pub fn pads(&mut self, readings: &[u16; 16], out: &mut impl FnMut(Packet)) {
        for (i, pad) in PADS.iter().enumerate() {
            match self.pads[i].update(readings[i], &self.config.pad) {
                Some(PadEvent::Hit(v)) => {
                    out(midi::note_on(CHANNEL, pad.note, v));
                    self.lights.set(pad.light, v);
                }
                Some(PadEvent::Release) => {
                    out(midi::note_off(CHANNEL, pad.note));
                    self.lights.set(pad.light, 0);
                }
                Some(PadEvent::Tap(v)) => {
                    out(midi::note_on(CHANNEL, pad.note, v));
                    out(midi::note_off(CHANNEL, pad.note));
                }
                None => {}
            }
        }
    }

    /// One reading of the second multiplexer: the faders, then the
    /// expression pedal.
    pub fn second_mux(&mut self, readings: &[u16; 16], out: &mut impl FnMut(Packet)) {
        for (i, cc) in FADERS.iter().enumerate() {
            if let Some(value) = self.faders[i].update(readings[i]) {
                out(midi::control(CHANNEL, *cc, value));
            }
        }
        if let Some(value) = self.expression.update(readings[EXPRESSION_CHANNEL]) {
            out(midi::control(PERFORMANCE, EXPRESSION_CC, value));
        }
    }

    /// A packet from Phosphor.
    pub fn midi_in(&mut self, packet: Packet) {
        if let Some(command) = midi::light_command(packet, CHANNEL) {
            self.lights.set_note(command.note, command.level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::{GROUPS, LIGHTS};

    const IDLE: [u8; CHIPS] = [0xFF; CHIPS];

    fn config() -> Config {
        Config { settle: 2, flip_encoders: false, pad: crate::pad::DEFAULT }
    }

    fn with(closed: &[usize]) -> [u8; CHIPS] {
        let mut b = IDLE;
        for &i in closed {
            b[i / 8] &= !(1 << (i % 8));
        }
        b
    }

    fn scan(deck: &mut Deck, bytes: [u8; CHIPS], times: usize) -> heapless_vec::Out {
        let mut out = heapless_vec::Out::default();
        for _ in 0..times {
            deck.digital(&bytes, &mut |p| out.push(p));
        }
        out
    }

    /// A tiny fixed buffer so the tests need no allocator.
    mod heapless_vec {
        #[derive(Default)]
        pub struct Out {
            pub items: [[u8; 4]; 32],
            pub len: usize,
        }
        impl Out {
            pub fn push(&mut self, p: [u8; 4]) {
                self.items[self.len] = p;
                self.len += 1;
            }
            pub fn all(&self) -> &[[u8; 4]] {
                &self.items[..self.len]
            }
        }
    }

    #[test]
    fn the_table_matches_the_panel() {
        assert_eq!(DIGITAL.len(), 92, "61 buttons + 10 pushes + 20 encoder pins + sustain");
        assert_eq!(ENCODERS.len(), 10);
        assert_eq!(PADS.len(), 16);
        assert_eq!(FADERS.len(), 8);
        assert_eq!(LIGHTS, 42);
        assert_eq!(CHANNEL, 15);
        // PLAY is input 0 and sends note 0, as the schematics say.
        assert_eq!(DIGITAL[0], Input::Button { note: 0, light: Some(0) });
        assert_eq!(DIGITAL[91], Input::Sustain);
        assert_eq!(GROUPS.len(), 3, "function targets, track modes, step halves");
    }

    #[test]
    fn a_button_is_a_note_on_then_a_note_off_on_channel_16() {
        let mut deck = Deck::new(&IDLE, config());
        let pressed = scan(&mut deck, with(&[0]), 3);
        assert_eq!(pressed.all(), &[[0x09, 0x9F, 0, 127]]);
        let released = scan(&mut deck, IDLE, 3);
        assert_eq!(released.all(), &[[0x08, 0x8F, 0, 0]]);
    }

    #[test]
    fn a_button_held_at_power_up_is_not_a_press() {
        let mut deck = Deck::new(&with(&[0]), config());
        assert!(scan(&mut deck, with(&[0]), 5).all().is_empty());
    }

    #[test]
    fn an_encoder_click_is_a_relative_step_on_its_controller() {
        let a = DIGITAL.iter().position(|i| *i == Input::EncoderA { encoder: 0 }).unwrap();
        let b = a + 1;
        let mut deck = Deck::new(&IDLE, config());
        // Pins idle high (11). One click: 01, 00, 10, 11 — closed means low.
        let mut out = heapless_vec::Out::default();
        for bytes in [with(&[a]), with(&[a, b]), with(&[b]), IDLE] {
            deck.digital(&bytes, &mut |p| out.push(p));
        }
        assert_eq!(out.all(), &[[0x0B, 0xBF, ENCODERS[0], 1]]);
        let mut flipped = Deck::new(&IDLE, Config { flip_encoders: true, ..config() });
        let mut out = heapless_vec::Out::default();
        for bytes in [with(&[a]), with(&[a, b]), with(&[b]), IDLE] {
            flipped.digital(&bytes, &mut |p| out.push(p));
        }
        assert_eq!(out.all(), &[[0x0B, 0xBF, ENCODERS[0], 127]]);
    }

    #[test]
    fn the_sustain_pedal_works_either_polarity() {
        // A normally-closed pedal: the input reads closed at rest.
        let mut deck = Deck::new(&with(&[91]), config());
        let down = scan(&mut deck, IDLE, 3);
        assert_eq!(down.all(), &[[0x0B, 0xB0, SUSTAIN_CC, 127]], "opening it is the press");
        let up = scan(&mut deck, with(&[91]), 3);
        assert_eq!(up.all(), &[[0x0B, 0xB0, SUSTAIN_CC, 0]]);
    }

    #[test]
    fn a_pad_hit_plays_and_lights_its_pad() {
        let mut deck = Deck::new(&IDLE, config());
        let mut out = heapless_vec::Out::default();
        let mut readings = [0u16; 16];
        for r in [600, 2000, 2400, 2300, 2100, 1900, 1800] {
            readings[8] = r;
            deck.pads(&readings, &mut |p| out.push(p));
        }
        assert_eq!(out.len, 1);
        let [_, status, note, velocity] = out.items[0];
        assert_eq!((status, note), (0x9F, PADS[8].note));
        assert!(velocity > 90);
        assert_eq!(deck.lights().level(PADS[8].light), velocity);
    }

    #[test]
    fn faders_send_their_controller_and_the_pedal_sends_expression() {
        let mut deck = Deck::new(&IDLE, config());
        let mut out = heapless_vec::Out::default();
        let mut readings = [0u16; 16];
        readings[3] = 4095;
        readings[EXPRESSION_CHANNEL] = 2048;
        deck.second_mux(&readings, &mut |p| out.push(p));
        assert!(out.all().contains(&[0x0B, 0xBF, FADERS[3], 127]));
        assert!(out.all().contains(&[0x0B, 0xB0, EXPRESSION_CC, 64]));
    }

    #[test]
    fn phosphor_lights_a_control_by_its_note() {
        let mut deck = Deck::new(&IDLE, config());
        deck.midi_in(midi::note_on(CHANNEL, 2, 100)); // REC
        assert_eq!(deck.lights().level(1), 100, "REC is the second light");
        deck.midi_in(midi::note_off(CHANNEL, 2));
        assert_eq!(deck.lights().level(1), 0);
    }
}
