//! The names of notes, for the whole application.
//!
//! One convention, one table: sharps, and middle C — MIDI 60 — is **C4**,
//! as music teaching, most hardware and most DAWs spell it. Every screen
//! that names a key goes through here, so the pad map, the piano roll, the
//! chord device and the sequencer can never call the same key two things.

/// Pitch-class names, 0 = C, sharp-spelled.
pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The octave a straight division by twelve is off by: MIDI 0 is C-1, so
/// the octave number is one below `note / 12`.
pub const OCTAVE_OFFSET: i32 = 1;

/// The name of a pitch class, 0 = C.
#[must_use]
pub fn pitch_class_name(pitch_class: u8) -> &'static str {
    NOTE_NAMES[usize::from(pitch_class % 12)]
}

/// The octave number a note is written with: 4 for middle C.
#[must_use]
pub fn octave_of(note: u8) -> i32 {
    i32::from(note) / 12 - OCTAVE_OFFSET
}

/// A MIDI note's name with its octave: 60 is `C4`, 21 is `A0`, 0 is `C-1`.
#[must_use]
pub fn note_name(note: u8) -> String {
    format!("{}{}", pitch_class_name(note), octave_of(note))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_c_is_c4_and_the_piano_runs_a0_to_c8() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(21), "A0");
        assert_eq!(note_name(108), "C8");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(127), "G9");
    }

    #[test]
    fn a_pitch_class_wraps() {
        assert_eq!(pitch_class_name(0), "C");
        assert_eq!(pitch_class_name(13), "C#");
        assert_eq!(pitch_class_name(11), "B");
    }
}
