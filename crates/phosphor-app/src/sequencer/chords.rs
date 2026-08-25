//! Naming what a step plays, and working out what a player just played.
//!
//! The arithmetic lives in [`phosphor_core::pattern`] — chord intervals, mode
//! scales, voicings — because the audio thread has to expand a chord in the
//! callback and there can only be one table of that. What lives here is
//! everything the audio thread has no business knowing: the names, the degree
//! numerals, the readout line, and [`identify`], which is the table run
//! backwards so that step-record can store "first-inversion minor seventh"
//! rather than four loose notes.
//!
//! # The readout
//!
//! A step's chord is shown spelled out — `Cm7/D# · D#3 G3 A#3 C4` — because
//! the alternative is a player learning what `min7` + `1st inv` sounds like
//! by trying it, once per pattern, forever. It is one line and it removes the
//! whole class of "what does this control do".
//!
//! Sharps rather than flats, which is not the way `Cm7/E♭` is conventionally
//! spelled, and is the way every other note name in this application is
//! spelled — `midi_note_name` in the bottom bar has been ASCII sharps since
//! the first version. One convention that is slightly wrong is easier to read
//! than two that are each right somewhere.

use phosphor_core::pattern::{chord_notes, Chord, Mode, Voicing, MAX_CHORD_NOTES};

/// Pitch-class names, sharp-spelled. Middle C is note 60 and is called C4,
/// which is what the rest of the application already says.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The name of a pitch class, 0 = C.
#[must_use]
pub fn note_name(pitch_class: u8) -> &'static str {
    NAMES[(pitch_class % 12) as usize]
}

/// The name of a MIDI note, with its octave: 60 is `C4`.
#[must_use]
pub fn note_label(note: u8) -> String {
    format!("{}{}", note_name(note % 12), i16::from(note) / 12 - 1)
}

/// What a chord type is called on screen.
#[must_use]
pub fn chord_name(chord: Chord) -> &'static str {
    match chord {
        Chord::None => "single",
        Chord::Fifth => "5",
        Chord::Octave => "oct",
        Chord::Diatonic => "diatonic",
        Chord::Diatonic7 => "diatonic 7",
        Chord::Maj => "maj",
        Chord::Min => "min",
        Chord::Dim => "dim",
        Chord::Sus2 => "sus2",
        Chord::Sus4 => "sus4",
        Chord::Maj6 => "maj6",
        Chord::Min6 => "min6",
        Chord::Dom7 => "7",
        Chord::Min7 => "min7",
        Chord::Maj7 => "maj7",
        Chord::Quartal => "quartal",
    }
}

/// The suffix a chord takes after its root, the way a chart writes it.
///
/// The diatonic entries have no fixed suffix — the whole point of them is
/// that the quality comes from the degree — so [`chord_symbol`] resolves them
/// first and this is never asked about one.
fn chord_suffix(chord: Chord) -> &'static str {
    match chord {
        Chord::None => "",
        Chord::Fifth => "5",
        Chord::Octave => " oct",
        Chord::Maj | Chord::Diatonic | Chord::Diatonic7 => "",
        Chord::Min => "m",
        Chord::Dim => "dim",
        Chord::Sus2 => "sus2",
        Chord::Sus4 => "sus4",
        Chord::Maj6 => "6",
        Chord::Min6 => "m6",
        Chord::Dom7 => "7",
        Chord::Min7 => "m7",
        Chord::Maj7 => "maj7",
        Chord::Quartal => "q4",
    }
}

/// Roman numerals for the degrees of a mode, cased by the triad's quality —
/// which is what a numeral is *for*: `ii` and `II` are different chords.
#[must_use]
pub fn degree_label(mode: Mode, tonic: u8, note: u8) -> Option<&'static str> {
    const UPPER: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
    const LOWER: [&str; 7] = ["i", "ii", "iii", "iv", "v", "vi", "vii"];
    const DIMINISHED: [&str; 7] = ["i°", "ii°", "iii°", "iv°", "v°", "vi°", "vii°"];

    let degree = mode.degree_of(note, tonic)?;
    let mut notes = [0u8; MAX_CHORD_NOTES];
    let count = chord_notes(
        note,
        Chord::Diatonic,
        Voicing::Close,
        false,
        mode,
        tonic,
        &mut notes,
    );
    // Major or minor third, diminished or perfect fifth: enough to case the
    // numeral, and read off the notes rather than from a second table that
    // could disagree with the first.
    let third = if count > 1 { notes[1] - notes[0] } else { 4 };
    let fifth = if count > 2 { notes[2] - notes[0] } else { 7 };
    Some(match (third, fifth) {
        (3, 6) => DIMINISHED[degree],
        (3, _) => LOWER[degree],
        _ => UPPER[degree],
    })
}

