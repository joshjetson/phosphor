//! The deck's door into the app.
//!
//! One message off the wire, decoded through the panel's own table
//! ([`phosphor_app::surface::layout`]), bound to what it means
//! ([`phosphor_app::surface::binding`]), and acted on here. The real deck and
//! the simulator the tests drive both arrive at [`App::handle_deck_input`].
//!
//! The fixed controls are keys, through [`App::handle_event`] — the deck's
//! LOCK is the keyboard's Enter, guarded by the same rules. The controls that
//! follow the screen read it ([`phosphor_app::surface::screen`]) and act
//! through the functions the keys reach: the column knobs in `deck_bank`, the
//! track buttons and faders in `deck_tracks`, the function knob and its
//! neighbours in `deck_function`. Nothing here changes what a key does, so
//! Phosphor without a deck is exactly the Phosphor it was.

use super::*;

use phosphor_app::sequencer::ops::SeqOp;
use phosphor_app::surface::actions::{actions, Act};
use phosphor_app::surface::binding::{bind, nav_letter, Chord, FnTarget, Intent, Key, Nav, TrackMode};
use phosphor_app::surface::fader::Pickup;
use phosphor_app::surface::layout::{self, control, ControlId, DeckInput, COLUMNS};
use phosphor_app::surface::screen::{screen, Axis, Screen};

/// What the deck is holding: the modifier, the knob page, the function
/// knob's target, the track buttons' mode and bank, the pads' octave, and
/// each fader's grip on its track.
#[derive(Debug, Default)]
pub(crate) struct DeckState {
    pub(crate) shift: bool,
    /// The column knobs' page, on `page_screen`. A new screen starts at
    /// page one.
    pub(crate) page: usize,
    pub(crate) page_screen: Option<Screen>,
    pub(crate) function: FnTarget,
    /// The last column knob turned, for LAST: the screen it was on and the
    /// control it turned.
    pub(crate) last_knob: Option<(Screen, super::deck_bank::Ctl)>,
    pub(crate) track_mode: TrackMode,
    /// Which eight tracks the faders and track buttons hold.
    pub(crate) track_bank: usize,
    /// Octaves the pads are moved from their home.
    pub(crate) pad_octave: i8,
    /// Steps 1-16 (`0`) or 17-32 (`1`).
    pub(crate) step_half: u8,
    pub(crate) pickups: [Pickup; COLUMNS as usize],
    /// Which track each fader last held and the level it left it at.
    pub(crate) held: [Option<(usize, Option<i32>)>; COLUMNS as usize],
    /// Recent TAP presses.
    pub(crate) taps: Vec<std::time::Instant>,
    /// A deck has spoken: from now on its line is drawn.
    pub(crate) seen: bool,
}

impl App {
    /// A message from the wire. `true` when it was the deck's and has been
    /// acted on; anything else is a keyboard's, and goes on its way.
    pub(crate) fn handle_deck_message(&mut self, message: phosphor_midi::MidiMessageType) -> bool {
        match layout::decode(message) {
            Some(input) => {
                self.handle_deck_input(input);
                true
            }
            None => false,
        }
    }

    /// One gesture on the deck.
    pub(crate) fn handle_deck_input(&mut self, input: DeckInput) {
        self.deck.seen = true;
        // The deck lights some controls itself as they are pressed and let
        // go; whatever it did, the next frame says what the light should be.
        if let (DeckInput::Press(id) | DeckInput::Hit(id, _) | DeckInput::Release(id), Some(out)) =
            (input, self.deck_out.as_mut())
        {
            out.forget(id);
        }
        match input {
            DeckInput::Turn(id, detents) => self.turn_deck(id, detents),
            DeckInput::Move(ControlId::Fader(n), position) => self.move_deck_fader(n, position),
            DeckInput::Move(..) => {}
            DeckInput::Press(id) | DeckInput::Hit(id, _) => {
                let intent = bind(id, self.deck.shift);
                if intent == Intent::Shift {
                    self.deck.shift = true;
                } else {
                    self.run_intent(id, intent);
                }
            }
            DeckInput::Release(id) => {
                if bind(id, false) == Intent::Shift {
                    self.deck.shift = false;
                }
            }
        }
        self.after_input();
    }

    fn turn_deck(&mut self, id: ControlId, detents: i8) {
        match id {
            ControlId::Navigate => self.navigate(detents),
            ControlId::Function => self.turn_function(detents),
            ControlId::Knob(n) => self.turn_knob(n, detents),
            _ => {}
        }
    }

    fn run_intent(&mut self, id: ControlId, intent: Intent) {
        match intent {
            Intent::Key(chord) => self.press_chord(chord),
            Intent::Menu(letter) => self.press_menu(letter),
            Intent::Nav { dir, stride } => self.press_nav(dir, stride),
            Intent::Shift | Intent::Navigate | Intent::Function | Intent::Volume(_) => {}
            Intent::FunctionTarget(target) => self.point_function(target),
            Intent::LoopRecord => self.toggle_loop_record(),
            Intent::CountIn => self.cycle_count_in(),
            Intent::Tap => self.tap_tempo(std::time::Instant::now()),
            Intent::Browse => self.deck_browse(),
            Intent::NextPart => self.next_part(),
            Intent::Delete => self.deck_delete(),
            Intent::Duplicate => self.deck_duplicate(),
            Intent::Knob(n) => self.show_knob(n),
            Intent::Action(n) => self.deck_action(n),
            Intent::TrackButton(n) => self.track_button(n),
            Intent::Pad(n) => self.deck_pad(n),
            Intent::Page(delta) => self.deck_page(delta),
            Intent::TrackMode(mode) => {
                self.deck.track_mode = mode;
                self.flash(format!("track buttons: {}", format!("{mode:?}").to_lowercase()));
            }
            Intent::TrackBank(delta) => self.deck_track_bank(delta),
            Intent::PadOctave(delta) => {
                self.deck.pad_octave = (self.deck.pad_octave + delta).clamp(-3, 4);
                let base = self.pad_base();
                self.flash(format!("pads from {}", phosphor_app::format::note_name(base)));
            }
            Intent::StepHalf(half) => {
                self.deck.step_half = half;
                self.flash(if half == 0 { "pads: steps 1-16" } else { "pads: steps 17-32" });
            }
            Intent::Unbuilt(words) => {
                self.flash(format!("{}: not built yet \u{00b7} {words}", control(id).label()));
            }
        }
    }

