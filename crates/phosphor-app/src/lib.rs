//! Shared business logic for Phosphor DAW frontends.
//!
//! This crate contains the application state, data models, navigation logic,
//! undo/redo system, session serialization, and all types needed by both
//! the TUI and GUI frontends. It has no dependency on any rendering framework.

pub mod actions;
pub mod discrete;
pub mod fx;
pub mod paths;
pub mod preset;
pub mod practice;
pub mod progressions;
pub mod sequencer;
pub mod session;
pub mod state;
