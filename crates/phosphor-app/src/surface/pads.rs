//! What a pad hit becomes on its way in: a step, or a note.
//!
//! The pads change job with the screen — sequencer steps on a step grid,
//! notes everywhere else — but a note has to reach the instrument from the
//! MIDI callback, the one road into the audio engine, and the callback
//! cannot ask the app what is on screen. So the app tells it, through a
//! [`PadRoute`] of atomics it updates whenever the screen changes, and the
//! callback calls [`route`] on every message, saying whether it came from the
//! deck's own port — the channel alone never makes a message the deck's, so a
//! keyboard set to channel 16 still plays. The tests' deck calls the same
//! function, so the road a test's pad takes is the road the hardware's takes.
//!
//! The pads are two rows of eight: `Pad(0..8)` the top row, `Pad(8..16)` the
//! bottom. As steps they read like a bar, 1–8 along the top and 9–16 along
//! the bottom. As notes the bottom row is the lower octave's eight and the
//! top row the next eight, so pitch rises up and to the right, the way pads
//! have always been laid out.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use phosphor_midi::{MidiMessage, MidiMessageType};

use super::layout::{decode, ControlId, DeckInput, PADS};

/// No note sounding on this pad.
const SILENT: u8 = 0xFF;

/// The pads' current job, shared between the app and the MIDI callback.
pub struct PadRoute {
    /// Notes rather than steps.
    play: AtomicBool,
    /// The note the bottom-left pad plays.
    base: AtomicU8,
    /// The note each pad started, so its release ends that note even if the
    /// pads moved an octave while it was held.
    held: [AtomicU8; PADS as usize],
}

impl Default for PadRoute {
    fn default() -> Self {
        Self {
            // Notes until the app says otherwise: the first screen is the
            // track list, and a pad hit before any other input is a note.
            play: AtomicBool::new(true),
            base: AtomicU8::new(48),
            held: std::array::from_fn(|_| AtomicU8::new(SILENT)),
        }
    }
}

impl PadRoute {
    /// The app's word on what the pads are: notes from `base`, or steps.
    pub fn set(&self, play: bool, base: u8) {
        self.play.store(play, Ordering::Relaxed);
        self.base.store(base.min(127 - 15), Ordering::Relaxed);
    }

    pub fn plays_notes(&self) -> bool {
        self.play.load(Ordering::Relaxed)
    }

    pub fn base(&self) -> u8 {
        self.base.load(Ordering::Relaxed)
    }
}

/// How far above the base pad `n` plays: the bottom row first.
pub fn note_offset(pad: u8) -> u8 {
    if pad < 8 { pad + 8 } else { pad - 8 }
}

/// One message for the app: what arrived, and whether it is a deck control
/// rather than a performance.
#[derive(Debug, Clone, Copy)]
pub struct Tap {
    pub message: MidiMessage,
    pub deck: bool,
}

/// Where a message goes: to the engine (a performance), and to the app.
#[derive(Debug, Clone, Copy)]
pub struct Routed {
    /// What the instrument hears, if anything. A deck button never reaches
    /// it; a pad in note mode reaches it as a note.
    pub engine: Option<MidiMessage>,
    /// What the app sees — the message as sent, or the note a pad became.
    pub app: Tap,
}

fn note(on: bool, note: u8, velocity: u8, stamp: Option<u64>) -> MidiMessage {
    let (status, message_type) = if on {
        (0x90, MidiMessageType::NoteOn { channel: 0, note, velocity })
    } else {
        (0x80, MidiMessageType::NoteOff { channel: 0, note, velocity: 0 })
    };
    MidiMessage { received_micros: stamp, message_type, raw: [status, note, velocity], len: 3 }
}

fn performance(message: MidiMessage) -> Routed {
    Routed { engine: Some(message), app: Tap { message, deck: false } }
}

