//! What each deck control means to the app.
//!
//! Almost everything is a key the app already understands: the deck is a
//! keyboard laid out for music, so pressing its ENTER is pressing Enter, and
//! every rule a key obeys — a held control taking every key, a modal
//! swallowing typing, the undo step a key records — applies to the deck by
//! construction. The few controls that are not a key name what they are
//! instead ([`Intent`]), and the ones the app has no answer for yet say so in
//! words ([`Intent::Unbuilt`]), which is what the coverage audit counts.

use super::layout::ControlId;

/// A key, as the app's handlers read it. Shift is spelled into the key (an
/// uppercase letter, `BackTab`) because that is how the terminal delivers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub ctrl: bool,
}

impl Chord {
    pub const fn key(key: Key) -> Self {
        Self { key, ctrl: false }
    }

    pub const fn ch(c: char) -> Self {
        Self::key(Key::Char(c))
    }

    pub const fn ctrl(c: char) -> Self {
        Self { key: Key::Char(c), ctrl: true }
    }
}

/// The arrow pad. Resolved to keys by the app, because the right key depends
/// on where the cursor is: `h`/`j`/`k`/`l` everywhere the house grammar runs,
/// the arrow keys in a field being typed into, where a letter would type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StripVerb {
    Select,
    Mute,
    Solo,
    Arm,
}

/// What the pads do when struck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PadMode {
    /// Each pad is a step of the selected sequencer lane.
    #[default]
    Step,
    /// The pads play sampler pads, from the deck's own notes on channel 1.
    Pads,
    /// The pads play notes, the same way.
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    /// Press this key.
    Key(Chord),
    /// One of the arrows, with SHIFT already folded in when `stride` is set.
    Nav { dir: Nav, stride: bool },
    /// What pressing Space and then this letter does — the Space menu's own
    /// road, so its refusals (source mode turning it away, say) hold.
    Menu(char),
    /// The modifier, held.
    Shift,
    /// An encoder: this chord per detent clockwise, that one per detent back.
    Turn { up: Chord, down: Chord },
    /// A strip button for visible track `n`; `n == STRIPS` is the master.
    Strip(StripVerb, u8),
    /// Fader `n`, set by where it sits.
    Volume(u8),
    /// Loop-record on the selected track — the tracks pane's `R`, from
    /// anywhere.
    LoopRecord,
    /// Step `n` of the current page, on the selected lane.
    Step(u8),
    /// Select lane `n`.
    Lane(u8),
    /// Queue pattern slot `n`.
    Slot(u8),
    PadMode(PadMode),
    /// Show steps 1-16 (`0`) or 17-32 (`1`).
    StepPage(u8),
    /// Accent the step under the cursor.
    Accent,
    /// A control the app has nothing behind yet — the words say what is
    /// missing. Counted by the coverage audit, and flashed when pressed so a
    /// player is never left wondering whether the button is broken.
    Unbuilt(&'static str),
}

/// What `id` does when pressed (or turned, or moved), with SHIFT held or not.
pub fn bind(id: ControlId, shift: bool) -> Intent {
    use ControlId as C;
    use Intent as I;
    let ch = Chord::ch;
    let nav = |dir| I::Nav { dir, stride: shift };
    match id {
        C::Play => I::Menu('p'),
        C::Stop => I::Menu('0'),
        C::Rec => I::Menu('r'),
        C::LoopRec => I::LoopRecord,
        C::Loop => I::Menu('l'),
        C::Click => I::Menu('m'),
        C::Count => I::Unbuilt("count-in has no key of its own; it lives in the transport pane"),
        C::Panic => I::Menu('!'),
        C::Tempo => I::Turn { up: ch('+'), down: ch('-') },
        C::PaneTransport => I::Menu('1'),
        C::PaneTracks => I::Menu('2'),
        C::PaneClip => I::Menu('3'),
        C::Value if shift => I::Turn { up: ch('L'), down: ch('H') },
        C::Value => I::Turn { up: ch('l'), down: ch('h') },
        C::Up => nav(Nav::Up),
        C::Down => nav(Nav::Down),
        C::Left => nav(Nav::Left),
        C::Right => nav(Nav::Right),
        C::Shift => I::Shift,
        C::Back => I::Key(Chord::key(Key::Esc)),
        C::Tab if shift => I::Key(Chord::key(Key::BackTab)),
        C::Tab => I::Key(Chord::key(Key::Tab)),
        C::Menu => I::Key(ch(' ')),
        C::Enter => I::Key(Chord::key(Key::Enter)),
        C::Save => I::Key(Chord::ctrl('s')),
        C::Bank(_) => I::Unbuilt("the eight encoders need the on-screen label strip"),
        C::PagePrev | C::PageNext => I::Unbuilt("encoder pages need the on-screen label strip"),
        C::Undo => I::Key(ch('u')),
        C::Redo => I::Key(Chord::ctrl('r')),
        C::Copy => I::Key(ch(if shift { 'Y' } else { 'y' })),
        C::Paste => I::Key(ch(if shift { 'P' } else { 'p' })),
        C::Delete => I::Key(ch(if shift { 'D' } else { 'd' })),
        C::New => I::Key(ch('n')),
        C::Track(n) => I::Strip(StripVerb::Select, n),
        C::Master => I::Strip(StripVerb::Select, super::layout::STRIPS),
        C::Mute(n) => I::Strip(StripVerb::Mute, n),
        C::Solo(n) => I::Strip(StripVerb::Solo, n),
        C::Arm(n) => I::Strip(StripVerb::Arm, n),
        C::Fader(n) => I::Volume(n),
        C::Pad(n) => I::Step(n),
        C::Lane(n) if shift => I::Slot(n),
        C::Lane(n) => I::Lane(n),
        C::ModeStep => I::PadMode(PadMode::Step),
        C::ModePads => I::PadMode(PadMode::Pads),
        C::ModeNote => I::PadMode(PadMode::Note),
        C::Accent => I::Accent,
        C::StepPage(n) => I::StepPage(n),
    }
}

/// The key an arrow sends, with SHIFT for the stride — `h`/`j`/`k`/`l` and
/// `H`/`J`/`K`/`L`, the house grammar.
pub fn nav_letter(dir: Nav, stride: bool) -> char {
    let c = match dir {
        Nav::Up => 'k',
        Nav::Down => 'j',
        Nav::Left => 'h',
        Nav::Right => 'l',
    };
    if stride { c.to_ascii_uppercase() } else { c }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::layout::deck;

    /// Every control on the panel means something — a key, a named action,
    /// or the words for what it is still waiting on. None is silent.
    #[test]
    fn every_control_is_bound_with_and_without_shift() {
        for c in deck() {
            for shift in [false, true] {
                let _ = bind(c.id, shift);
            }
        }
    }

    #[test]
    fn the_arrows_speak_the_house_grammar() {
        assert_eq!(nav_letter(Nav::Left, false), 'h');
        assert_eq!(nav_letter(Nav::Right, true), 'L');
        assert_eq!(nav_letter(Nav::Up, false), 'k');
        assert_eq!(nav_letter(Nav::Down, true), 'J');
    }

    #[test]
    fn shift_turns_copy_into_copy_one_sound_and_lanes_into_patterns() {
        assert_eq!(bind(ControlId::Copy, true), Intent::Key(Chord::ch('Y')));
        assert_eq!(bind(ControlId::Lane(2), true), Intent::Slot(2));
        assert_eq!(bind(ControlId::Lane(2), false), Intent::Lane(2));
        assert_eq!(bind(ControlId::Tab, true), Intent::Key(Chord::key(Key::BackTab)));
    }
}
