//! What each deck control means to the app.
//!
//! The fixed controls are keys the app already understands — the deck's
//! UNDO is `u`, its LOCK is Enter — so every rule a key obeys applies to
//! them by construction. The controls that follow the screen (NAVIGATE, the
//! eight column knobs and the buttons under them, the pads) name what they
//! are instead, and the app decides what they do from what is on screen
//! ([`super::screen`]). A control with nothing behind it yet says so in
//! words ([`Intent::Unbuilt`]).

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

/// The arrow pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
}

/// What the FUNCTION knob is turning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FnTarget {
    #[default]
    Tempo,
    Swing,
    Grid,
    Master,
    Loop,
    /// Whichever column knob was turned last.
    Last,
}

/// What the eight track buttons do, whichever mode button is lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TrackMode {
    #[default]
    Select,
    Mute,
    Solo,
    Arm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    /// Press this key.
    Key(Chord),
    /// What Space and then this letter does — the menu's own road, so its
    /// refusals hold.
    Menu(char),
    /// The modifier, held.
    Shift,
    /// One of the arrows, SHIFT folded in as the stride.
    Nav { dir: Nav, stride: bool },
    /// The NAVIGATE knob: walk what the screen lists.
    Navigate,
    /// The FUNCTION knob: turn whatever function is selected.
    Function,
    /// Point the FUNCTION knob at something.
    FunctionTarget(FnTarget),
    /// Loop-record on the selected track.
    LoopRecord,
    /// Count-in: off, one bar, two bars.
    CountIn,
    /// Tap tempo.
    Tap,
    /// The list this screen loads from: presets, samples, sessions.
    Browse,
    /// The next part of the selected track.
    NextPart,
    /// Remove the thing under the cursor, asking where the app asks.
    Delete,
    /// Make a copy of the thing under the cursor, beside it.
    Duplicate,
    /// Column knob `n`: turned it adjusts, pushed it shows its name.
    Knob(u8),
    /// The action button under column `n`.
    Action(u8),
    /// Track button `n`, doing what the lit mode says.
    TrackButton(u8),
    /// Fader `n`, set by where it sits.
    Volume(u8),
    /// Pad `n`: a step, a sampler pad or a note, by what is on screen.
    Pad(u8),
    /// Page the column knobs: −1 back, +1 on.
    Page(i8),
    TrackMode(TrackMode),
    /// Move the faders and track buttons by eight tracks.
    TrackBank(i8),
    /// Move the pads up or down an octave.
    PadOctave(i8),
    /// Show steps 1-16 (`0`) or 17-32 (`1`) on the pads.
    StepHalf(u8),
    /// Nothing behind it yet — the words say what is missing.
    Unbuilt(&'static str),
}

/// What `id` does when pressed, with SHIFT held or not. The three kinds of
/// knob answer here for their pushes; their turns are
/// [`Intent::Navigate`], [`Intent::Function`] and [`Intent::Knob`].
pub fn bind(id: ControlId, shift: bool) -> Intent {
    use ControlId as C;
    use Intent as I;
    let ch = Chord::ch;
    let nav = |dir| I::Nav { dir, stride: shift };
    match id {
        C::Play => I::Menu('p'),
        C::Stop => I::Menu('0'),
        C::Rec => I::Menu('r'),
        C::Overdub => I::LoopRecord,
        C::Loop => I::Menu('l'),
        C::Click => I::Menu('m'),
        C::CountIn => I::CountIn,
        C::Tap => I::Tap,
        C::Function => I::Function,
        C::FnTempo => I::FunctionTarget(FnTarget::Tempo),
        C::FnSwing => I::FunctionTarget(FnTarget::Swing),
        C::FnGrid => I::FunctionTarget(FnTarget::Grid),
        C::FnMaster => I::FunctionTarget(FnTarget::Master),
        C::FnLoop => I::FunctionTarget(FnTarget::Loop),
        C::FnLast => I::FunctionTarget(FnTarget::Last),
        C::Navigate | C::Lock => I::Key(Chord::key(Key::Enter)),
        C::Back => I::Key(Chord::key(Key::Esc)),
        C::Browse => I::Browse,
        C::Shift => I::Shift,
        C::Menu => I::Key(ch(' ')),
        C::Left => nav(Nav::Left),
        C::Up => nav(Nav::Up),
        C::Down => nav(Nav::Down),
        C::Right => nav(Nav::Right),
        C::Part if shift => I::Key(Chord::key(Key::BackTab)),
        C::Part => I::NextPart,
        C::Undo => I::Key(ch('u')),
        C::Redo => I::Key(Chord::ctrl('r')),
        C::Copy => I::Key(ch(if shift { 'Y' } else { 'y' })),
        C::Paste => I::Key(ch(if shift { 'P' } else { 'p' })),
        C::Delete => I::Delete,
        C::Duplicate => I::Duplicate,
        C::Save => I::Key(Chord::ctrl('s')),
        C::New => I::Key(ch('n')),
        C::Knob(n) => I::Knob(n),
        C::Action(n) => I::Action(n),
        C::TrackButton(n) => I::TrackButton(n),
        C::Fader(n) => I::Volume(n),
        C::Pad(n) => I::Pad(n),
        C::PagePrev => I::Page(-1),
        C::PageNext => I::Page(1),
        C::MyPage => I::Unbuilt("a favourites page needs saving with each instrument's presets"),
        C::ModeSelect => I::TrackMode(TrackMode::Select),
        C::ModeMute => I::TrackMode(TrackMode::Mute),
        C::ModeSolo => I::TrackMode(TrackMode::Solo),
        C::ModeArm => I::TrackMode(TrackMode::Arm),
        C::TrackBankPrev => I::TrackBank(-1),
        C::TrackBankNext => I::TrackBank(1),
        C::PadsUp => I::PadOctave(1),
        C::PadsDown => I::PadOctave(-1),
        C::StepsLow => I::StepHalf(0),
        C::StepsHigh => I::StepHalf(1),
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

    /// Every control on the panel means something — a key, a named job, or
    /// the words for what it is still waiting on.
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
    fn lock_and_back_are_enter_and_esc() {
        assert_eq!(bind(ControlId::Lock, false), Intent::Key(Chord::key(Key::Enter)));
        assert_eq!(bind(ControlId::Navigate, false), Intent::Key(Chord::key(Key::Enter)), "NAVIGATE's push is LOCK");
        assert_eq!(bind(ControlId::Back, false), Intent::Key(Chord::key(Key::Esc)));
        assert_eq!(bind(ControlId::Copy, true), Intent::Key(Chord::ch('Y')));
    }
}
