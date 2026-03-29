//! App methods: piano roll.

use super::*;

impl App {

    /// Draw a new note at the given column and pitch.
    /// Creates a clip if none exists on the track.
    pub(crate) fn draw_note(&mut self, col: usize, note_num: u8) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        let col_w = 1.0 / col_count as f64;
        let start_frac = col as f64 * col_w;
        let duration_frac = col_w;

        // If there's no clip yet, create one on both TUI and audio thread
        if self.nav.active_clip().is_none() {
            let start_tick = self.nav.loop_editor.start_ticks();
            let loop_len = self.nav.loop_editor.end_ticks() - start_tick;
            let length_ticks = if loop_len > 0 { loop_len } else { Transport::PPQ * 4 * 4 };

            if let Some(track) = self.nav.tracks.get_mut(self.nav.track_cursor) {
                let clip_number = track.clips.len() + 1;
                let beats = (length_ticks as f64 / Transport::PPQ as f64).ceil() as u16;
                track.clips.push(crate::state::Clip {
                    number: clip_number,
                    width: beats.max(2),
                    has_content: true,
                    start_tick,
                    length_ticks,
                    notes: Vec::new(),
                    hidden_notes: Vec::new(),
                });
                self.nav.clip_view_target = Some((self.nav.track_cursor, track.clips.len() - 1));

                // Also create the clip on the audio thread
                if let Some(mixer_id) = track.mixer_id {
                    let _ = self.engine.shared.mixer_command_tx.send(
                        MixerCommand::CreateClip {
                            track_id: mixer_id,
                            start_tick,
                            length_ticks,
                        }
                    );
                }
                crate::debug_log::system(&format!("created clip: {} ticks (TUI + audio)", length_ticks));
            }
        }

        // Get track/clip indices for undo
        let target = self.nav.clip_view_target;

