//! The Phosphor Deck: the hardware control surface, as the app sees it.
//!
//! * [`layout`] — every control, and the MIDI message it sends. The one
//!   source the app, the simulator and the firmware all read.
//! * [`binding`] — what each control means: almost always a key the app
//!   already understands, so the deck obeys every rule the keys do.
//! * [`fader`] — a physical fader against the app's stepped volume, with
//!   pickup so touching one never makes a level jump.
//! * [`screen`] — which screen has the keys, read from the app's own state
//!   every time, so the deck and the keyboard can never disagree about it.
//! * [`actions`] — the eight action buttons' verbs on each screen.
//! * [`pads`] — what a pad hit becomes on its way in: a note for the
//!   instrument, or a step for the app.
//! * [`lights`] — what the deck's forty-two lights show, from the app's
//!   state.
//!
//! Nothing here touches the terminal or the engine. The app's side — taking
//! a message off the wire and acting on it — lives in the TUI's `deck`
//! module, which is the door both the real deck and the simulator knock on.

pub mod actions;
pub mod binding;
pub mod fader;
pub mod layout;
pub mod lights;
pub mod pads;
pub mod screen;
