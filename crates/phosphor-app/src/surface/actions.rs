//! The eight buttons under the column knobs: each screen's own actions.
//!
//! These are the actions that need a letter key on a keyboard — `t` trims a
//! pad, `b` bypasses an effect — named and laid out so the deck reaches them
//! with one press. Each is the very key the keyboard sends, so the action is
//! the keyboard's own, with its own undo step and its own refusals. A second
//! set sits under SHIFT for the screens with more than eight.
//!
//! The coverage audit reads these tables: every key in them counts as a key
//! the deck can send.

use super::binding::{Chord, Key};
use super::screen::Screen;

/// What one action button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    /// Press this key.
    Key(Chord),
    /// What Space and then this letter does.
    Menu(char),
}

/// One button: its name and what it does.
pub type Action = (&'static str, Act);

// Macros rather than functions: a list built from function calls cannot be
// a static, and these tables are statics so every screen's list is free.
macro_rules! k {
    ($c:expr) => {
        Act::Key(Chord { key: Key::Char($c), ctrl: false })
    };
}

macro_rules! ctrl {
    ($c:expr) => {
        Act::Key(Chord { key: Key::Char($c), ctrl: true })
    };
}

macro_rules! m {
    ($c:expr) => {
        Act::Menu($c)
    };
}

const ENTER: Act = Act::Key(Chord { key: Key::Enter, ctrl: false });

/// The actions on `screen`, SHIFT held or not. At most eight; a slot past the
/// end of the list does nothing.
pub fn actions(screen: Screen, shift: bool) -> &'static [Action] {
    use Screen as S;
    match (screen, shift) {
        (S::Prompt, false) => &[("YES", k!('y')), ("NO", k!('n')), ("TYPE PATH", k!('/'))],
        (S::Tracks, false) => &[
            ("ADD TRACK", m!('a')),
            ("OPEN", m!('o')),
            ("SAVE AS", m!('s')),
            ("PRESETS", m!('w')),
            ("QUANTIZE", m!('q')),
            ("PRACTICE", m!('f')),
            ("THEME", m!('v')),
            ("HELP", m!('h')),
        ],
        (S::Track, false) => &[
            ("DUP TRACK", k!('D')),
            ("RENAME", k!('n')),
            ("COPY CLIPS", k!('y')),
            ("PASTE CLIPS", k!('P')),
            ("DELETE", m!('d')),
            ("LOOP REC", k!('R')),
            ("ADD TRACK", m!('a')),
            ("EDIT NOTES", m!('e')),
        ],
        (S::TrackCell, false) => &[
            ("TRIM ◀", ctrl!('h')),
            ("TRIM ▶", ctrl!('l')),
            ("DUPLICATE", k!('d')),
            ("COPY", k!('y')),
            ("PASTE", k!('p')),
            ("PASTE AT", k!('P')),
        ],
        (S::Instrument, false) => &[("PRESETS", m!('w')), ("LOAD SOUND", k!('a'))],
        (S::FxChain, false) => &[
            ("ADD", k!('a')),
            ("OPEN", ENTER),
            ("BYPASS", k!('b')),
            ("MUTE", k!('m')),
            ("REMOVE", k!('d')),
            ("◀ MOVE", k!('[')),
            ("MOVE ▶", k!(']')),
            ("PRINT", k!('c')),
        ],
        (S::Effect, false) => &[("BYPASS", k!('b')), ("MUTE", k!('m')), ("BAND ON", k!('n'))],
        (S::MidiEffect, false) => &[
            ("BYPASS", k!('b')),
            ("MUTE", k!('m')),
            ("PROGRESSIONS", k!('e')),
            ("FEEL 1", k!('1')),
            ("FEEL 2", k!('2')),
            ("FEEL 3", k!('3')),
            ("FEEL 4", k!('4')),
        ],
        (S::Pads, false) => &[
            ("LOAD", k!('a')),
            ("TRIM", k!('t')),
            ("CHOP", k!('c')),
            ("RECORD", k!('i')),
            ("NORMALIZE", k!('n')),
            ("MUTE", k!('m')),
            ("◀ SOUND", k!('[')),
            ("SOUND ▶", k!(']')),
        ],
        (S::Pads, true) => &[("KEYS MODE", k!('K')), ("CHOP FILE", k!('C')), ("REMOVE", k!('d'))],
        (S::Zones, false) => &[
            ("LOAD", k!('a')),
            ("TRIM", k!('t')),
            ("WHOLE BED", k!('w')),
            ("OCTAVE", k!('o')),
            ("SPLIT", k!('s')),
            ("DROP ZONE", k!('D')),
            ("LEARN ROOT", k!('R')),
            ("PADS MODE", k!('K')),
        ],
        (S::Zones, true) => &[("CHOP", k!('c')), ("RECORD", k!('i')), ("◀ SOUND", k!('[')), ("SOUND ▶", k!(']'))],
        (S::Trim, false) => &[("HUG", k!('w')), ("SNAP", k!('z')), ("REVERSE", k!('r')), ("LOOP", k!('t'))],
        (S::Chop, false) => &[
            ("ADD CUT", k!('a')),
            ("REMOVE CUT", k!('d')),
            ("PLAY", k!('p')),
            ("LAND", k!('c')),
        ],
        (S::Source, false) => &[("RECORD", k!('r')), ("AUDIO/PHRASE", k!('p')), ("INSTRUMENT", k!('i'))],
        (S::Steps, false) => &[
            ("WRITE", k!('n')),
            ("ACCENT", k!('a')),
            ("TIE", k!('_')),
            ("REST", k!('.')),
            ("CLEAR", k!('x')),
            ("RUN", k!('t')),
            ("STEP REC", k!('r')),
            ("BOUNCE", k!('b')),
        ],
        (S::Steps, true) => &[
            ("MUTE LANE", k!('m')),
            ("SOLO LANE", k!('s')),
            ("◀ LANE", k!('[')),
            ("LANE ▶", k!(']')),
            ("CHAIN", k!('c')),
            ("UNCHAIN", k!('C')),
            ("CLEAR ALL", k!('X')),
        ],
        (S::Notes, false) => &[
            ("DRAW", k!('n')),
            ("◀ OCTAVE", k!('{')),
            ("OCTAVE ▶", k!('}')),
            ("◀ NOTE", k!('[')),
            ("NOTE ▶", k!(']')),
            ("PLAYHEAD", k!('g')),
            ("AUTOMATION", k!('A')),
            ("EDIT NOTES", m!('e')),
        ],
        (S::Notes, true) => &[("CLEAR CTRL", k!('X')), ("QUANTIZE", m!('q')), ("RECORD", k!('r'))],
        (S::EditMode, false) => &[
            ("VEL −", k!(',')),
            ("VEL +", k!('.')),
            ("VEL −−", k!('<')),
            ("VEL ++", k!('>')),
            ("MUTE", k!('m')),
            ("DELETE", k!('d')),
            ("DONE", k!('e')),
        ],
        (S::Automation, false) => &[
            ("◀ LANE", k!('[')),
            ("LANE ▶", k!(']')),
            ("RAMP", k!('r')),
            ("CLEAR", k!('d')),
            ("CLOSE", k!('A')),
        ],
        (S::Loop, false) => &[
            ("ON / OFF", ENTER),
            ("GRID", k!('g')),
            ("LIFT", k!('y')),
            ("CUT", k!('x')),
            ("STAMP", k!('p')),
            ("LAYER", k!('P')),
        ],
        (S::Practice, false) => &[
            ("START", ENTER),
            ("KEY −", k!('<')),
            ("KEY +", k!('>')),
            ("TEMPO −", k!('[')),
            ("TEMPO +", k!(']')),
            ("HANDS", k!('h')),
            ("WAIT/FLOW", k!('w')),
            ("CLICK", k!('c')),
        ],
        (S::Practice, true) => &[("◀ STEP", k!(',')), ("STEP ▶", k!('.'))],
        (S::Progressions, false) => &[
            ("◀", k!('[')),
            ("▶", k!(']')),
            ("ADD", k!('a')),
            ("LEARN", k!('r')),
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_screen_has_more_actions_than_buttons() {
        for screen in Screen::ALL {
            for shift in [false, true] {
                assert!(actions(screen, shift).len() <= 8, "{screen:?} has more than eight");
            }
        }
    }

    /// Two buttons on one screen doing the same thing would be a wasted
    /// button and a confusing label.
    #[test]
    fn no_screen_offers_one_action_twice() {
        for screen in Screen::ALL {
            let mut seen = std::collections::HashSet::new();
            for shift in [false, true] {
                for (label, act) in actions(screen, shift) {
                    assert!(seen.insert(*act), "{screen:?}: {label} repeats an action");
                }
            }
        }
    }
}
