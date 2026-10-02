//! MIDI input: every port, as it comes and goes.
//!
//! Phosphor listens to every MIDI input the system has — a keyboard, a pad
//! controller and the Phosphor Deck together — and keeps listening as they
//! are plugged in and pulled out, checking every couple of seconds.
//!
//! Each port gets its own connection and its own callback. What a callback
//! receives is routed by [`phosphor_app::surface::pads::route`]: performances
//! go to the audio engine's ring and to the app's tap, deck controls to the
//! app alone. A message is the deck's only when it arrives on the deck's own
//! port ([`phosphor_app::surface::layout::is_deck_port`]), never because of
//! its channel.
//!
//! On macOS the system's list of MIDI devices reaches a process as
//! notifications on the run loop of the thread that made its first MIDI
//! client. A terminal program never runs that loop, so without help the
//! list it sees is frozen at launch; each scan lets the pending notifications
//! through first ([`pump_system_notifications`]), from the UI thread, which
//! is where the clients are made.
//!
//! The engine's ring has one producer, and with several ports there are
//! several callbacks, possibly on several threads, so the producer sits
//! behind a mutex. That lock is held for one push and is only ever wanted by
//! MIDI callbacks — never by the audio thread, which owns the other end.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use phosphor_app::surface::layout::is_deck_port;
use phosphor_app::surface::pads::{route, PadRoute, Tap};
use phosphor_midi::ring::MidiRingSender;

use crate::app::MidiStatus;

/// How often the port list is read for devices plugged in or pulled out.
const RESCAN: Duration = Duration::from_secs(2);

/// The client name every connection is made under.
const CLIENT: &str = "phosphor";

/// Everything the callbacks share.
#[derive(Clone)]
struct Sinks {
    engine: Arc<Mutex<MidiRingSender>>,
    app: crossbeam_channel::Sender<Tap>,
    pads: Arc<PadRoute>,
    status: Arc<MidiStatus>,
}

/// The open connections, by port id.
pub(crate) struct MidiInputs {
    sinks: Sinks,
    /// The client the port list is read through, kept for the life of the
    /// app so the system keeps it told about devices coming and going.
    lister: Option<midir::MidiInput>,
    open: HashMap<String, (String, midir::MidiInputConnection<()>)>,
    last_scan: Instant,
}

impl MidiInputs {
    /// Connect to every port there is now.
    pub(crate) fn start(
        engine: MidiRingSender,
        app: crossbeam_channel::Sender<Tap>,
        pads: Arc<PadRoute>,
        status: Arc<MidiStatus>,
    ) -> Self {
        let sinks = Sinks { engine: Arc::new(Mutex::new(engine)), app, pads, status };
        let lister = midir::MidiInput::new(CLIENT)
            .map_err(|e| tracing::warn!("MIDI unavailable: {e}"))
            .ok();
        let mut inputs = Self { sinks, lister, open: HashMap::new(), last_scan: Instant::now() };
        inputs.scan();
        inputs
    }

    /// Whether any port is open.
    pub(crate) fn any(&self) -> bool {
        !self.open.is_empty()
    }

    /// Called every frame; reads the port list when it is due. Says what
    /// was plugged in or pulled out, if anything was.
    pub(crate) fn poll(&mut self) -> Option<String> {
        if self.last_scan.elapsed() < RESCAN {
            return None;
        }
        self.last_scan = Instant::now();
        let changes = self.scan();
        (!changes.is_empty()).then(|| changes.join(" \u{00b7} "))
    }

    /// Open the ports that have appeared and close the ones that have gone,
    /// in words.
    fn scan(&mut self) -> Vec<String> {
        let mut changes = Vec::new();
        pump_system_notifications();
        let Some(lister) = self.lister.as_ref() else { return changes };
        let present: Vec<(String, String)> = lister
            .ports()
            .iter()
            .filter_map(|port| Some((port.id(), lister.port_name(port).ok()?)))
            // ALSA's loopback echoes whatever is sent to it; listening to it
            // as well as the device would hear every note twice.
            .filter(|(_, name)| !name.contains("Through"))
            .collect();

        let gone: Vec<String> =
            self.open.keys().filter(|id| !present.iter().any(|(p, _)| p == *id)).cloned().collect();
        for id in gone {
            if let Some((name, connection)) = self.open.remove(&id) {
                connection.close();
                tracing::info!("MIDI port gone: {name}");
                crate::debug_log::system(&format!("midi: closed {name}"));
                changes.push(format!("{name} unplugged"));
            }
        }
        for (id, name) in present {
            if !self.open.contains_key(&id) {
                if let Some(connection) = self.connect(&id, &name) {
                    changes.push(format!("{name} connected"));
                    self.open.insert(id, (name, connection));
                }
            }
        }
        self.sinks.status.connected.store(self.any(), Ordering::Relaxed);
        changes
    }

    fn connect(&self, id: &str, name: &str) -> Option<midir::MidiInputConnection<()>> {
        let input = midir::MidiInput::new(CLIENT).ok()?;
        let port = input.find_port_by_id(id.to_string())?;
        let deck = is_deck_port(name);
        let sinks = self.sinks.clone();
        let result = input.connect(
            &port,
            "phosphor-in",
            move |_timestamp, data, _| {
                let Some(mut message) = phosphor_midi::MidiMessage::from_bytes(data) else { return };
                message.received_micros = Some(phosphor_midi::clock::now_micros());
                if let phosphor_midi::MidiMessageType::NoteOn { note, .. } = message.message_type {
                    sinks.status.last_note.store(note, Ordering::Relaxed);
                }
                sinks.status.message_count.fetch_add(1, Ordering::Relaxed);
                // A deck button is a command, not a note: it goes to the app
                // only, or pressing PLAY would also play an instrument. A deck
                // pad becomes a note here when the pads are notes.
                let routed = route(message, deck, &sinks.pads);
                if let Some(performance) = routed.engine {
                    if let Ok(mut engine) = sinks.engine.lock() {
                        engine.push(performance);
                    }
                }
                // The app's copy. A send that fails means nothing is
                // listening, which is not a reason to stop playing.
                let _ = sinks.app.send(routed.app);
            },
            (),
        );
        match result {
            Ok(connection) => {
                tracing::info!("MIDI connected: {name}{}", if deck { " (the deck)" } else { "" });
                crate::debug_log::system(&format!("midi: opened {name} deck={deck}"));
                Some(connection)
            }
            Err(e) => {
                tracing::warn!("MIDI port {name} would not open: {e}");
                None
            }
        }
    }
}

/// Let the system's pending MIDI notifications through, so the port list
/// is the list of now. See the module notes.
#[cfg(target_os = "macos")]
fn pump_system_notifications() {
    use std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: *const c_void;
        fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    }
    // Zero seconds: handle what is waiting and return at once, so a frame
    // is never held up by it.
    // SAFETY: a CoreFoundation call with the framework's own constant, on
    // the calling thread's run loop, which always exists.
    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.0, 0);
    }
}

/// Elsewhere the port list is always current.
#[cfg(not(target_os = "macos"))]
fn pump_system_notifications() {}
