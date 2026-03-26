//! Undo/redo system — `u` to undo, `r` to redo (vim-style).
//!
//! Each destructive action pushes an UndoAction that captures exactly
//! what was changed so it can be reversed.

use super::TrackState;
use phosphor_core::clip::NoteSnapshot;

/// A single undoable action.
#[derive(Debug, Clone)]
pub enum UndoAction {
    /// Notes were deleted from a clip (undo = add them back).
    DeleteNotes {
        track_idx: usize,
        clip_idx: usize,
        notes: Vec<NoteSnapshot>,
    },
    /// Notes were added to a clip via paste (undo = remove them).
    PasteNotes {
        track_idx: usize,
        clip_idx: usize,
        notes: Vec<NoteSnapshot>,
    },
    /// A note was drawn (added).
    DrawNote {
        track_idx: usize,
        clip_idx: usize,
        note: NoteSnapshot,
    },
    /// A note was toggled off (removed by pressing n on it).
    RemoveNote {
        track_idx: usize,
        clip_idx: usize,
        note: NoteSnapshot,
    },
    /// A clip was deleted.
    DeleteClip {
        track_idx: usize,
        clip_idx: usize,
        clip: super::Clip,
    },
    /// A track was deleted. Stores full track state for restoration.
    DeleteTrack {
        track_idx: usize,
        track: TrackState,
        mixer_id: usize,
    },
}

/// Undo/redo stack.
#[derive(Debug)]
pub struct UndoStack {
    undo: Vec<UndoAction>,
    redo: Vec<UndoAction>,
    max_size: usize,
}

impl Default for UndoStack {
    fn default() -> Self { Self::new() }
}

impl UndoStack {
    pub fn new() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), max_size: 100 }
    }

    /// Push a new action. Clears the redo stack (new timeline branch).
    pub fn push(&mut self, action: UndoAction) {
        self.undo.push(action);
        self.redo.clear();
        if self.undo.len() > self.max_size {
            self.undo.remove(0);
        }
    }

    /// Push to undo stack WITHOUT clearing redo (used during redo operations).
    pub fn push_undo_only(&mut self, action: UndoAction) {
        self.undo.push(action);
        if self.undo.len() > self.max_size {
            self.undo.remove(0);
        }
    }

    /// Pop the last undo action (for undoing). Returns it so caller can reverse it.
    pub fn pop_undo(&mut self) -> Option<UndoAction> {
        self.undo.pop()
    }

    /// Push an action to redo stack (after undoing).
    pub fn push_redo(&mut self, action: UndoAction) {
        self.redo.push(action);
    }

    /// Pop from redo stack (for redoing).
    pub fn pop_redo(&mut self) -> Option<UndoAction> {
        self.redo.pop()
    }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
}
