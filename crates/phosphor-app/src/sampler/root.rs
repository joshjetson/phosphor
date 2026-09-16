//! Reading a root out of a file name.
//!
//! A sample library names its pitch in the file: `Piano_C3.wav`,
//! `kick_A#1.wav`, `Strings Eb2.wav`. A player who has just loaded one into
//! a keytracking zone should not have to dial the same number in by hand,
//! so the stem is read on the way in and the flash says what was learned.
//!
//! It is the *third* way a root arrives, and the least certain of them: a
//! capture knows the pitch because it recorded the performance, learn knows
//! it because the player played it, and this one is a guess about a string.
//! So it is deliberately narrow — a name it cannot read changes nothing at
//! all, which is the only behaviour that is safe when the answer retunes
//! every key in a zone.
//!
//! # What counts as a note name
//!
//! A letter `A`–`G`, an optional `#` or `b`, and an octave number that may
//! be negative — at the *end* of the stem, and either alone or after a
//! delimiter. `Piano_C3` reads; `C3loop` does not, because the name is
//! `C3loop` and not a note; `07_Kick` does not, because there is no note in
//! it; `kit.samples/C3-1` does not, because that is take 1 of pad C3 and
//! reading `-1` as an octave would retune a zone four octaves down.
//!
//! Octaves follow the house convention, which is
//! [`crate::format::note_name`]'s: C3 is MIDI 60.

/// The house's octave offset: MIDI 60 is C3, so the octave number is two
/// below the one a straight division would give.
const OCTAVE_BASE: i32 = 2;

/// Semitone of each letter above C.
fn letter_semitone(letter: char) -> Option<i32> {
    match letter.to_ascii_uppercase() {
        'C' => Some(0),
        'D' => Some(2),
        'E' => Some(4),
        'F' => Some(5),
        'G' => Some(7),
        'A' => Some(9),
        'B' => Some(11),
        _ => None,
    }
}

/// Whether a note name may start here: at the front of the stem, or after
/// something that separates words.
fn delimited(before: Option<char>) -> bool {
    match before {
        None => true,
        Some(c) => matches!(c, '_' | '-' | '.' | ' '),
    }
}

/// The MIDI note a file stem names, if it names one.
///
/// Read right to left, because the note is at the end and everything before
/// it is the sample's name: the octave digits, an optional minus, an
/// optional accidental, the letter, and then the proof that the letter
/// starts a word rather than ending one.
#[must_use]
pub fn from_name(stem: &str) -> Option<u8> {
    let chars: Vec<char> = stem.chars().collect();
    let mut at = chars.len();

    // The octave digits.
    let end = at;
    while at > 0 && chars[at - 1].is_ascii_digit() {
        at -= 1;
    }
    if at == end {
        return None;
    }
    let digits: String = chars[at..end].iter().collect();
    let mut octave: i32 = digits.parse().ok()?;

    // A minus in front of them makes the octave negative — the only way
    // `A-1`, the bottom key of the bed, can be spelled.
    if at > 0 && chars[at - 1] == '-' {
        at -= 1;
        octave = -octave;
    }

    // The accidental, then the letter. Tried with the accidental first so
    // that `Eb2` reads as E flat rather than as B with an E stuck to it;
    // `b2` on its own still reads as B, because there is no letter behind
    // the `b` to be flattened.
    for accidental in [true, false] {
        let mut seek = at;
        let mut offset = 0i32;
        if accidental {
            match seek.checked_sub(1).and_then(|i| chars.get(i)) {
                Some('#' | '\u{266F}') => offset = 1,
                Some('b' | '\u{266D}') => offset = -1,
                _ => continue,
            }
            seek -= 1;
        }
        let Some(&letter) = seek.checked_sub(1).and_then(|i| chars.get(i)) else { continue };
        let Some(semitone) = letter_semitone(letter) else { continue };
        if !delimited(seek.checked_sub(2).and_then(|i| chars.get(i)).copied()) {
            continue;
        }
        let note = (octave + OCTAVE_BASE) * 12 + semitone + offset;
        if (0..=127).contains(&note) {
            return Some(note as u8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::note_name;

    /// The expected numbers come from the house's own spelling rather than
    /// from arithmetic done here: whatever [`note_name`] calls a note is
    /// what a file named that has to read back as.
    fn round_trip(note: u8) {
        let name = note_name(note);
        assert_eq!(from_name(&name), Some(note), "{name} did not read back");
        assert_eq!(from_name(&format!("Piano_{name}")), Some(note), "Piano_{name}");
        assert_eq!(from_name(&format!("07 {name}")), Some(note), "07 {name}");
    }

    #[test]
    fn every_key_on_the_bed_reads_back_as_itself() {
        for note in 21..=108u8 {
            round_trip(note);
        }
    }

    /// The names a library actually ships, including the flat spelling the
    /// house never writes but every string library does.
    #[test]
    fn the_names_a_library_ships_are_read() {
        assert_eq!(from_name("Piano_C3"), Some(60));
        assert_eq!(from_name("kick_A#1"), Some(46));
        assert_eq!(from_name("Strings Eb2"), Some(51));
        assert_eq!(from_name("C3"), Some(60));
        assert_eq!(from_name("bass-c3"), Some(60), "a lowercase letter is still a note");
        assert_eq!(from_name("Cello.A-1"), Some(21), "the bottom key needs its minus");
    }

    /// The names that must change nothing. A wrong root retunes every key
    /// in a zone, so a guess is worse than no answer.
    #[test]
    fn a_name_that_is_not_a_note_changes_nothing() {
        for stem in [
            "07_Kick",     // no note in it at all
            "C3loop",      // the note is not the end of the name
            "TR808",       // digits, no letter
            "Loop_120bpm", // ends in a word
            "C3-1",        // pad C3's first take, not C minus one
            "Cs3-1",       // the same with the sharp spelled out
            "Sub2",        // a `b` that is part of a word
            "kick2",       // a letter that is not a note
            "",
            "-",
            "3",
            "H4",   // not a note letter
            "C12",  // an octave off the end of MIDI
            "C-12", // and off the other end
        ] {
            assert_eq!(from_name(stem), None, "{stem:?} was read as a note");
        }
    }

    /// The flat and the sharp reach the same key, and both end up inside
    /// the MIDI range.
    #[test]
    fn a_flat_and_a_sharp_meet_in_the_middle() {
        assert_eq!(from_name("pad_Db3"), from_name("pad_C#3"));
        assert_eq!(from_name("pad_Db3"), Some(61));
        // The bottom and the top of the range, right on the edge.
        assert_eq!(from_name("C-2"), Some(0));
        assert_eq!(from_name("G8"), Some(127));
        assert_eq!(from_name("G#8"), None, "a note past 127 was invented");
    }
}
