//! App methods: piano roll edit mode — note navigation, selection, movement.
//!
//! Navigation (column-locked):
//!   j/k = cycle notes within the current column
//!   h/l = jump to next/prev column that has notes
//! Selection (hold shift):
//!   Shift+dir = start/extend selection (adds notes as you navigate)
//!   Release shift + dir = move selected notes
//! Moving:
//!   h/l = move by grid step, j/k = move by semitone
//!   Esc = lock notes in place, clear selection

use super::*;
use crate::state::{EditSubMode, Pane, ClipViewFocus, ClipTab};

#[derive(Clone, Copy)]
enum Dir { Left, Right, Up, Down }

impl App {
    /// Enter edit mode. Selects the top-left note if any exist.
    pub(crate) fn enter_edit_mode(&mut self) {
        use crate::debug_log as dbg;

        if self.nav.clip_view_target.is_none() {
            self.status_message = Some(("no clip open".into(), std::time::Instant::now()));
            return;
        }

        self.nav.focused_pane = Pane::ClipView;
        self.nav.clip_view.clip_tab = ClipTab::PianoRoll;
        self.nav.clip_view.focus = ClipViewFocus::PianoRoll;

        self.nav.clip_view.piano_roll.edit_mode = true;
        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Navigate;
        self.nav.clip_view.piano_roll.edit_selected.clear();

        // Find top-left note (highest pitch, then earliest start)
        let best = self.nav.active_clip().and_then(|clip| {
            if clip.notes.is_empty() { return None; }
            let mut b = 0usize;
            for (i, n) in clip.notes.iter().enumerate() {
                let bn = &clip.notes[b];
                if n.note > bn.note || (n.note == bn.note && n.start_frac < bn.start_frac) {
                    b = i;
                }
            }
            Some((b, clip.notes[b].note))
        });

        if let Some((idx, note)) = best {
            self.nav.clip_view.piano_roll.edit_cursor = idx;
            self.nav.clip_view.piano_roll.cursor_note = note;
            dbg::system(&format!("edit mode: entered, cursor note={} idx={}", note, idx));
        } else {
            self.nav.clip_view.piano_roll.edit_cursor = 0;
            dbg::system("edit mode: entered (no notes)");
        }

        self.status_message = Some(("edit mode".into(), std::time::Instant::now()));
    }

    pub(crate) fn exit_edit_mode(&mut self) {
        let pr = &mut self.nav.clip_view.piano_roll;
        pr.edit_mode = false;
        pr.edit_selected.clear();
        pr.edit_sub = EditSubMode::Navigate;
        self.status_message = Some(("edit mode off".into(), std::time::Instant::now()));
    }

    /// Handle keys while in edit mode.
    ///
    /// State machine:
    ///   Navigate + Shift+dir → Selecting (add current note, navigate, add destination)
    ///   Selecting + Shift+dir → keep selecting (navigate, add destination)
    ///   Selecting + plain dir → Moving (start moving selected notes)
    ///   Moving + plain dir → keep moving
    ///   Moving/Selecting + Esc → lock notes, clear selection, → Navigate
    ///   Navigate + Esc → exit edit mode
    pub(crate) fn handle_edit_mode_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        use crate::debug_log as dbg;

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        dbg::user(&format!(
            "edit key: {:?} shift={} sub={:?} cursor={} selected={:?}",
            key.code, shift, self.nav.clip_view.piano_roll.edit_sub,
            self.nav.clip_view.piano_roll.edit_cursor,
            self.nav.clip_view.piano_roll.edit_selected,
        ));

        // Normalize direction from key code (works for both Char and arrow keys)
        let dir = match key.code {
            KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Left => Some(Dir::Left),
            KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right => Some(Dir::Right),
            KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => Some(Dir::Down),
            KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => Some(Dir::Up),
            _ => None,
        };