/// Route one message off the wire, from the deck's port or another's.
/// Real-time safe: atomics only.
pub fn route(message: MidiMessage, from_deck: bool, pads: &PadRoute) -> Routed {
    // Anything not from the deck is a performance, whatever its channel; so
    // is anything from the deck that is not one of its controls.
    let Some(input) = decode(message.message_type).filter(|_| from_deck) else {
        return performance(message);
    };
    if pads.plays_notes() {
        let stamp = message.received_micros;
        match input {
            DeckInput::Hit(ControlId::Pad(n), velocity) => {
                let pitch = pads.base().saturating_add(note_offset(n)).min(127);
                pads.held[usize::from(n)].store(pitch, Ordering::Relaxed);
                return performance(note(true, pitch, velocity, stamp));
            }
            DeckInput::Release(ControlId::Pad(n)) => {
                let pitch = pads.held[usize::from(n)].swap(SILENT, Ordering::Relaxed);
                if pitch != SILENT {
                    return performance(note(false, pitch, 0, stamp));
                }
            }
            _ => {}
        }
    }
    // A deck control: the app's alone.
    Routed { engine: None, app: Tap { message, deck: true } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::layout::encode;

    fn msg(input: DeckInput) -> MidiMessage {
        MidiMessage::from_bytes(&encode(input)).unwrap()
    }

    #[test]
    fn a_keyboard_note_goes_to_the_instrument_and_the_app() {
        let route_ = PadRoute::default();
        let key = MidiMessage::from_bytes(&[0x90, 60, 100]).unwrap();
        let r = route(key, false, &route_);
        assert_eq!(r.engine.unwrap().message_type, key.message_type);
    }

    /// A keyboard set to channel 16 sends what the deck's PLAY sends. From
    /// any port but the deck's it is a note, and it plays.
    #[test]
    fn channel_16_from_a_keyboard_is_a_performance() {
        let route_ = PadRoute::default();
        let r = route(msg(DeckInput::Press(ControlId::Play)), false, &route_);
        assert!(r.engine.is_some(), "a keyboard on channel 16 was taken for the deck");
        assert!(!r.app.deck);
    }

    #[test]
    fn a_deck_button_never_reaches_the_instrument() {
        let route_ = PadRoute::default();
        route_.set(true, 48);
        assert!(route(msg(DeckInput::Press(ControlId::Play)), true, &route_).engine.is_none());
    }

    #[test]
    fn in_step_mode_a_pad_is_the_apps() {
        let route_ = PadRoute::default();
        route_.set(false, 48);
        let r = route(msg(DeckInput::Hit(ControlId::Pad(3), 90)), true, &route_);
        assert!(r.engine.is_none());
        assert!(r.app.deck);
        assert_eq!(decode(r.app.message.message_type), Some(DeckInput::Hit(ControlId::Pad(3), 90)));
    }

    /// Bottom-left is the base; pitch rises along the row and up a row.
    #[test]
    fn in_note_mode_pads_play_upward_from_the_bottom_left() {
        let route_ = PadRoute::default();
        route_.set(true, 36);
        let pitch = |n: u8| match route(msg(DeckInput::Hit(ControlId::Pad(n), 100)), true, &route_).engine {
            Some(MidiMessage { message_type: MidiMessageType::NoteOn { note, velocity: 100, .. }, .. }) => note,
            other => panic!("pad {n} became {other:?}"),
        };
        assert_eq!(pitch(8), 36, "bottom-left is the base");
        assert_eq!(pitch(15), 43);
        assert_eq!(pitch(0), 44, "top-left is the next eight");
        assert_eq!(pitch(7), 51);
    }

    /// The pads moved an octave while a pad was held: its release still ends
    /// the note it started.
    #[test]
    fn before_the_app_has_spoken_the_pads_are_notes() {
        assert!(PadRoute::default().plays_notes());
    }

    #[test]
    fn a_held_pad_lets_go_of_the_note_it_started() {
        let route_ = PadRoute::default();
        route_.set(true, 36);
        route(msg(DeckInput::Hit(ControlId::Pad(8), 100)), true, &route_);
        route_.set(true, 48);
        let off = route(msg(DeckInput::Release(ControlId::Pad(8))), true, &route_).engine.unwrap();
        assert!(matches!(off.message_type, MidiMessageType::NoteOff { note: 36, .. }), "{off:?}");
    }
}