    /// The checks every input owes, wherever it came from — see
    /// [`App::handle_event`], whose own tail this is.
    pub(crate) fn after_input(&mut self) {
        self.reconcile_sampler_preview();
        self.reconcile_sampler_source();
        self.reconcile_sampler_learn();
        // A Drum Rack's kit changes by the knob, undo, a session opening and
        // a kit saved over; the engine hears about all of them from here.
        self.reconcile_user_kits();
        self.nav.follow_focus();
        self.refresh_deck();
    }

    /// Bring the deck's view of the screen up to date: the pads' road for
    /// the callback, and — once a deck has spoken — its line on screen.
    fn refresh_deck(&mut self) {
        let now = screen(&self.nav);
        if self.deck.page_screen != Some(now) {
            self.deck.page_screen = Some(now);
            self.deck.page = 0;
        }
        let plays_notes = now != Screen::Steps;
        let base = self.pad_base();
        self.deck_pads.set(plays_notes, base);
        if self.deck.seen {
            self.nav.deck_strip = Some(self.deck_line());
        }
    }

    /// The note the bottom-left pad plays: C2 on a sampler, where a chop
    /// starts, C3 elsewhere, moved by PADS ▲ ▼.
    pub(crate) fn pad_base(&self) -> u8 {
        let home: i32 = if self.nav.current_track().is_some_and(|t| t.sampler.is_some()) { 36 } else { 48 };
        (home + 12 * i32::from(self.deck.pad_octave)).clamp(0, 112) as u8
    }

    /// A key, through the keyboard's own door.
    pub(crate) fn press_chord(&mut self, chord: Chord) {
        let (code, mut modifiers) = match chord.key {
            Key::Char(c) => (
                KeyCode::Char(c),
                if c.is_ascii_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE },
            ),
            Key::Enter => (KeyCode::Enter, KeyModifiers::NONE),
            Key::Esc => (KeyCode::Esc, KeyModifiers::NONE),
            Key::Tab => (KeyCode::Tab, KeyModifiers::NONE),
            Key::BackTab => (KeyCode::BackTab, KeyModifiers::SHIFT),
        };
        if chord.ctrl {
            modifiers |= KeyModifiers::CONTROL;
        }
        self.press_code(code, modifiers);
    }

    fn press_code(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.handle_event(crossterm::event::Event::Key(crossterm::event::KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }));
    }

    /// Whether the keys are being typed into a field, where a letter is a
    /// letter and only the arrow keys move.
    fn typing(&self) -> bool {
        self.nav.input_modal.open || (self.nav.file_picker.open && !self.nav.file_picker.list_owns_letters())
    }

    pub(crate) fn press_nav(&mut self, dir: Nav, stride: bool) {
        if self.typing() {
            let code = match dir {
                Nav::Up => KeyCode::Up,
                Nav::Down => KeyCode::Down,
                Nav::Left => KeyCode::Left,
                Nav::Right => KeyCode::Right,
            };
            self.press_code(code, if stride { KeyModifiers::SHIFT } else { KeyModifiers::NONE });
        } else {
            self.press_chord(Chord::ch(nav_letter(dir, stride)));
        }
    }

    /// Space, then a letter: the menu's own road. When the menu will not
    /// open — source mode turns it away — the letter is not sent, because
    /// without the menu it would land on whatever is under it.
    pub(crate) fn press_menu(&mut self, letter: char) {
        self.nav.space_menu.open = false;
        self.press_chord(Chord::ch(' '));
        if self.nav.space_menu.open {
            self.press_chord(Chord::ch(letter));
        }
    }

    /// NAVIGATE: down the screen's list, or along its row. SHIFT strides.
    fn navigate(&mut self, detents: i8) {
        let axis = screen(&self.nav).axis();
        let dir = match (axis, detents >= 0) {
            (Axis::List, true) => Nav::Down,
            (Axis::List, false) => Nav::Up,
            (Axis::Row, true) => Nav::Right,
            (Axis::Row, false) => Nav::Left,
        };
        for _ in 0..detents.unsigned_abs() {
            self.press_nav(dir, self.deck.shift);
        }
    }

    /// The action button under column `n`.
    fn deck_action(&mut self, n: u8) {
        let now = screen(&self.nav);
        match actions(now, self.deck.shift).get(usize::from(n)) {
            Some((_, Act::Key(chord))) => self.press_chord(*chord),
            Some((_, Act::Menu(letter))) => self.press_menu(*letter),
            None => self.flash(format!("{}: no action here", now.label())),
        }
    }

    /// A pad that reached the app as itself: a step, on a step grid. In every
    /// other place the pads are notes, and the callback has already turned
    /// them into one before they got here.
    fn deck_pad(&mut self, n: u8) {
        if screen(&self.nav) != Screen::Steps {
            return;
        }
        let step = self.deck.step_half * 16 + n;
        self.sequencer_op(SeqOp::SelectStep(step));
        self.sequencer_op(SeqOp::ToggleStep);
        self.flash(format!("step {}", step + 1));
    }
}