/// The chord symbol for a step: root, quality, and the bass note when the
/// voicing has put something else underneath it.
///
/// `Cm7/D#` — a first-inversion C minor seventh, which is a different thing
/// on the page from `Cm7` and should be a different thing on the screen.
#[must_use]
pub fn chord_symbol(
    root: u8,
    chord: Chord,
    voicing: Voicing,
    root_below: bool,
    mode: Mode,
    tonic: u8,
) -> String {
    let mut notes = [0u8; MAX_CHORD_NOTES];
    let count = chord_notes(root, chord, voicing, root_below, mode, tonic, &mut notes);
    if count == 0 {
        return note_label(root);
    }

    // A diatonic chord is named by what it turned out to be, not by the fact
    // that it was derived: a player reading "diatonic" learns nothing.
    let named = match chord {
        Chord::Diatonic | Chord::Diatonic7 => resolve_diatonic(root, chord, mode, tonic),
        other => other,
    };

    let mut symbol = format!("{}{}", note_name(root % 12), chord_suffix(named));
    if named == Chord::Diatonic7 {
        // The one diatonic quality with no entry in the table: half
        // diminished, on the leading tone.
        symbol = format!("{}m7b5", note_name(root % 12));
    }
    let bass = notes[0];
    if bass % 12 != root % 12 {
        symbol.push('/');
        symbol.push_str(note_name(bass % 12));
    }
    symbol
}

/// Which quality a diatonic chord came out as, by comparing what it produced
/// against the explicit entries.
fn resolve_diatonic(root: u8, chord: Chord, mode: Mode, tonic: u8) -> Chord {
    let mut derived = [0u8; MAX_CHORD_NOTES];
    let count = chord_notes(root, chord, Voicing::Close, false, mode, tonic, &mut derived);
    for candidate in Chord::ALL {
        if matches!(candidate, Chord::Diatonic | Chord::Diatonic7) {
            continue;
        }
        let mut explicit = [0u8; MAX_CHORD_NOTES];
        let n = chord_notes(root, candidate, Voicing::Close, false, mode, tonic, &mut explicit);
        if n == count && explicit[..n] == derived[..count] {
            return candidate;
        }
    }
    // Half-diminished has no entry, and is the only quality that reaches
    // here. `chord_symbol` spells it.
    chord
}

/// The whole line: what the chord is called, and the notes it plays.
///
/// `Cm7/D# · D#3 G3 A#3 C4`.
#[must_use]
pub fn readout(
    root: u8,
    chord: Chord,
    voicing: Voicing,
    root_below: bool,
    mode: Mode,
    tonic: u8,
) -> String {
    let mut notes = [0u8; MAX_CHORD_NOTES];
    let count = chord_notes(root, chord, voicing, root_below, mode, tonic, &mut notes);
    let spelled: Vec<String> = notes[..count].iter().copied().map(note_label).collect();
    format!(
        "{} · {}",
        chord_symbol(root, chord, voicing, root_below, mode, tonic),
        spelled.join(" ")
    )
}

// ── Identification ──

/// A chord the player held, as a step stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Named {
    pub root: u8,
    pub chord: Chord,
    pub voicing: Voicing,
    pub root_below: bool,
}

