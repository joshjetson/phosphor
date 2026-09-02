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
                if n.note > bn.note || (n.note == bn.note && n.start_tick < bn.start_tick) {
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
                    KeyCode::Esc | KeyCode::Char('e') => {
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
                    // Velocity ride: , and . nudge, < and > stride.
                    KeyCode::Char(',') => { self.nudge_velocity(-8); return; }
                    KeyCode::Char('.') => { self.nudge_velocity(8); return; }
                    KeyCode::Char('<') => { self.nudge_velocity(-24); return; }
                    KeyCode::Char('>') => { self.nudge_velocity(24); return; }
                    KeyCode::Char('m') => { self.toggle_note_mute(); return; }
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
                    KeyCode::Char('m') => { self.toggle_note_mute(); return; }
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
                    // The whole selection rides together.
                    KeyCode::Char(',') => { self.nudge_velocity(-8); return; }
                    KeyCode::Char('.') => { self.nudge_velocity(8); return; }
                    KeyCode::Char('<') => { self.nudge_velocity(-24); return; }
                    KeyCode::Char('>') => { self.nudge_velocity(24); return; }
                    KeyCode::Char('m') => { self.toggle_note_mute(); return; }
                    _ => {}
                }
                let step = self
                    .nav
                    .clip_view
                    .piano_roll
                    .grid
                    .step_ticks(phosphor_core::transport::Transport::PPQ);
                if let Some(d) = dir {
                    if shift {
                        // Shift+h/l = stretch right edge, Shift+j/k = stretch left edge
                        // This matches the Right-Left-Trick: shift = right edge
                        match d {
                            Dir::Left => self.stretch_selected_edit_notes(-step, true),
                            Dir::Right => self.stretch_selected_edit_notes(step, true),
                            // Shift+j/k = adjust left edge (start position)
                            Dir::Up => self.stretch_selected_edit_notes(-step, false),
                            Dir::Down => self.stretch_selected_edit_notes(step, false),
                        }
                    } else {
                        // Plain direction = move notes
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
        let col_tick = self.current_cursor_column_tick();
        let col_ticks = self.edit_column_ticks();
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
            if !Self::same_column(n.start_tick, col_tick, col_ticks) { continue; }
            if n.note <= cur_note { continue; }
            if best.map_or(true, |(_, bn)| n.note < bn) {
                best = Some((i, n.note));
            }
        }

        dbg::system(&format!(
            "edit up: col_tick={col_tick} col_ticks={col_ticks} cur_note={cur_note} found={best:?}"
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
        let col_tick = self.current_cursor_column_tick();
        let col_ticks = self.edit_column_ticks();
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
            if !Self::same_column(n.start_tick, col_tick, col_ticks) { continue; }
            if n.note >= cur_note { continue; }
            if best.map_or(true, |(_, bn)| n.note > bn) {
                best = Some((i, n.note));
            }
        }

        dbg::system(&format!(
            "edit down: col_tick={col_tick} col_ticks={col_ticks} cur_note={cur_note} found={best:?}"
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
        let len = self.nav.active_clip().map_or(1, |c| c.length_ticks.max(1)) as f64;
        let cur_tick = notes[cur_idx].start_tick;
        let cur_note_val = notes[cur_idx].note;
        let col_ticks = self.edit_column_ticks();

        // Find the nearest note strictly to the left (different column).
        // Distance stays in clip fractions so the pitch tiebreak keeps the
        // same weight at every clip length.
        let mut best: Option<(usize, f64)> = None;
        for (i, n) in notes.iter().enumerate() {
            if n.start_tick >= cur_tick - col_ticks / 2 { continue; }
            let dx = (cur_tick - n.start_tick) as f64 / len;
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
        let len = self.nav.active_clip().map_or(1, |c| c.length_ticks.max(1)) as f64;
        let cur_tick = notes[cur_idx].start_tick;
        let cur_note_val = notes[cur_idx].note;
        let col_ticks = self.edit_column_ticks();

        let mut best: Option<(usize, f64)> = None;
        for (i, n) in notes.iter().enumerate() {
            if n.start_tick <= cur_tick + col_ticks / 2 { continue; }
            let dx = (n.start_tick - cur_tick) as f64 / len;
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

    /// The start tick of the note at the current edit cursor.
    fn current_cursor_column_tick(&self) -> i64 {
        if let Some(clip) = self.nav.active_clip() {
            let idx = self.nav.clip_view.piano_roll.edit_cursor;
            if let Some(n) = clip.notes.get(idx) {
                return n.start_tick;
            }
        }
        0
    }

    /// Column width in ticks, based on grid resolution.
    fn edit_column_ticks(&self) -> i64 {
        self.nav
            .clip_view
            .piano_roll
            .grid
            .step_ticks(phosphor_core::transport::Transport::PPQ)
    }

    /// Check if two notes are in the same column.
    /// Uses 90% of column width as tolerance to handle imprecise recorded timing.
    fn same_column(tick_a: i64, tick_b: i64, col_ticks: i64) -> bool {
        (tick_a - tick_b).abs() < col_ticks * 9 / 10
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
        let total_beats = pr.total_beats;
        let grid = pr.grid;
        let snap = pr.snap_enabled;

        let mut indices: Vec<usize> = pr.edit_selected.clone();
        if !indices.contains(&pr.edit_cursor) {
            indices.push(pr.edit_cursor);
        }

        let _ = total_beats;
        let ppq = phosphor_core::transport::Transport::PPQ;
        let step = grid.step_ticks(ppq);

        let undo_before = self.checkpoint_viewed_track();

        // Apply the move (grid-step horizontal, snap-aware; semitone vertical)
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks;
            for &idx in &indices {
                if let Some(note) = clip.notes.get_mut(idx) {
                    if grid_steps != 0 {
                        let new_tick = note.start_tick + grid_steps as i64 * step;
                        let new_tick = if snap { grid.snap_ticks(new_tick, ppq) } else { new_tick };
                        note.start_tick = new_tick.clamp(0, (len - note.duration_ticks).max(0));
                    }
                    if semitones != 0 {
                        let new_note = note.note as i32 + semitones;
                        note.note = new_note.clamp(0, 127) as u8;
                    }
                }
            }
        }

        self.commit_viewed_track(undo_before, "move notes");

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

    /// Stretch selected notes' edges. Reuses apply_edge_delta from piano_roll.
    fn stretch_selected_edit_notes(&mut self, delta_ticks: i64, right_edge: bool) {
        use crate::debug_log as dbg;
        let pr = &self.nav.clip_view.piano_roll;

        let mut indices: Vec<usize> = pr.edit_selected.clone();
        if !indices.contains(&pr.edit_cursor) {
            indices.push(pr.edit_cursor);
        }

        let undo_before = self.checkpoint_viewed_track();

        // Apply stretch
        let mut touched = 0usize;
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks;
            for &idx in &indices {
                if let Some(note) = clip.notes.get_mut(idx) {
                    Self::apply_edge_delta(note, delta_ticks, right_edge, len);
                    touched += 1;
                }
            }
        }

        if touched > 0 {
            self.commit_viewed_track(undo_before, "stretch notes");
            self.send_clip_update();
            dbg::system(&format!("edit stretch: edge={} delta={} notes={}",
                if right_edge { "right" } else { "left" }, delta_ticks, indices.len()));
        }
    }

    /// Nudge the velocity of the cursor note — and of the selection, when
    /// one is held — clamped to 1..=127 because velocity zero is a note-off
    /// on the wire and a note that silently stops existing is not a
    /// dynamic. A held ride folds into one undo step.
    pub(crate) fn nudge_velocity(&mut self, delta: i32) {
        let pr = &self.nav.clip_view.piano_roll;
        let mut indices: Vec<usize> = pr.edit_selected.clone();
        if !indices.contains(&pr.edit_cursor) {
            indices.push(pr.edit_cursor);
        }
        let track_idx = match self.nav.clip_view_target {
            Some((ti, _)) => ti,
            None => return,
        };

        let cursor = self.nav.clip_view.piano_roll.edit_cursor;
        let undo_before = self.checkpoint_viewed_track();
        let mut touched = 0usize;
        let mut cursor_vel = 0u8;
        if let Some(clip) = self.nav.active_clip_mut() {
            for &i in &indices {
                if let Some(n) = clip.notes.get_mut(i) {
                    n.velocity = (n.velocity as i32 + delta).clamp(1, 127) as u8;
                    touched += 1;
                }
            }
            if let Some(n) = clip.notes.get(cursor) {
                cursor_vel = n.velocity;
            }
        }
        if touched == 0 {
            return;
        }
        self.commit_viewed_track_coalesced(
            undo_before,
            "velocity",
            crate::state::undo::UndoGesture::Velocity { track_idx },
        );
        self.send_clip_update();
        if touched > 1 {
            self.flash(format!("vel {cursor_vel} \u{00b7} {touched} notes"));
        } else {
            self.flash(format!("vel {cursor_vel}"));
        }
    }

    /// Toggle mute on the selection, or the note under the cursor. A muted
    /// note stays in the clip — visible and editable — but leaves the audio.
    /// The cursor note decides the direction for the whole group, so a mixed
    /// selection settles to one state instead of flapping half-and-half.
    pub(crate) fn toggle_note_mute(&mut self) {
        let pr = &self.nav.clip_view.piano_roll;
        let mut indices: Vec<usize> = pr.edit_selected.clone();
        if !indices.contains(&pr.edit_cursor) {
            indices.push(pr.edit_cursor);
        }
        let cursor = pr.edit_cursor;
        let undo_before = self.checkpoint_viewed_track();
        let mut touched = 0usize;
        let mut now_muted = false;
        if let Some(clip) = self.nav.active_clip_mut() {
            let lead = if cursor < clip.notes.len() { cursor } else { *indices.first().unwrap_or(&0) };
            let Some(target) = clip.notes.get(lead).map(|n| !n.muted) else { return };
            for &i in &indices {
                if let Some(n) = clip.notes.get_mut(i) {
                    n.muted = target;
                    touched += 1;
                }
            }
            now_muted = target;
        }
        if touched == 0 {
            return;
        }
        let label = if now_muted { "mute note" } else { "unmute note" };
        self.commit_viewed_track(undo_before, label);
        self.send_clip_update();
        let word = if now_muted { "muted" } else { "unmuted" };
        if touched > 1 {
            self.flash(format!("{word} \u{00b7} {touched} notes"));
        } else {
            self.flash(word.to_string());
        }
    }

    /// Delete the note at the edit cursor. Pushes undo, syncs audio, kills sound.
    pub(crate) fn edit_delete_cursor_note(&mut self) {
        use crate::debug_log as dbg;
        let undo_before = self.checkpoint_viewed_track();
        let cursor = self.nav.clip_view.piano_roll.edit_cursor;

        if let Some(clip) = self.nav.active_clip_mut() {
            if cursor < clip.notes.len() {
                let removed = clip.notes.remove(cursor);
                dbg::system(&format!("edit delete: removed note {} at tick {}", removed.note, removed.start_tick));

                self.commit_viewed_track(undo_before, "delete note");

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
        let undo_before = self.checkpoint_viewed_track();
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
            self.commit_viewed_track(undo_before, "delete notes");

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
