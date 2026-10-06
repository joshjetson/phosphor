//! MIDI output to the Phosphor Deck: its lights.
//!
//! The deck is found by its port name, as its input is, and looked for again
//! every couple of seconds so it can be plugged in at any time. What each
//! light should show is worked out by [`phosphor_app::surface::lights`];
//! this sends only what changed since the deck was last told, a few bytes a
//! frame at most, and says it all again when the deck appears.
//!
//! Without a deck nothing here does anything: there is no port to find, and
//! [`DeckOut::show`] returns at once.

use std::time::{Duration, Instant};

use phosphor_app::surface::layout::{is_deck_port, ControlId, DECK_CHANNEL};
use phosphor_app::surface::lights::{note_of, Level, LIGHTS};

/// How often the port list is read for the deck coming or going.
const RESCAN: Duration = Duration::from_secs(2);

/// How often every light is said again whether it changed or not. The deck
/// lights some things itself as they are pressed; this puts back anything
/// that drifted from what the screen shows.
const REFRESH: Duration = Duration::from_secs(4);

const CLIENT: &str = "phosphor";

pub(crate) struct DeckOut {
    /// The client the port list is read through.
    lister: Option<midir::MidiOutput>,
    /// The open connection, and the port's id.
    connection: Option<(String, midir::MidiOutputConnection)>,
    /// What each light was last set to; `None` when it has not been, or
    /// should be said again.
    sent: [Option<u8>; LIGHTS],
    last_scan: Instant,
    last_refresh: Instant,
}

impl DeckOut {
    pub(crate) fn start() -> Self {
        let lister = midir::MidiOutput::new(CLIENT)
            .map_err(|e| tracing::warn!("MIDI output unavailable: {e}"))
            .ok();
        let mut out = Self {
            lister,
            connection: None,
            sent: [None; LIGHTS],
            last_scan: Instant::now(),
            last_refresh: Instant::now(),
        };
        out.scan();
        out
    }

    /// Called every frame: look for the deck when it is due.
    pub(crate) fn poll(&mut self) {
        if self.last_scan.elapsed() >= RESCAN {
            self.last_scan = Instant::now();
            self.scan();
        }
    }

    fn scan(&mut self) {
        let Some(lister) = self.lister.as_ref() else { return };
        let found = lister
            .ports()
            .iter()
            .find_map(|port| {
                let name = lister.port_name(port).ok()?;
                is_deck_port(&name).then(|| (port.id(), name))
            });
        match (&self.connection, found) {
            (Some((open, _)), Some((id, _))) if *open == id => {}
            (_, Some((id, name))) => {
                self.connection = None;
                let Ok(output) = midir::MidiOutput::new(CLIENT) else { return };
                let Some(port) = output.find_port_by_id(id.clone()) else { return };
                match output.connect(&port, "phosphor-lights") {
                    Ok(connection) => {
                        tracing::info!("deck lights: {name}");
                        crate::debug_log::system(&format!("midi out: opened {name}"));
                        self.connection = Some((id, connection));
                        // A deck that has just appeared knows nothing.
                        self.sent = [None; LIGHTS];
                    }
                    Err(e) => tracing::warn!("deck lights would not open on {name}: {e}"),
                }
            }
            (Some(_), None) => {
                self.connection = None;
                crate::debug_log::system("midi out: deck gone");
            }
            (None, None) => {}
        }
    }

    /// Whether a deck is there to be lit.
    pub(crate) fn connected(&self) -> bool {
        self.connection.is_some()
    }

    /// A control on the deck was touched: the deck may have changed its own
    /// light, so say it again on the next frame.
    pub(crate) fn forget(&mut self, id: ControlId) {
        if let Some(light) = phosphor_app::surface::layout::control(id).light {
            self.sent[usize::from(light)] = None;
        }
    }

    /// Send whatever differs from what the deck was last told.
    pub(crate) fn show(&mut self, levels: &[Level; LIGHTS]) {
        let Some((_, connection)) = self.connection.as_mut() else { return };
        if self.last_refresh.elapsed() >= REFRESH {
            self.last_refresh = Instant::now();
            self.sent = [None; LIGHTS];
        }
        for (i, level) in levels.iter().enumerate() {
            let want = match (*level, self.sent[i]) {
                (Level::Set(v), sent) if sent != Some(v) => Some(v),
                // Handing a light back to the deck: dark first, so a step
                // that was lit does not stay lit over a pad that is a note.
                (Level::Deck, Some(v)) if v != 0 => Some(0),
                _ => None,
            };
            let Some(v) = want else { continue };
            let Some(note) = note_of(i) else { continue };
            if connection.send(&[0x90 | DECK_CHANNEL, note, v]).is_err() {
                // The deck went between scans; the next one notices.
                return;
            }
            self.sent[i] = match level {
                Level::Set(_) => Some(v),
                Level::Deck => Some(0),
            };
        }
    }
}