        match self.nav.clip_view.piano_roll.edit_sub {
            EditSubMode::Navigate => {
                match key.code {
                    KeyCode::Esc => {
                        self.exit_edit_mode();
                        return;
                    }
                    KeyCode::Char('d') => {
                        self.edit_delete_cursor_note();
                        return;
                    }
                    KeyCode::Char('u') => {
                        self.perform_undo();
                        return;
                    }
                    // Enter = toggle selection on cursor note (single-note select)
                    KeyCode::Enter => {
                        self.add_cursor_to_selection();
                        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Moving;
                        dbg::system(&format!(
                            "edit: selected cursor note, now {:?}",
                            self.nav.clip_view.piano_roll.edit_selected
                        ));
                        return;
                    }
                    _ => {}
                }
                if let Some(d) = dir {
                    if shift {
                        // Shift+direction: start selecting
                        self.add_cursor_to_selection();
                        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Selecting;
                        self.edit_navigate_dir(d);
                        self.add_cursor_to_selection();
                        dbg::system(&format!(
                            "edit: started selecting, now {:?}",
                            self.nav.clip_view.piano_roll.edit_selected
                        ));
                    } else {
                        self.edit_navigate_dir(d);
                    }
                }
            }
            EditSubMode::Selecting => {
                match key.code {
                    KeyCode::Esc => {
                        self.nav.clip_view.piano_roll.edit_selected.clear();
                        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Navigate;
                        dbg::user("edit: selection cleared");
                        return;
                    }
                    KeyCode::Char('d') => {
                        self.edit_delete_selected_notes();
                        return;
                    }
                    _ => {}
                }
                if let Some(d) = dir {
                    if shift {
                        // Still holding shift: keep selecting
                        self.edit_navigate_dir(d);
                        self.add_cursor_to_selection();
                        dbg::system(&format!(
                            "edit: extended selection, now {:?}",
                            self.nav.clip_view.piano_roll.edit_selected
                        ));
                    } else {
                        // Released shift: transition to Moving, apply first move
                        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Moving;
                        let (gs, st) = Self::dir_to_move(d);
                        self.move_selected_notes(gs, st);
                        dbg::system("edit: → moving");
                    }
                }
            }
            EditSubMode::Moving => {
                match key.code {
                    KeyCode::Esc => {
                        self.nav.clip_view.piano_roll.edit_selected.clear();
                        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Navigate;
                        self.send_clip_update();
                        dbg::user("edit: notes locked");
                        return;
                    }
                    KeyCode::Char('d') => {
                        self.edit_delete_selected_notes();
                        return;
                    }
                    _ => {}
                }
                if let Some(d) = dir {
                    if shift {
                        // Shift while moving: go back to selecting to add more notes
                        self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Selecting;
                        self.edit_navigate_dir(d);
                        self.add_cursor_to_selection();
                        dbg::system("edit: back to selecting from moving");
                    } else {
                        let (gs, st) = Self::dir_to_move(d);
                        self.move_selected_notes(gs, st);
                    }
                }
            }
        }
    }

    /// Convert a direction to (grid_steps, semitones) for move_selected_notes.
    fn dir_to_move(dir: Dir) -> (i32, i32) {
        match dir {
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
            Dir::Up => (0, 1),
            Dir::Down => (0, -1),
        }
    }

    /// Add the current edit_cursor to edit_selected if not already there.
    fn add_cursor_to_selection(&mut self) {
        let pr = &mut self.nav.clip_view.piano_roll;
        let cursor = pr.edit_cursor;
        if !pr.edit_selected.contains(&cursor) {
            pr.edit_selected.push(cursor);
        }
    }

    /// Navigate in the given direction using column-locked rules.
    fn edit_navigate_dir(&mut self, dir: Dir) {
        match dir {
            Dir::Up => self.edit_move_up_in_column(),
            Dir::Down => self.edit_move_down_in_column(),
            Dir::Left => self.edit_move_to_prev_column(),
            Dir::Right => self.edit_move_to_next_column(),
        }
    }

    /// Move cursor UP within the same column (higher pitch).
    pub(crate) fn edit_move_up_in_column(&mut self) {
        use crate::debug_log as dbg;
        let col_frac = self.current_cursor_column_frac();
        let col_w = self.edit_column_width();
        let notes = match self.nav.active_clip() {
            Some(c) => c.notes.clone(),
            None => return,
        };
        let pr = &self.nav.clip_view.piano_roll;
        let cur_idx = pr.edit_cursor;
        if cur_idx >= notes.len() { return; }
        let cur_note = notes[cur_idx].note;

        // Find next higher note in the same column
        let mut best: Option<(usize, u8)> = None;
        for (i, n) in notes.iter().enumerate() {
            if i == cur_idx { continue; }
            if !Self::same_column(n.start_frac, col_frac, col_w) { continue; }
            if n.note <= cur_note { continue; }
            if best.map_or(true, |(_, bn)| n.note < bn) {
                best = Some((i, n.note));
            }
        }

        dbg::system(&format!(
            "edit up: col_frac={:.4} col_w={:.4} cur_note={} found={:?}",
            col_frac, col_w, cur_note, best
        ));

        if let Some((idx, note)) = best {
            let pr = &mut self.nav.clip_view.piano_roll;
            pr.edit_cursor = idx;
            pr.cursor_note = note;
            self.auto_scroll_edit_cursor();
        }
    }

    /// Move cursor DOWN within the same column (lower pitch).
    pub(crate) fn edit_move_down_in_column(&mut self) {
        use crate::debug_log as dbg;
        let col_frac = self.current_cursor_column_frac();
        let col_w = self.edit_column_width();
        let notes = match self.nav.active_clip() {
            Some(c) => c.notes.clone(),
            None => return,
        };
        let pr = &self.nav.clip_view.piano_roll;
        let cur_idx = pr.edit_cursor;
        if cur_idx >= notes.len() { return; }
        let cur_note = notes[cur_idx].note;

        let mut best: Option<(usize, u8)> = None;
        for (i, n) in notes.iter().enumerate() {
            if i == cur_idx { continue; }
            if !Self::same_column(n.start_frac, col_frac, col_w) { continue; }
            if n.note >= cur_note { continue; }
            if best.map_or(true, |(_, bn)| n.note > bn) {
                best = Some((i, n.note));
            }
        }

        dbg::system(&format!(
            "edit down: col_frac={:.4} col_w={:.4} cur_note={} found={:?}",
            col_frac, col_w, cur_note, best
        ));

        if let Some((idx, note)) = best {
            let pr = &mut self.nav.clip_view.piano_roll;
            pr.edit_cursor = idx;
            pr.cursor_note = note;
            self.auto_scroll_edit_cursor();
        }
    }

    /// Move cursor to the nearest note in the previous column (left).
    pub(crate) fn edit_move_to_prev_column(&mut self) {
        let notes = match self.nav.active_clip() {
            Some(c) => c.notes.clone(),
            None => return,
        };
        let pr = &self.nav.clip_view.piano_roll;
        let cur_idx = pr.edit_cursor;
        if cur_idx >= notes.len() { return; }
        let cur_frac = notes[cur_idx].start_frac;
        let cur_note_val = notes[cur_idx].note;
        let col_w = self.edit_column_width();

        // Find the nearest note strictly to the left (different column)
        let mut best: Option<(usize, f64)> = None;
        for (i, n) in notes.iter().enumerate() {
            if n.start_frac >= cur_frac - col_w * 0.5 { continue; }
            let dx = cur_frac - n.start_frac;
            let dy = (n.note as f64 - cur_note_val as f64).abs() * 0.0001;
            let dist = dx + dy;
            if best.map_or(true, |(_, d)| dist < d) {
                best = Some((i, dist));
            }
        }

        if let Some((idx, _)) = best {
            let pr = &mut self.nav.clip_view.piano_roll;
            pr.edit_cursor = idx;
            pr.cursor_note = notes[idx].note;
            self.auto_scroll_edit_cursor();
        }
    }

    /// Move cursor to the nearest note in the next column (right).
    pub(crate) fn edit_move_to_next_column(&mut self) {
        let notes = match self.nav.active_clip() {
            Some(c) => c.notes.clone(),
            None => return,
        };
        let pr = &self.nav.clip_view.piano_roll;
        let cur_idx = pr.edit_cursor;
        if cur_idx >= notes.len() { return; }
        let cur_frac = notes[cur_idx].start_frac;
        let cur_note_val = notes[cur_idx].note;
        let col_w = self.edit_column_width();

        let mut best: Option<(usize, f64)> = None;
        for (i, n) in notes.iter().enumerate() {
            if n.start_frac <= cur_frac + col_w * 0.5 { continue; }
            let dx = n.start_frac - cur_frac;
            let dy = (n.note as f64 - cur_note_val as f64).abs() * 0.0001;
            let dist = dx + dy;
            if best.map_or(true, |(_, d)| dist < d) {
                best = Some((i, dist));
            }
        }

        if let Some((idx, _)) = best {
            let pr = &mut self.nav.clip_view.piano_roll;
            pr.edit_cursor = idx;
            pr.cursor_note = notes[idx].note;
            self.auto_scroll_edit_cursor();
        }
    }

    /// Get the column center frac for the note at the current edit cursor.
    fn current_cursor_column_frac(&self) -> f64 {
        if let Some(clip) = self.nav.active_clip() {
            let idx = self.nav.clip_view.piano_roll.edit_cursor;
            if let Some(n) = clip.notes.get(idx) {
                return n.start_frac;
            }
        }
        0.0
    }

    /// Column width based on grid resolution.
    fn edit_column_width(&self) -> f64 {
        let pr = &self.nav.clip_view.piano_roll;
        pr.grid.step_frac(pr.column_count)
    }

    /// Check if two notes are in the same column.
    /// Uses 90% of column width as tolerance to handle imprecise recorded timing.
    fn same_column(frac_a: f64, frac_b: f64, col_w: f64) -> bool {
        (frac_a - frac_b).abs() < col_w * 0.9
    }

    fn auto_scroll_edit_cursor(&mut self) {
        let pr = &mut self.nav.clip_view.piano_roll;
        let top = pr.view_bottom_note.saturating_add(pr.view_height);
        if pr.cursor_note < pr.view_bottom_note {
            pr.view_bottom_note = pr.cursor_note;
        } else if pr.cursor_note >= top {
            pr.view_bottom_note = pr.cursor_note - pr.view_height + 1;
        }
    }

    /// Move all selected notes by grid steps horizontally and semitones vertically.
    /// Pushes undo on first move, updates audio, scrolls view.
    pub(crate) fn move_selected_notes(&mut self, grid_steps: i32, semitones: i32) {
        use crate::debug_log as dbg;
        let pr = &self.nav.clip_view.piano_roll;
        let total_beats = pr.column_count;
        let grid = pr.grid;
        let snap = pr.snap_enabled;
        let target = self.nav.clip_view_target;

        let mut indices: Vec<usize> = pr.edit_selected.clone();
        if !indices.contains(&pr.edit_cursor) {
            indices.push(pr.edit_cursor);
        }

        let step = grid.step_frac(total_beats);

        // Capture before-state for undo (only on first move of a selection)
        let before: Vec<(usize, phosphor_core::clip::NoteSnapshot)> = if let Some(clip) = self.nav.active_clip() {
            indices.iter()
                .filter_map(|&idx| clip.notes.get(idx).map(|n| (idx, *n)))
                .collect()
        } else {
            Vec::new()
        };

        // Apply the move
        if let Some(clip) = self.nav.active_clip_mut() {
            for &idx in &indices {
                if let Some(note) = clip.notes.get_mut(idx) {
                    if grid_steps != 0 {
                        let new_frac = note.start_frac + grid_steps as f64 * step;
                        note.start_frac = if snap {
                            grid.snap(new_frac, total_beats).clamp(0.0, 1.0 - note.duration_frac)
                        } else {
                            new_frac.clamp(0.0, 1.0 - note.duration_frac)
                        };
                    }
                    if semitones != 0 {
                        let new_note = note.note as i32 + semitones;
                        note.note = new_note.clamp(0, 127) as u8;
                    }
                }
            }
        }

        // Push undo
        if !before.is_empty() {
            if let Some((ti, ci)) = target {
                self.nav.undo_stack.push(crate::state::undo::UndoAction::MoveNotes {
                    track_idx: ti, clip_idx: ci, before,
                });
            }
        }

        // Update the cursor note to track the moved note's new pitch
        let cursor_idx = self.nav.clip_view.piano_roll.edit_cursor;
        if let Some(clip) = self.nav.active_clip() {
            if let Some(note) = clip.notes.get(cursor_idx) {
                self.nav.clip_view.piano_roll.cursor_note = note.note;
            }
        }

        // Scroll view to follow the cursor
        self.auto_scroll_edit_cursor();

        // Sync to audio thread
        self.send_clip_update();
        dbg::system(&format!("edit move: steps={} semi={} notes={}", grid_steps, semitones, indices.len()));
    }

    /// Delete the note at the edit cursor. Pushes undo, syncs audio, kills sound.
    pub(crate) fn edit_delete_cursor_note(&mut self) {
        use crate::debug_log as dbg;
        let target = self.nav.clip_view_target;
        let cursor = self.nav.clip_view.piano_roll.edit_cursor;

        if let Some(clip) = self.nav.active_clip_mut() {
            if cursor < clip.notes.len() {
                let removed = clip.notes.remove(cursor);
                dbg::system(&format!("edit delete: removed note {} at frac {:.4}", removed.note, removed.start_frac));

                // Push undo
                if let Some((ti, ci)) = target {
                    self.nav.undo_stack.push(crate::state::undo::UndoAction::RemoveNote {
                        track_idx: ti, clip_idx: ci, note: removed,
                    });
                }

                // Fix cursor if it's now past the end
                let len = self.nav.active_clip().map(|c| c.notes.len()).unwrap_or(0);
                if len == 0 {
                    self.nav.clip_view.piano_roll.edit_cursor = 0;
                } else if cursor >= len {
                    self.nav.clip_view.piano_roll.edit_cursor = len - 1;
                }

                self.send_clip_update();
                self.engine.panic(); // kill any in-flight note-on
                self.status_message = Some(("note deleted".into(), std::time::Instant::now()));
            }
        }
    }

    /// Delete all selected notes (+ cursor note). Pushes undo, syncs audio.
    fn edit_delete_selected_notes(&mut self) {
        use crate::debug_log as dbg;
        let target = self.nav.clip_view_target;
        let pr = &self.nav.clip_view.piano_roll;
        let mut indices: Vec<usize> = pr.edit_selected.clone();
        if !indices.contains(&pr.edit_cursor) {
            indices.push(pr.edit_cursor);
        }
        // Sort descending so removing by index doesn't shift later indices
        indices.sort_unstable();
        indices.dedup();
        indices.reverse();

        let mut removed_notes = Vec::new();
        if let Some(clip) = self.nav.active_clip_mut() {
            for &idx in &indices {
                if idx < clip.notes.len() {
                    removed_notes.push(clip.notes.remove(idx));
                }
            }
        }

        if !removed_notes.is_empty() {
            let count = removed_notes.len();
            if let Some((ti, ci)) = target {
                self.nav.undo_stack.push(crate::state::undo::UndoAction::DeleteNotes {
                    track_idx: ti, clip_idx: ci, notes: removed_notes,
                });
            }

            // Reset edit state
            let len = self.nav.active_clip().map(|c| c.notes.len()).unwrap_or(0);
            self.nav.clip_view.piano_roll.edit_selected.clear();
            self.nav.clip_view.piano_roll.edit_sub = EditSubMode::Navigate;
            self.nav.clip_view.piano_roll.edit_cursor = if len > 0 { 0 } else { 0 };

            self.send_clip_update();
            self.engine.panic();
            dbg::system(&format!("edit delete: removed {} notes", count));
            self.status_message = Some((
                format!("{} note{} deleted", count, if count == 1 { "" } else { "s" }),
                std::time::Instant::now(),
            ));
        }
    }
}
