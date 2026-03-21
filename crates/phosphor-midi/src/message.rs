//! MIDI message types — lightweight, copy-friendly, real-time safe.

/// A timestamped MIDI message. Small enough to pass through a ring buffer.
#[derive(Debug, Clone, Copy)]
pub struct MidiMessage {
    /// When this message was received (high-resolution).
    pub timestamp: Option<u64>, // nanoseconds from midir
    /// The parsed message type.
    pub message_type: MidiMessageType,
    /// Raw bytes for forwarding (up to 3 bytes for channel messages).
    pub raw: [u8; 3],
    /// Number of valid bytes in `raw`.
    pub len: u8,
}

/// Parsed MIDI message types we care about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiMessageType {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    PitchBend {
        channel: u8,
        /// 14-bit value: 0..16383, center = 8192
        value: u16,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    ChannelPressure {
        channel: u8,
        pressure: u8,
    },
    /// Anything we don't specifically parse.
    Other,
}

impl MidiMessage {
    /// Parse a raw MIDI byte slice into a MidiMessage.
    /// Returns None only if the slice is empty.
    pub fn from_bytes(bytes: &[u8], timestamp: u64) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }

        let mut raw = [0u8; 3];
        let len = bytes.len().min(3);
        raw[..len].copy_from_slice(&bytes[..len]);

        let message_type = Self::parse_type(bytes);

        Some(Self {
            timestamp: Some(timestamp),
            message_type,
            raw,
            len: len as u8,
        })
    }

    fn parse_type(bytes: &[u8]) -> MidiMessageType {
        if bytes.is_empty() {
            return MidiMessageType::Other;
        }

        let status = bytes[0];
        let kind = status & 0xF0;
        let channel = status & 0x0F;

        match kind {
            0x90 if bytes.len() >= 3 => {
                if bytes[2] == 0 {
                    // Note-on with velocity 0 = note-off
                    MidiMessageType::NoteOff {
                        channel,
                        note: bytes[1],
                        velocity: 0,
                    }
                } else {
                    MidiMessageType::NoteOn {
                        channel,
                        note: bytes[1],
                        velocity: bytes[2],
                    }
                }
            }
            0x80 if bytes.len() >= 3 => MidiMessageType::NoteOff {
                channel,
                note: bytes[1],
                velocity: bytes[2],
            },
            0xB0 if bytes.len() >= 3 => MidiMessageType::ControlChange {
                channel,
                controller: bytes[1],
                value: bytes[2],
            },
            0xE0 if bytes.len() >= 3 => {
                let value = (bytes[2] as u16) << 7 | (bytes[1] as u16);
                MidiMessageType::PitchBend { channel, value }
            }
            0xC0 if bytes.len() >= 2 => MidiMessageType::ProgramChange {
                channel,
                program: bytes[1],
            },
            0xD0 if bytes.len() >= 2 => MidiMessageType::ChannelPressure {
                channel,
                pressure: bytes[1],
            },
            _ => MidiMessageType::Other,
        }
    }

    /// MIDI note number to frequency (A4 = 440 Hz).
    pub fn note_to_freq(note: u8) -> f64 {
        440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
    }

    /// MIDI note number to name (e.g., 60 → "C4").
    pub fn note_to_name(note: u8) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let octave = (note as i8 / 12) - 1;
        let name = NAMES[note as usize % 12];
        format!("{name}{octave}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_note_on() {
        let msg = MidiMessage::from_bytes(&[0x90, 60, 100], 0).unwrap();
        assert_eq!(
            msg.message_type,
            MidiMessageType::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            }
        );
    }

    #[test]
    fn parse_note_on_velocity_zero_is_note_off() {
        let msg = MidiMessage::from_bytes(&[0x90, 60, 0], 0).unwrap();
        assert_eq!(
            msg.message_type,
            MidiMessageType::NoteOff {
                channel: 0,
                note: 60,
                velocity: 0,
            }
        );
    }

    #[test]
    fn parse_note_off() {
        let msg = MidiMessage::from_bytes(&[0x80, 60, 64], 0).unwrap();
        assert_eq!(
            msg.message_type,
            MidiMessageType::NoteOff {
                channel: 0,
                note: 60,
                velocity: 64,
            }
        );
    }

    #[test]
    fn parse_control_change() {
        let msg = MidiMessage::from_bytes(&[0xB3, 7, 127], 0).unwrap();
        assert_eq!(
            msg.message_type,
            MidiMessageType::ControlChange {
                channel: 3,
                controller: 7,
                value: 127,
            }
        );
    }

    #[test]
    fn parse_pitch_bend() {
        // Center position: LSB=0, MSB=64 → value = 8192
        let msg = MidiMessage::from_bytes(&[0xE0, 0, 64], 0).unwrap();
        assert_eq!(
            msg.message_type,
            MidiMessageType::PitchBend {
                channel: 0,
                value: 8192,
            }
        );
    }

    #[test]
    fn parse_pitch_bend_extremes() {
        // Minimum
        let msg = MidiMessage::from_bytes(&[0xE0, 0, 0], 0).unwrap();
        if let MidiMessageType::PitchBend { value, .. } = msg.message_type {
            assert_eq!(value, 0);
        }
        // Maximum
        let msg = MidiMessage::from_bytes(&[0xE0, 127, 127], 0).unwrap();
        if let MidiMessageType::PitchBend { value, .. } = msg.message_type {
            assert_eq!(value, 16383);
        }
    }

    #[test]
    fn parse_program_change() {
        let msg = MidiMessage::from_bytes(&[0xC5, 42], 0).unwrap();
        assert_eq!(
            msg.message_type,
            MidiMessageType::ProgramChange {
                channel: 5,
                program: 42,
            }
        );
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(MidiMessage::from_bytes(&[], 0).is_none());
    }

    #[test]
    fn parse_truncated_note_on_is_other() {
        let msg = MidiMessage::from_bytes(&[0x90], 0).unwrap();
        assert_eq!(msg.message_type, MidiMessageType::Other);
    }

    #[test]
    fn parse_unknown_status_is_other() {
        let msg = MidiMessage::from_bytes(&[0xF0, 0x7E], 0).unwrap();
        assert_eq!(msg.message_type, MidiMessageType::Other);
    }

    #[test]
    fn note_to_freq_a4() {
        let freq = MidiMessage::note_to_freq(69);
        assert!((freq - 440.0).abs() < 0.01);
    }

    #[test]
    fn note_to_freq_middle_c() {
        let freq = MidiMessage::note_to_freq(60);
        assert!((freq - 261.63).abs() < 0.1);
    }

    #[test]
    fn note_to_name_middle_c() {
        assert_eq!(MidiMessage::note_to_name(60), "C4");
    }

    #[test]
    fn note_to_name_a4() {
        assert_eq!(MidiMessage::note_to_name(69), "A4");
    }

    #[test]
    fn all_channels_parsed_correctly() {
        for ch in 0..16u8 {
            let msg = MidiMessage::from_bytes(&[0x90 | ch, 60, 100], 0).unwrap();
            if let MidiMessageType::NoteOn { channel, .. } = msg.message_type {
                assert_eq!(channel, ch);
            } else {
                panic!("Expected NoteOn for channel {ch}");
            }
        }
    }

    #[test]
    fn raw_bytes_preserved() {
        let msg = MidiMessage::from_bytes(&[0x90, 60, 100], 12345).unwrap();
        assert_eq!(msg.raw[0], 0x90);
        assert_eq!(msg.raw[1], 60);
        assert_eq!(msg.raw[2], 100);
        assert_eq!(msg.len, 3);
        assert_eq!(msg.timestamp, Some(12345));
    }
}
