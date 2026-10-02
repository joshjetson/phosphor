//! The deck's door into the app.
//!
//! One message off the wire, decoded through the panel's own table
//! ([`phosphor_app::surface::layout`]), bound to what it means
//! ([`phosphor_app::surface::binding`]), and acted on here. The real deck and
//! the simulator the tests drive both arrive at [`App::handle_deck_input`];
//! nothing about the deck is decided anywhere else.
//!
//! Keys go through [`App::handle_event`] — the deck's ENTER is the keyboard's
//! Enter, through the same door, guarded by the same rules. The handful of
//! controls that are not a key (a strip button, a fader, a step pad) call the
//! same functions those keys call, and then the same after-input checks.

use super::*;

use phosphor_app::sequencer::ops::SeqOp;
use phosphor_core::project::TrackKind;
use phosphor_app::surface::binding::{bind, nav_letter, Chord, Intent, Key, Nav, PadMode, StripVerb};
use phosphor_app::surface::fader::{steps_between, Pickup};
use phosphor_app::surface::layout::{self, control, ControlId, DeckInput, MASTER_FADER, STRIPS};

/// What the deck itself is holding: the modifier, the pad mode, and each
/// fader's grip on its track.
#[derive(Debug, Clone, Default)]
pub(crate) struct DeckState {
    pub(crate) shift: bool,
    pub(crate) pad_mode: PadMode,
    /// Steps 1-16 (`0`) or 17-32 (`1`).
    pub(crate) step_page: u8,
    pickups: [Pickup; MASTER_FADER as usize + 1],
    /// Which track each fader last held, and the level it left it at. A
    /// fader whose track has changed, or whose level was set by something
    /// else, has to catch it again.
    held: [Option<(usize, Option<i32>)>; MASTER_FADER as usize + 1],
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
        let (id, press) = match input {
            DeckInput::Press(id) | DeckInput::Hit(id, _) => (id, true),
            DeckInput::Release(id) => (id, false),
            DeckInput::Turn(id, detents) => return self.turn_deck(id, detents),
            DeckInput::Move(id, position) => return self.move_deck_fader(id, position),
        };
        let intent = bind(id, self.deck.shift);
        if intent == Intent::Shift {
            self.deck.shift = press;
            return;
        }
        // Everything else acts on the way down; the way up is a key let go,
        // which no handler listens for.
        if press {
            self.run_intent(id, intent);
        }
    }

    fn turn_deck(&mut self, id: ControlId, detents: i8) {
        match bind(id, self.deck.shift) {
            Intent::Turn { up, down } => {
                let chord = if detents >= 0 { up } else { down };
                for _ in 0..detents.unsigned_abs() {
                    self.press_chord(chord);
                }
            }
            intent => self.run_intent(id, intent),
        }
    }

    fn run_intent(&mut self, id: ControlId, intent: Intent) {
        match intent {
            Intent::Key(chord) => self.press_chord(chord),
            Intent::Nav { dir, stride } => self.press_nav(dir, stride),
            Intent::Menu(letter) => self.press_menu(letter),
            Intent::Turn { .. } => {
                // An encoder's push. VALUE's push is ENTER, the rest are
                // waiting on the label strip.
                if id == ControlId::Value {
                    self.press_chord(Chord::key(Key::Enter));
                } else {
                    self.flash(format!("{}: push does nothing yet", control(id).label()));
                }
            }
            Intent::Strip(verb, n) => self.strip(verb, n),
            Intent::LoopRecord => {
                self.toggle_loop_record();
                self.after_input();
            }
            Intent::Step(n) => self.deck_step(n),
            Intent::Lane(n) => {
                self.deck_seq(SeqOp::SelectLane(n), &format!("lane {}", n + 1));
            }
            Intent::Slot(n) => {
                let name = (b'A' + n) as char;
                self.deck_seq(SeqOp::QueueSlot(n), &format!("pattern {name} queued"));
            }
            Intent::Accent => {
                self.deck_seq(SeqOp::ToggleAccent, "accent");
            }
            Intent::PadMode(mode) => {
                self.deck.pad_mode = mode;
                self.flash(format!("pads: {}", format!("{mode:?}").to_lowercase()));
            }
            Intent::StepPage(page) => {
                self.deck.step_page = page;
                self.flash(if page == 0 { "pads: steps 1-16" } else { "pads: steps 17-32" });
            }
            Intent::Volume(_) | Intent::Shift => {}
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
    }

    /// A key, through the keyboard's own door.
    fn press_chord(&mut self, chord: Chord) {
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

    fn press_nav(&mut self, dir: Nav, stride: bool) {
        if self.typing() {
            let code = match dir {
                Nav::Up => KeyCode::Up,
                Nav::Down => KeyCode::Down,
                Nav::Left => KeyCode::Left,
                Nav::Right => KeyCode::Right,
            };
            let modifiers = if stride { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
            self.handle_event(crossterm::event::Event::Key(crossterm::event::KeyEvent {
                code,
                modifiers,
                kind: crossterm::event::KeyEventKind::Press,
                state: crossterm::event::KeyEventState::NONE,
            }));
        } else {
            self.press_chord(Chord::ch(nav_letter(dir, stride)));
        }
    }

    /// Space, then a letter: the menu's own road. When the menu will not
    /// open — source mode turns it away — the letter is not sent, because
    /// without the menu it would land on whatever is under it.
    fn press_menu(&mut self, letter: char) {
        self.nav.space_menu.open = false;
        self.press_chord(Chord::ch(' '));
        if self.nav.space_menu.open {
            self.press_chord(Chord::ch(letter));
        }
    }

    /// The track strip `n` stands for: the `n`th row of the app's window, or
    /// the master for `n == STRIPS`.
    fn strip_track(&self, n: u8) -> Option<usize> {
        if n >= STRIPS {
            return self.nav.tracks.iter().position(|t| t.kind == TrackKind::Master);
        }
        let idx = self.nav.track_scroll + usize::from(n);
        (idx < self.nav.tracks.len()).then_some(idx)
    }

    fn strip(&mut self, verb: StripVerb, n: u8) {
        let Some(idx) = self.strip_track(n) else {
            self.flash(format!("no track in row {}", n + 1));
            return;
        };
        match verb {
            StripVerb::Select => {
                self.nav.focus_pane(Pane::Tracks);
                self.nav.track_cursor = idx;
                self.nav.track_selected = true;
                self.nav.track_element = crate::state::TrackElement::Label;
                self.nav.show_current_track_controls();
            }
            // The same toggles `m`, `s` and `r` make, on this strip's track
            // rather than the cursor's — a strip button is a strip button.
            other => {
                let saved = self.nav.track_cursor;
                self.nav.track_cursor = idx;
                match other {
                    StripVerb::Mute => self.nav.toggle_mute(),
                    StripVerb::Solo => self.nav.toggle_solo(),
                    _ => self.nav.toggle_arm(),
                }
                self.nav.track_cursor = saved;
            }
        }
        self.after_input();
    }

    fn move_deck_fader(&mut self, id: ControlId, position: u8) {
        let ControlId::Fader(n) = id else { return };
        let Some(idx) = self.strip_track(n) else { return };
        let slot = usize::from(n);
        let current = self.nav.tracks[idx].volume_db().map(|db| db.round() as i32);
        // Someone else moved the level, or the row now holds another track:
        // the fader has to catch it again.
        if self.deck.held[slot].is_some_and(|(track, level)| track != idx || level != current) {
            self.deck.pickups[slot].release();
        }
        let Some(target) = self.deck.pickups[slot].moved(position, current) else {
            self.deck.held[slot] = Some((idx, current));
            return;
        };
        let steps = steps_between(current, target);
        if steps != 0 {
            let saved = self.nav.track_cursor;
            self.nav.track_cursor = idx;
            self.nav.adjust_volume(steps);
            self.nav.track_cursor = saved;
        }
        let now = self.nav.tracks[idx].volume_db().map(|db| db.round() as i32);
        self.deck.held[slot] = Some((idx, now));
        self.after_input();
    }

    /// A pad in STEP mode: that step of the selected lane, on the selected
    /// track's sequencer.
    fn deck_step(&mut self, n: u8) {
        let step = self.deck.step_page * 16 + n;
        if self.deck_seq(SeqOp::SelectStep(step), "") {
            self.deck_seq(SeqOp::ToggleStep, &format!("step {}", step + 1));
        }
    }

    /// One sequencer op on the selected track, when it has a sequencer.
    fn deck_seq(&mut self, op: SeqOp, words: &str) -> bool {
        if self.nav.current_track().and_then(|t| t.sequencer.as_ref()).is_none() {
            self.flash("the pads' STEP mode plays the selected track's sequencer \u{00b7} this track has none");
            return false;
        }
        self.sequencer_op(op);
        if !words.is_empty() {
            self.flash(words);
        }
        self.after_input();
        true
    }
}
