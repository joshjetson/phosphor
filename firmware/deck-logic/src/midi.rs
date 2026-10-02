//! The bytes that go out over USB, and the ones that come back.
//!
//! USB MIDI sends each message as a four-byte event packet: a header naming
//! the cable and the kind of message, then the ordinary MIDI bytes.

/// One USB MIDI event packet.
pub type Packet = [u8; 4];

/// The channel performances go out on: the pedals, counted from zero.
pub const PERFORMANCE: u8 = 0;

pub fn note_on(channel: u8, note: u8, velocity: u8) -> Packet {
    [0x09, 0x90 | channel, note & 0x7F, velocity.clamp(1, 127)]
}

pub fn note_off(channel: u8, note: u8) -> Packet {
    [0x08, 0x80 | channel, note & 0x7F, 0]
}

pub fn control(channel: u8, controller: u8, value: u8) -> Packet {
    [0x0B, 0xB0 | channel, controller & 0x7F, value & 0x7F]
}

/// An encoder's turn: clicks as a 7-bit two's-complement step, 1 for one
/// click clockwise and 127 for one counter-clockwise.
pub fn relative(clicks: i8) -> u8 {
    (clicks.clamp(-63, 63) as u8) & 0x7F
}

/// A note for the deck, from Phosphor: which control, and how bright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightCommand {
    pub note: u8,
    /// 0 is off.
    pub level: u8,
}

/// Read a packet from Phosphor. Only notes on the deck's own channel mean
/// anything to the deck.
pub fn light_command(packet: Packet, channel: u8) -> Option<LightCommand> {
    let [_, status, note, velocity] = packet;
    if status & 0x0F != channel {
        return None;
    }
    match status & 0xF0 {
        0x90 => Some(LightCommand { note, level: velocity }),
        0x80 => Some(LightCommand { note, level: 0 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_steps_are_seven_bit_twos_complement() {
        assert_eq!(relative(1), 1);
        assert_eq!(relative(-1), 127);
        assert_eq!(relative(3), 3);
        assert_eq!(relative(-3), 125);
    }

    #[test]
    fn a_note_on_never_carries_velocity_zero() {
        // Velocity 0 is a note off in MIDI's running-status shorthand.
        assert_eq!(note_on(15, 0, 0), [0x09, 0x9F, 0, 1]);
    }

    #[test]
    fn only_the_decks_channel_moves_its_lights() {
        assert_eq!(light_command([0x09, 0x9F, 4, 100], 15), Some(LightCommand { note: 4, level: 100 }));
        assert_eq!(light_command([0x08, 0x8F, 4, 0], 15), Some(LightCommand { note: 4, level: 0 }));
        assert_eq!(light_command([0x09, 0x90, 4, 100], 15), None);
    }
}