        if let Some(clip) = self.nav.active_clip_mut() {
            // Toggle: if a note exists at this position, delete it
            let existing = clip.notes.iter().position(|n| {
                n.note == note_num
                    && (n.start_frac - start_frac).abs() < col_w * 0.5
            });

            if let Some(idx) = existing {
                let removed = clip.notes.remove(idx);
                if let Some((ti, ci)) = target {
                    self.nav.undo_stack.push(UndoAction::RemoveNote {
                        track_idx: ti, clip_idx: ci, note: removed,
                    });
                }
                crate::debug_log::system(&format!("removed note {} at col {}", note_num, col));
            } else {
                let note = phosphor_core::clip::NoteSnapshot {
                    note: note_num, velocity: 100, start_frac, duration_frac,
                };
                clip.notes.push(note.clone());
                if let Some((ti, ci)) = target {
                    self.nav.undo_stack.push(UndoAction::DrawNote {
                        track_idx: ti, clip_idx: ci, note,
                    });
                }
                crate::debug_log::system(&format!("drew note {} at col {}", note_num, col));
            }
        }
    }

    /// Push edited note data from the TUI clip to the audio thread.
    /// Send clip events to audio thread for a specific track/clip (used by undo/redo).

    /// Push edited note data from the TUI clip to the audio thread.
    /// Send clip events to audio thread for a specific track/clip (used by undo/redo).
    pub(crate) fn send_clip_update_for(&self, track_idx: usize, clip_idx: usize) {
        if let Some(track) = self.nav.tracks.get(track_idx) {
            if let (Some(mixer_id), Some(clip)) = (track.mixer_id, track.clips.get(clip_idx)) {
                let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                    &clip.notes, clip.length_ticks,
                );
                let _ = self.engine.shared.mixer_command_tx.send(
                    MixerCommand::UpdateClip {
                        track_id: mixer_id, clip_index: clip_idx, events,
                    }
                );
            }
        }
    }


    pub(crate) fn send_clip_update(&self) {
        use crate::debug_log as dbg;
        if let Some((track_idx, clip_idx)) = self.nav.clip_view_target {
            if let Some(track) = self.nav.tracks.get(track_idx) {
                if let (Some(mixer_id), Some(clip)) = (track.mixer_id, track.clips.get(clip_idx)) {
                    let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                        &clip.notes,
                        clip.length_ticks,
                    );
                    dbg::system(&format!(
                        "send_clip_update: track={} clip={} mixer={} notes={} events={}",
                        track_idx, clip_idx, mixer_id, clip.notes.len(), events.len()
                    ));
                    let _ = self.engine.shared.mixer_command_tx.send(
                        MixerCommand::UpdateClip {
                            track_id: mixer_id,
                            clip_index: clip_idx,
                            events,
                        }
                    );
                } else {
                    dbg::system(&format!(
                        "send_clip_update: track={} clip={} — no mixer_id or clip not found",
                        track_idx, clip_idx
                    ));
                }
            } else {
                dbg::system(&format!("send_clip_update: track {} not found", track_idx));
            }
        } else {
            dbg::system("send_clip_update: no clip_view_target");
        }
    }


    /// Find the first note in a column, searching from cursor. `down` = search lower notes.
    /// Adjust a single note's edge. `right_edge` = true adjusts duration, false adjusts start.

    /// Find the first note in a column, searching from cursor. `down` = search lower notes.
    /// Adjust a single note's edge. `right_edge` = true adjusts duration, false adjusts start.
    pub(crate) fn adjust_note_edge(&mut self, col: usize, note_num: u8, delta: f64, right_edge: bool) {
        let (col_start, col_end) = self.column_frac_range(col);
        if let Some(clip) = self.nav.active_clip_mut() {
            for note in &mut clip.notes {
                if note.note == note_num && note.start_frac >= col_start && note.start_frac < col_end {
                    Self::apply_edge_delta(note, delta, right_edge);
                    return;
                }
            }
        }
    }

    /// Get indices of notes that fall within a column's time range.

    /// Get indices of notes that fall within a column's time range.
    pub(crate) fn note_indices_in_column(&self, col: usize) -> Vec<usize> {
        let (col_start, col_end) = self.column_frac_range(col);
        match self.nav.active_clip() {
            Some(clip) => clip.notes.iter().enumerate()
                .filter(|(_, n)| n.start_frac >= col_start && n.start_frac < col_end)
                .map(|(i, _)| i)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Adjust notes by their stored indices (captured when column was selected).

    /// Adjust notes by their stored indices (captured when column was selected).
    pub(crate) fn adjust_column_edges(&mut self, delta: f64, right_edge: bool) {
        let indices = self.nav.clip_view.piano_roll.selected_note_indices.clone();
        let count = indices.len();
        if let Some(clip) = self.nav.active_clip_mut() {
            for &idx in &indices {
                if let Some(note) = clip.notes.get_mut(idx) {
                    Self::apply_edge_delta(note, delta, right_edge);
                }
            }
        }
        crate::debug_log::system(&format!("adjust {} notes", count));
    }

    /// Get the fractional range [start, end) for a column index.

    /// Get the fractional range [start, end) for a column index.
    pub(crate) fn column_frac_range(&self, col: usize) -> (f64, f64) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        let col_w = 1.0 / col_count as f64;
        (col as f64 * col_w, (col + 1) as f64 * col_w)
    }

    /// Apply a delta to a note's left or right edge.

    /// Apply a delta to a note's left or right edge.
    pub(crate) fn apply_edge_delta(note: &mut phosphor_core::clip::NoteSnapshot, delta: f64, right_edge: bool) {
        if right_edge {
            note.duration_frac = (note.duration_frac + delta).clamp(0.005, 1.0 - note.start_frac);
        } else {
            let end = note.start_frac + note.duration_frac;
            note.start_frac = (note.start_frac + delta).clamp(0.0, end - 0.005);
            note.duration_frac = end - note.start_frac;
        }
    }

    /// If a synth param was just adjusted, send the update to the audio thread.
    /// When the patch selector (index 0) changes, sends ALL params to sync preset.

    /// If a synth param was just adjusted, send the update to the audio thread.
    /// When the patch selector (index 0) changes, sends ALL params to sync preset.
    pub(crate) fn send_synth_param_update(&self) {
        if self.nav.focused_pane != Pane::ClipView
            || self.nav.clip_view.focus != ClipViewFocus::FxPanel
            || self.nav.clip_view.fx_panel_tab != FxPanelTab::Synth
        {
            return;
        }
        let idx = self.nav.clip_view.synth_param_cursor;
        if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
            if let Some(mixer_id) = track.mixer_id {
                if idx == 0 {
                    // Patch changed — send ALL params to audio thread
                    for (i, &val) in track.synth_params.iter().enumerate() {
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetParameter {
                            track_id: mixer_id,
                            param_index: i,
                            value: val,
                        });
                    }
                } else if let Some(&val) = track.synth_params.get(idx) {
                    let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetParameter {
                        track_id: mixer_id,
                        param_index: idx,
                        value: val,
                    });
                }
            }
        }
    }


    // ── (Old column-only methods removed — use delete_selected_notes etc.) ──

    #[allow(dead_code)]
    fn delete_highlighted_notes(&mut self, col_start: usize, col_end: usize) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let col_w = 1.0 / col_count as f64;
        let range_start = col_start as f64 * col_w;
        let range_end = (col_end + 1) as f64 * col_w;

        let target = self.nav.clip_view_target;

        if let Some(clip) = self.nav.active_clip_mut() {
            // Collect notes to remove for undo
            let mut removed_notes = Vec::new();
            let mut kept = Vec::new();
            for n in clip.notes.drain(..) {
                let note_center = n.start_frac + n.duration_frac * 0.5;
                if note_center >= range_start && note_center < range_end {
                    removed_notes.push(n);
                } else {
                    kept.push(n);
                }
            }
            clip.notes = kept;

            if !removed_notes.is_empty() {
                let count = removed_notes.len();
                if let Some((ti, ci)) = target {
                    self.nav.undo_stack.push(UndoAction::DeleteNotes {
                        track_idx: ti, clip_idx: ci, notes: removed_notes,
                    });
                }
                self.status_message = Some((
                    format!("{count} note{} deleted", if count == 1 { "" } else { "s" }),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    // ── Piano roll yank/paste ──


    // ── Piano roll yank/paste ──

    #[allow(dead_code)]
    pub(crate) fn yank_highlighted_notes(&mut self, col_start: usize, col_end: usize) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let col_w = 1.0 / col_count as f64;
        let range_start = col_start as f64 * col_w;
        let range_end = (col_end + 1) as f64 * col_w;

        if let Some(clip) = self.nav.active_clip() {
            // Copy notes in the range, with start_frac made relative to range_start
            let mut yanked = Vec::new();
            for n in &clip.notes {
                let note_center = n.start_frac + n.duration_frac * 0.5;
                if note_center >= range_start && note_center < range_end {
                    let mut copied = *n;
                    copied.start_frac -= range_start; // make relative to yank origin
                    yanked.push(copied);
                }
            }
            let num_cols = col_end - col_start + 1;
            self.nav.clip_view.piano_roll.yank_buffer = yanked.clone();
            self.nav.clip_view.piano_roll.yank_columns = num_cols;

            self.status_message = Some((
                format!("{} note{} yanked from {} col{}",
                    yanked.len(), if yanked.len() == 1 { "" } else { "s" },
                    num_cols, if num_cols == 1 { "" } else { "s" }),
                std::time::Instant::now(),
            ));
        }
    }


    #[allow(dead_code)]
    pub(crate) fn paste_yanked_notes(&mut self, paste_col: usize) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let col_w = 1.0 / col_count as f64;
        let paste_start = paste_col as f64 * col_w;

        let yank_buf = self.nav.clip_view.piano_roll.yank_buffer.clone();
        if yank_buf.is_empty() {
            self.status_message = Some(("nothing to paste".into(), std::time::Instant::now()));
            return;
        }

        // Get the highlight width for clipping (if highlighted), otherwise use yank_columns
        let max_paste_cols = if let Some((hl_start, hl_end)) = self.nav.clip_view.piano_roll.highlight_range() {
            hl_end - hl_start + 1
        } else {
            self.nav.clip_view.piano_roll.yank_columns
        };
        let max_paste_frac = max_paste_cols as f64 * col_w;

        let target = self.nav.clip_view_target;
        let mut pasted_count = 0;

        if let Some(clip) = self.nav.active_clip_mut() {
            for n in &yank_buf {
                // Only paste notes that fit within the paste region
                if n.start_frac + n.duration_frac <= max_paste_frac {
                    let mut pasted = *n;
                    pasted.start_frac += paste_start; // offset to paste position
                    // Clamp to clip bounds
                    if pasted.start_frac + pasted.duration_frac <= 1.0 {
                        clip.notes.push(pasted);
                        pasted_count += 1;
                    }
                }
            }
        }

        if pasted_count > 0 {
            // Push undo for the paste (as a bulk draw)
            if let (Some((ti, ci)), Some(clip)) = (target, self.nav.active_clip()) {
                // Collect the pasted notes (they're the last N notes in the clip)
                let pasted_notes: Vec<_> = clip.notes[clip.notes.len() - pasted_count..].to_vec();
                self.nav.undo_stack.push(UndoAction::DeleteNotes {
                    track_idx: ti, clip_idx: ci, notes: pasted_notes,
                });
                // Note: we store as DeleteNotes so undo removes them
            }
        }

        self.status_message = Some((
            format!("{pasted_count} note{} pasted", if pasted_count == 1 { "" } else { "s" }),
            std::time::Instant::now(),
        ));
    }

    // ── Combined column+row selection operations ──

    /// Delete notes matching the selected columns and/or rows.
    pub(crate) fn delete_selected_notes(
        &mut self,
        col_range: Option<(usize, usize)>,
        row_range: Option<(u8, u8)>,
    ) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let col_w = 1.0 / col_count as f64;
        let target = self.nav.clip_view_target;

        if let Some(clip) = self.nav.active_clip_mut() {
            let mut removed = Vec::new();
            let mut kept = Vec::new();

            for n in clip.notes.drain(..) {
                let note_center = n.start_frac + n.duration_frac * 0.5;
                let in_col = col_range.map_or(true, |(cs, ce)| {
                    let range_start = cs as f64 * col_w;
                    let range_end = (ce + 1) as f64 * col_w;
                    note_center >= range_start && note_center < range_end
                });
                let in_row = row_range.map_or(true, |(lo, hi)| {
                    n.note >= lo && n.note <= hi
                });
                if in_col && in_row {
                    removed.push(n);
                } else {
                    kept.push(n);
                }
            }
            clip.notes = kept;

            if !removed.is_empty() {
                let count = removed.len();
                if let Some((ti, ci)) = target {
                    self.nav.undo_stack.push(UndoAction::DeleteNotes {
                        track_idx: ti, clip_idx: ci, notes: removed,
                    });
                }
                self.status_message = Some((
                    format!("{count} note{} deleted", if count == 1 { "" } else { "s" }),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    /// Yank notes matching the selected columns and/or rows.
    pub(crate) fn yank_selected_notes(
        &mut self,
        col_range: Option<(usize, usize)>,
        row_range: Option<(u8, u8)>,
    ) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let col_w = 1.0 / col_count as f64;
        let col_start_frac = col_range.map_or(0.0, |(cs, _)| cs as f64 * col_w);
        let _row_base = row_range.map_or(self.nav.clip_view.piano_roll.cursor_note, |(lo, _)| lo);

        if let Some(clip) = self.nav.active_clip() {
            let mut yanked = Vec::new();
            for n in &clip.notes {
                let note_center = n.start_frac + n.duration_frac * 0.5;
                let in_col = col_range.map_or(true, |(cs, ce)| {
                    let rs = cs as f64 * col_w;
                    let re = (ce + 1) as f64 * col_w;
                    note_center >= rs && note_center < re
                });
                let in_row = row_range.map_or(true, |(lo, hi)| {
                    n.note >= lo && n.note <= hi
                });
                if in_col && in_row {
                    let mut copied = *n;
                    copied.start_frac -= col_start_frac;
                    yanked.push(copied);
                }
            }
            let num_cols = col_range.map_or(col_count, |(cs, ce)| ce - cs + 1);
            self.nav.clip_view.piano_roll.yank_buffer = yanked.clone();
            self.nav.clip_view.piano_roll.yank_columns = num_cols;

            self.status_message = Some((
                format!("{} note{} yanked", yanked.len(), if yanked.len() == 1 { "" } else { "s" }),
                std::time::Instant::now(),
            ));
        }
    }

    /// Paste yanked notes at the given column, with optional row offset.
    /// Row offset shifts notes vertically: positive = up, negative = down.
    pub(crate) fn paste_selected_notes(&mut self, paste_col: usize, row_offset: Option<i16>) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let col_w = 1.0 / col_count as f64;
        let paste_start = paste_col as f64 * col_w;
        let note_shift = row_offset.unwrap_or(0);

        let yank_buf = self.nav.clip_view.piano_roll.yank_buffer.clone();
        if yank_buf.is_empty() {
            self.status_message = Some(("nothing to paste".into(), std::time::Instant::now()));
            return;
        }

        // Capture target before mutable borrow
        let target = self.nav.clip_view_target;
        let mut pasted_notes = Vec::new();

        if let Some(clip) = self.nav.active_clip_mut() {
            for n in &yank_buf {
                let new_note = (n.note as i16 + note_shift).clamp(0, 127) as u8;
                let new_start = n.start_frac + paste_start;
                if new_start + n.duration_frac <= 1.0 {
                    let pasted = phosphor_core::clip::NoteSnapshot {
                        note: new_note,
                        velocity: n.velocity,
                        start_frac: new_start,
                        duration_frac: n.duration_frac,
                    };
                    clip.notes.push(pasted);
                    pasted_notes.push(pasted);
                }
            }
        }

        // Push undo AFTER the mutable borrow ends
        if !pasted_notes.is_empty() {
            let count = pasted_notes.len();
            if let Some((ti, ci)) = target {
                // Undo a paste = remove the pasted notes
                self.nav.undo_stack.push(UndoAction::PasteNotes {
                    track_idx: ti, clip_idx: ci, notes: pasted_notes,
                });
            }
            self.status_message = Some((
                format!("{count} note{} pasted (u to undo)", if count == 1 { "" } else { "s" }),
                std::time::Instant::now(),
            ));
        } else {
            self.status_message = Some(("nothing to paste".into(), std::time::Instant::now()));
        }
    }
}
