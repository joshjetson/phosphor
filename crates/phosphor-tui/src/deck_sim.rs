//! A Phosphor Deck in software, for tests.
//!
//! It does what the hardware does and nothing more: every gesture becomes the
//! three bytes the panel's table says that control sends
//! ([`phosphor_app::surface::layout::encode`]), parsed as the MIDI layer
//! parses a real message, and handed to the app through the same tap a real
//! deck's messages arrive on. A journey written against this is a journey a
//! player can make with the hardware — no key a test types reaches the app
//! by any other road.

use phosphor_app::surface::layout::{ControlId, DeckInput, MASTER_FADER};
use phosphor_core::EngineConfig;

use crate::app::App;

pub(crate) struct DeckSim {
    pub(crate) app: App,
}

impl DeckSim {
    /// A headless app with the deck attached, its pickers looking at
    /// `scratch` so no test reads or writes the home directory.
    pub(crate) fn new(scratch: &std::path::Path) -> Self {
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false);
        app.browse_samples = Some(scratch.join("samples"));
        app.browse_sessions = Some(scratch.join("sessions"));
        let _ = std::fs::create_dir_all(scratch.join("samples"));
        let _ = std::fs::create_dir_all(scratch.join("sessions"));
        Self { app }
    }

    fn send(&mut self, input: DeckInput) {
        let bytes = phosphor_app::surface::layout::encode(input);
        let message = phosphor_midi::MidiMessage::from_bytes(&bytes).expect("three bytes parse");
        self.app.handle_tap_event(message.message_type, None);
    }

    /// Press and let go.
    pub(crate) fn press(&mut self, id: ControlId) -> &mut Self {
        self.send(DeckInput::Press(id));
        self.send(DeckInput::Release(id));
        self
    }

    /// Press `id` `times` times.
    pub(crate) fn tap(&mut self, id: ControlId, times: usize) -> &mut Self {
        for _ in 0..times {
            self.press(id);
        }
        self
    }

    /// Press with SHIFT held.
    pub(crate) fn shift(&mut self, id: ControlId) -> &mut Self {
        self.send(DeckInput::Press(ControlId::Shift));
        self.press(id);
        self.send(DeckInput::Release(ControlId::Shift));
        self
    }

    /// Turn an encoder by `detents` clicks, one message per click as the
    /// firmware sends them at normal speed.
    pub(crate) fn turn(&mut self, id: ControlId, detents: i8) -> &mut Self {
        let step = if detents >= 0 { 1 } else { -1 };
        for _ in 0..detents.unsigned_abs() {
            self.send(DeckInput::Turn(id, step));
        }
        self
    }

    /// Slide fader `n` (5 is master) from where it is to `to`, through every
    /// position between, as a hand does.
    pub(crate) fn slide(&mut self, n: u8, from: u8, to: u8) -> &mut Self {
        assert!(n <= MASTER_FADER);
        let id = ControlId::Fader(n);
        if from <= to {
            for p in from..=to {
                self.send(DeckInput::Move(id, p));
            }
        } else {
            for p in (to..=from).rev() {
                self.send(DeckInput::Move(id, p));
            }
        }
        self
    }

    /// Strike pad `n` (STEP mode) at `velocity`.
    pub(crate) fn hit(&mut self, n: u8, velocity: u8) -> &mut Self {
        self.send(DeckInput::Hit(ControlId::Pad(n), velocity));
        self.send(DeckInput::Release(ControlId::Pad(n)));
        self
    }

    /// A note from the keyboard the deck is clamped to, on channel 1 — the
    /// performance road, which the pads in PADS and NOTE modes also take.
    pub(crate) fn play(&mut self, note: u8, velocity: u8) -> &mut Self {
        use phosphor_midi::MidiMessageType::{NoteOff, NoteOn};
        self.app.handle_tap_event(NoteOn { channel: 0, note, velocity }, None);
        self.app.handle_tap_event(NoteOff { channel: 0, note, velocity: 0 }, None);
        self
    }

    /// What the status line says right now.
    pub(crate) fn flash(&self) -> String {
        self.app.status_message.as_ref().map(|(m, _)| m.clone()).unwrap_or_default()
    }
}
