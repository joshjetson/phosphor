//! Step record: what is played goes into the pattern.
//!
//! # Where the notes come from
//!
//! MIDI reaches the audio thread through a lock-free ring that has exactly
//! one consumer, so the UI cannot read it — it would be taking notes out of
//! the instrument's mouth. What it gets instead is a tap: the same `midir`
//! callback that fills the ring also drops note-ons and note-offs into a
//! channel that this drains once a frame. The audio path is untouched, and a
//! frame of latency does not matter for something written into a step rather
//! than played.
//!
//! # One gesture, one step
//!
//! Notes accumulate while any key is down and are written when the last one
//! comes back up. That is what makes chords work without a timer: hold three
//! keys, get a chord in one step; play one note, get one note. The step is
//! written once per gesture and the cursor moves on once, so a four-finger
//! chord does not walk four steps forward.

use super::*;

use phosphor_app::sequencer::ops::{HeldNotes, SeqOp};
use phosphor_app::sequencer::SequencerState;
use phosphor_midi::MidiMessageType;

impl App {
    /// Take everything the MIDI tap has seen since the last frame.
    pub(crate) fn poll_step_record(&mut self) {
        let Some(rx) = self.midi_ui_rx.as_ref() else { return };
        let mut events: Vec<MidiMessageType> = Vec::new();
        while let Ok(message) = rx.try_recv() {
            events.push(message.message_type);
        }
        for event in events {
            match event {
                // A note-on with no velocity is a note-off; every controller
                // that runs notes together sends them that way.
                MidiMessageType::NoteOn { note, velocity: 0, .. }
                | MidiMessageType::NoteOff { note, .. } => self.step_record_note_off(note),
                MidiMessageType::NoteOn { note, .. } => {
                    self.observe_note_for_recording_undo();
                    self.step_record_note_on(note);
                }
                _ => {}
            }
        }
    }

    /// One more note the recorder may be holding — see
    /// [`App::live_take_notes`]. The tap sees the same stream the recorder
    /// does, so counting here is how undo knows an in-flight pass exists
    /// without asking the audio thread.
    pub(crate) fn observe_note_for_recording_undo(&mut self) {
        let transport = &self.engine.transport;
        if transport.is_recording()
            && transport.is_playing()
            && self.nav.tracks.iter().any(|t| t.armed && t.is_live())
        {
            self.live_take_notes += 1;
        }
    }

    /// Whether the track under the cursor is waiting to be played into.
    fn step_record_armed(&self) -> bool {
        self.nav
            .current_track()
            .and_then(|t| t.sequencer.as_deref())
            .is_some_and(SequencerState::is_step_recording)
    }

    pub(crate) fn step_record_note_on(&mut self, note: u8) {
        if !self.step_record_armed() {
            return;
        }
        if !self.held_notes.contains(&note) {
            self.held_notes.push(note);
        }
        if !self.recorded_notes.contains(&note) {
            self.recorded_notes.push(note);
        }
    }

    pub(crate) fn step_record_note_off(&mut self, note: u8) {
        self.held_notes.retain(|&held| held != note);
        if !self.held_notes.is_empty() || self.recorded_notes.is_empty() {
            return;
        }
        if !self.step_record_armed() {
            self.recorded_notes.clear();
            return;
        }
        let played = std::mem::take(&mut self.recorded_notes);
        self.sequencer_op(SeqOp::RecordNotes(HeldNotes::new(&played)));
    }
}