/// Work out what was played.
///
/// The chord table run backwards: every root, quality and voicing that could
/// have produced exactly these notes is tried, and the first match wins. The
/// search order is what makes the answer the *obvious* one — close voicing
/// before rearranged, no bass double before one, simplest quality first — so
/// three notes a third apart come back as a plain triad rather than as some
/// inversion of something else that happens to contain them.
///
/// `None` when nothing in the table produces the set, which is a chord this
/// sequencer cannot store: step-record writes the lowest note instead.
///
/// Brute force, and it costs nothing worth measuring: the roots worth trying
/// are the two octaves around what was played, so it is a few thousand
/// interval additions on a keypress.
#[must_use]
pub fn identify(held: &[u8], mode: Mode, tonic: u8) -> Option<Named> {
    if held.is_empty() || held.len() > MAX_CHORD_NOTES {
        return None;
    }
    let mut wanted: Vec<u8> = held.to_vec();
    wanted.sort_unstable();
    wanted.dedup();

    let low = wanted[0].saturating_sub(24);
    let high = wanted[wanted.len() - 1].saturating_add(24);

    let mut produced = [0u8; MAX_CHORD_NOTES];
    for root_below in [false, true] {
        for voicing in Voicing::ALL {
            for chord in Chord::ALL {
                // The diatonic entries are a way of *choosing* a quality, not
                // a quality: storing one would make the step change when the
                // mode did, which is not what a player who held four keys
                // asked for.
                if matches!(chord, Chord::Diatonic | Chord::Diatonic7) {
                    continue;
                }
                for root in low..=high {
                    let count =
                        chord_notes(root, chord, voicing, root_below, mode, tonic, &mut produced);
                    if produced[..count] == wanted[..] {
                        return Some(Named { root, chord, voicing, root_below });
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes_of(root: u8, chord: Chord, voicing: Voicing, below: bool, mode: Mode) -> Vec<u8> {
        let mut out = [0u8; MAX_CHORD_NOTES];
        let n = chord_notes(root, chord, voicing, below, mode, 0, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn note_names_match_the_rest_of_the_application() {
        assert_eq!(note_label(60), "C4");
        assert_eq!(note_label(61), "C#4");
        assert_eq!(note_label(0), "C-1");
        assert_eq!(note_label(127), "G9");
    }

    /// Every chord type has a name, and no two share one — a list with a
    /// duplicate in it is a list a player cannot navigate.
    #[test]
    fn every_chord_has_its_own_name() {
        let mut seen = Vec::new();
        for chord in Chord::ALL {
            let name = chord_name(chord);
            assert!(!name.is_empty(), "{chord:?} has no name");
            assert!(!seen.contains(&name), "two chords are both called {name}");
            seen.push(name);
        }
        for voicing in Voicing::ALL {
            assert!(!voicing.label().is_empty());
        }
        for mode in Mode::ALL {
            assert!(!mode.label().is_empty());
        }
    }

    /// The readout is the whole feature: the chord as a chart would write it,
    /// and the notes it is actually going to play.
    #[test]
    fn the_readout_names_the_chord_and_spells_it_out() {
        assert_eq!(readout(60, Chord::Min7, Voicing::Close, false, Mode::Chromatic, 0),
                   "Cm7 · C4 D#4 G4 A#4");
        assert_eq!(readout(60, Chord::Maj, Voicing::Close, false, Mode::Chromatic, 0),
                   "C · C4 E4 G4");
        assert_eq!(readout(60, Chord::None, Voicing::Close, false, Mode::Chromatic, 0),
                   "C · C4");
    }

    /// An inversion is a different chord on the page, so it is a different
    /// chord on the screen: the bass note is named.
    #[test]
    fn a_voicing_that_moves_the_bass_is_spelled_over_it() {
        let symbol = chord_symbol(60, Chord::Min7, Voicing::First, false, Mode::Chromatic, 0);
        assert_eq!(symbol, "Cm7/D#");
        // ...and a close voicing is not.
        assert_eq!(
            chord_symbol(60, Chord::Min7, Voicing::Close, false, Mode::Chromatic, 0),
            "Cm7"
        );
        // Root-below puts the root underneath, which is still the root.
        assert_eq!(
            chord_symbol(60, Chord::Maj, Voicing::Close, true, Mode::Chromatic, 0),
            "C"
        );
    }

    /// A diatonic chord is named by what it turned out to be. "Diatonic" on
    /// screen would tell a player nothing about what they are hearing.
    #[test]
    fn a_diatonic_chord_is_named_by_its_quality() {
        // ii in C major is D minor.
        assert_eq!(
            chord_symbol(62, Chord::Diatonic, Voicing::Close, false, Mode::Ionian, 0),
            "Dm"
        );
        // V7 is a dominant seventh.
        assert_eq!(
            chord_symbol(67, Chord::Diatonic7, Voicing::Close, false, Mode::Ionian, 0),
            "G7"
        );
        // vii7 is the one quality the table has no entry for.
        assert_eq!(
            chord_symbol(71, Chord::Diatonic7, Voicing::Close, false, Mode::Ionian, 0),
            "Bm7b5"
        );
    }

    /// The numerals, cased by quality, in every mode. A numeral whose case
    /// does not match the chord is worse than no numeral.
    #[test]
    fn degree_numerals_are_cased_by_quality_in_every_mode() {
        let expected: [(Mode, [&str; 7]); 7] = [
            (Mode::Ionian, ["I", "ii", "iii", "IV", "V", "vi", "vii°"]),
            (Mode::Dorian, ["i", "ii", "III", "IV", "v", "vi°", "VII"]),
            (Mode::Phrygian, ["i", "II", "III", "iv", "v°", "VI", "vii"]),
            (Mode::Lydian, ["I", "II", "iii", "iv°", "V", "vi", "vii"]),
            (Mode::Mixolydian, ["I", "ii", "iii°", "IV", "v", "vi", "VII"]),
            (Mode::Aeolian, ["i", "ii°", "III", "iv", "v", "VI", "VII"]),
            (Mode::Locrian, ["i°", "II", "iii", "iv", "V", "VI", "vii"]),
        ];
        for tonic in 0..12u8 {
            for (mode, numerals) in &expected {
                let scale = mode.scale().unwrap();
                for (degree, numeral) in numerals.iter().enumerate() {
                    let note = (60 + i32::from(tonic) + scale[degree]) as u8;
                    assert_eq!(
                        degree_label(*mode, tonic, note),
                        Some(*numeral),
                        "{mode:?} degree {} in tonic {tonic}",
                        degree + 1
                    );
                }
            }
        }
        // A note outside the scale has no degree, and Chromatic has no
        // degrees at all.
        assert_eq!(degree_label(Mode::Ionian, 0, 61), None);
        assert_eq!(degree_label(Mode::Chromatic, 0, 60), None);
    }

    /// The property step-record rests on: whatever the chord table can
    /// produce, the identifier can name — and naming it produces the same
    /// notes back. Not necessarily the same spelling, because several
    /// spellings give the same notes and any of them is a correct answer;
    /// the notes are what the player hears.
    #[test]
    fn everything_the_table_can_play_can_be_identified_again() {
        for chord in Chord::ALL {
            if matches!(chord, Chord::Diatonic | Chord::Diatonic7) {
                continue;
            }
            for voicing in Voicing::ALL {
                for below in [false, true] {
                    for root in (36..=84u8).step_by(1) {
                        let played = notes_of(root, chord, voicing, below, Mode::Chromatic);
                        let named = identify(&played, Mode::Chromatic, 0).unwrap_or_else(|| {
                            panic!("{chord:?}/{voicing:?} below={below} at {root} has no name")
                        });
                        let back = notes_of(
                            named.root,
                            named.chord,
                            named.voicing,
                            named.root_below,
                            Mode::Chromatic,
                        );
                        assert_eq!(
                            back, played,
                            "{chord:?}/{voicing:?} below={below} at {root} came back different"
                        );
                    }
                }
            }
        }
    }

    /// The obvious answer, not merely a correct one: a plain triad played in
    /// close position comes back as a plain triad.
    #[test]
    fn the_obvious_reading_wins() {
        let named = identify(&[60, 64, 67], Mode::Chromatic, 0).unwrap();
        assert_eq!(
            (named.root, named.chord, named.voicing, named.root_below),
            (60, Chord::Maj, Voicing::Close, false)
        );

        let named = identify(&[60], Mode::Chromatic, 0).unwrap();
        assert_eq!((named.root, named.chord), (60, Chord::None));

        let named = identify(&[60, 63, 67, 70], Mode::Chromatic, 0).unwrap();
        assert_eq!((named.root, named.chord), (60, Chord::Min7));
    }

    /// Notes the table cannot make are refused rather than approximated: a
    /// wrong name would put a chord in the pattern that the player did not
    /// play.
    #[test]
    fn a_cluster_has_no_name() {
        assert_eq!(identify(&[60, 61, 62], Mode::Chromatic, 0), None);
        assert_eq!(identify(&[], Mode::Chromatic, 0), None);
        assert_eq!(identify(&[60, 62, 64, 66, 68, 70], Mode::Chromatic, 0), None);
    }

    /// Order and duplicates come from a keyboard, not from a chord: two
    /// fingers on the same note are one note.
    #[test]
    fn held_notes_are_identified_however_they_arrive() {
        let straight = identify(&[60, 64, 67], Mode::Chromatic, 0);
        assert_eq!(identify(&[67, 60, 64], Mode::Chromatic, 0), straight);
        assert_eq!(identify(&[64, 60, 67, 60], Mode::Chromatic, 0), straight);
    }
}
