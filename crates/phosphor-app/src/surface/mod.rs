//! The Phosphor Deck: the hardware control surface, as the app sees it.
//!
//! * [`layout`] — every control, and the MIDI message it sends. The one
//!   source the app, the simulator and the firmware all read.
//! * [`binding`] — what each control means: almost always a key the app
//!   already understands, so the deck obeys every rule the keys do.
//! * [`fader`] — a physical fader against the app's stepped volume, with
//!   pickup so touching one never makes a level jump.
//!
//! Nothing here touches the terminal or the engine. The app's side — taking
//! a message off the wire and acting on it — lives in the TUI's `deck`
//! module, which is the door both the real deck and the simulator knock on.

pub mod binding;
pub mod fader;
pub mod layout;
