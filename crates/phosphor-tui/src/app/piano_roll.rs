//! App methods: piano roll.

use super::*;

/// Whether a note belongs to the highlighted column/row region — the one
/// membership rule every column/row operation shares. Column membership is
/// judged by the note's centre, in ticks, with doubled arithmetic so the
/// half-tick centre stays exact.
fn note_in_selection(
    n: &phosphor_core::clip::NoteSnapshot,
    length_ticks: i64,
    col_range: Option<(usize, usize)>,
    row_range: Option<(u8, u8)>,
    col_count: usize,
) -> bool {
    let center2 = 2 * n.start_tick + n.duration_ticks;
    let in_col = col_range.is_none_or(|(cs, ce)| {
        let rs2 = 2 * (cs as i64 * length_ticks) / col_count.max(1) as i64;
        let re2 = 2 * ((ce + 1) as i64 * length_ticks) / col_count.max(1) as i64;
        center2 >= rs2 && center2 < re2
    });
    let in_row = row_range.is_none_or(|(lo, hi)| n.note >= lo && n.note <= hi);
    in_col && in_row
}

impl App {

    /// Draw a new note at the given column and pitch.
    /// Creates a clip if none exists on the track.
    pub(crate) fn draw_note(&mut self, col: usize, note_num: u8) {
        let pr = &self.nav.clip_view.piano_roll;
        let col_count = pr.column_count;
        let total_beats = pr.total_beats;
        let snap = pr.snap_enabled;
        let grid = pr.grid;
        let velocity = pr.default_velocity;
        let _ = total_beats;

        // Checkpointed before the implicit clip below, so that undoing the
        // first note drawn on an empty track takes the clip it conjured with
        // it rather than leaving an empty box on the timeline.
        let undo_track = self
            .nav
            .clip_view_target
            .map(|(ti, _)| ti)
            .unwrap_or(self.nav.track_cursor);
        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackClips { track_idx: undo_track },
        );

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
                    controls: Vec::new(),
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

                // Say so. Drawing the first note on an empty track makes a
                // clip out of nothing, and the only other sign of it is a
                // block appearing in a pane the player is not looking at.
                let bars = (length_ticks as f64 / (Transport::PPQ * 4) as f64).ceil() as i64;
                self.status_message = Some((
                    format!(
                        "clip {clip_number} created \u{00B7} {bars} bar{} from bar {}",
                        if bars == 1 { "" } else { "s" },
                        start_tick / (Transport::PPQ * 4) + 1,
                    ),
                    std::time::Instant::now(),
                ));
            }
        }

        let mut label = "draw note";
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks.max(1);
            let ppq = Transport::PPQ;
            let col_w_ticks = (len / col_count.max(1) as i64).max(1);
            let duration_ticks = grid.step_ticks(ppq);
            // Clamp the start to keep the note inside the clip. Without the
            // clamp, drawing at the last column with snap on can push the
            // start to the clip's end, which renders invisibly while still
            // playing. Invariant: start + duration <= length.
            let raw_start = col as i64 * col_w_ticks;
            let raw_start = if snap { grid.snap_ticks(raw_start, ppq) } else { raw_start };
            let start_tick = raw_start.clamp(0, (len - duration_ticks).max(0));

            // Toggle: if a note exists at this position, delete it
            let existing = clip.notes.iter().position(|n| {
                n.note == note_num
                    && (n.start_tick - start_tick).abs() < col_w_ticks / 2
            });

            if let Some(idx) = existing {
                clip.notes.remove(idx);
                label = "remove note";
                crate::debug_log::system(&format!("removed note {} at col {}", note_num, col));
            } else {
                clip.notes.push(phosphor_core::clip::NoteSnapshot {
                    note: note_num, velocity, start_tick, duration_ticks, muted: false,
                });
                crate::debug_log::system(&format!("drew note {} at col {}", note_num, col));
            }
        }
        self.nav.commit_undo(undo_before, label);
    }

    // ── Vertical navigation: snap between note-bearing pitches ──

    /// Move the cursor to the nearest pitch above it that has a note, so
    /// editing a note never means scrolling to find it. Lands on the row,
    /// not the note — h/l and n then work it. A no-op, said out loud, when
    /// there is no note higher up.
    pub(crate) fn snap_note_up(&mut self) {
        self.snap_note(true);
    }

    pub(crate) fn snap_note_down(&mut self) {
        self.snap_note(false);
    }

    fn snap_note(&mut self, up: bool) {
        let cursor = self.nav.clip_view.piano_roll.cursor_note;
        let Some(clip) = self.nav.active_clip() else { return };
        let target = if up {
            clip.notes.iter().map(|n| n.note).filter(|&p| p > cursor).min()
        } else {
            clip.notes.iter().map(|n| n.note).filter(|&p| p < cursor).max()
        };
        match target {
            Some(note) => self.nav.clip_view.piano_roll.cursor_to_note(note),
            None => self.flash(if up { "no note higher" } else { "no note lower" }),
        }
    }

    /// Put the column cursor where the playhead is — the column the music
    /// is on right now, which is where the note that just sounded wrong
    /// lives — and the pitch cursor on a note in that column when one
    /// exists. Falls back to the clip's start when the playhead is outside
    /// the clip.
    pub(crate) fn jump_to_playhead(&mut self) {
        let Some(clip) = self.nav.active_clip() else { return };
        if clip.length_ticks <= 0 {
            return;
        }
        let pos = self.engine.transport.position_ticks() - clip.start_tick;
        let col_count = self.nav.clip_view.piano_roll.column_count.max(1);
        let col = if pos >= 0 && pos < clip.length_ticks {
            ((pos * col_count as i64) / clip.length_ticks) as usize
        } else {
            0
        };
        // A note sounding in this column, nearest the cursor pitch, if any.
        let col_lo = (col as i64 * clip.length_ticks) / col_count as i64;
        let col_hi = ((col + 1) as i64 * clip.length_ticks) / col_count as i64;
        let cursor_pitch = self.nav.clip_view.piano_roll.cursor_note;
        let target = clip
            .notes
            .iter()
            .filter(|n| n.start_tick < col_hi && n.end_tick() > col_lo)
            .min_by_key(|n| (n.note as i16 - cursor_pitch as i16).abs())
            .map(|n| n.note);

        let pr = &mut self.nav.clip_view.piano_roll;
        pr.column = col.min(col_count - 1);
        pr.ensure_column_visible();
        if let Some(note) = target {
            pr.cursor_to_note(note);
            self.flash("at the playhead");
        } else {
            self.flash("at the playhead \u{00b7} no note here");
        }
    }

    // ── Automation lane ──

    /// The controller streams the viewed clip offers, or empty when no clip
    /// is in view.
    pub(crate) fn automation_streams(&self) -> Vec<crate::state::AutomationStream> {
        self.nav.active_clip().map(|c| c.control_streams()).unwrap_or_default()
    }

    /// The stream the lane is currently pointed at, its index clamped to
    /// what the clip offers.
    pub(crate) fn current_automation_stream(&self) -> Option<crate::state::AutomationStream> {
        let streams = self.automation_streams();
        if streams.is_empty() {
            return None;
        }
        let idx = self.nav.clip_view.piano_roll.automation_lane.min(streams.len() - 1);
        Some(streams[idx])
    }

    /// Open the lane and give it the keys, or (if already focused) hand the
    /// keys back to the note grid. Toggled with `A` from the piano roll.
    pub(crate) fn toggle_automation_lane(&mut self) {
        let pr = &mut self.nav.clip_view.piano_roll;
        if pr.automation_open && pr.automation_focus {
            pr.automation_focus = false;
            self.flash("automation: note grid has the keys");
        } else {
            pr.automation_open = true;
            pr.automation_focus = true;
            let label = self
                .current_automation_stream()
                .map(|s| s.label())
                .unwrap_or_else(|| "mod".to_string());
            self.flash(format!("automation: {label} \u{00b7} jk draw, r ramp, [ ] lane, d clear, A note grid"));
        }
    }

    /// Close the lane entirely (Esc from a focused lane).
    pub(crate) fn close_automation_lane(&mut self) {
        let pr = &mut self.nav.clip_view.piano_roll;
        pr.automation_open = false;
        pr.automation_focus = false;
    }

    /// Point the lane at the next or previous controller stream.
    pub(crate) fn automation_cycle_stream(&mut self, delta: i32) {
        let count = self.automation_streams().len();
        if count == 0 {
            return;
        }
        let pr = &mut self.nav.clip_view.piano_roll;
        let cur = pr.automation_lane.min(count - 1) as i32;
        pr.automation_lane = (cur + delta).rem_euclid(count as i32) as usize;
        if let Some(stream) = self.current_automation_stream() {
            self.flash(format!("automation lane: {}", stream.label()));
        }
    }

    /// Raise or lower the curve at the cursor column, drawing a point there.
    /// A sweep of these — held, or walked across columns — folds into one
    /// undo step; the value carries between columns so a ramp is just h then
    /// k, k, k.
    pub(crate) fn automation_draw(&mut self, delta: i32) {
        let Some(stream) = self.current_automation_stream() else { return };
        let (col, col_count) = {
            let pr = &self.nav.clip_view.piano_roll;
            (pr.column, pr.column_count.max(1))
        };
        let track_idx = match self.nav.clip_view_target {
            Some((ti, _)) => ti,
            None => return,
        };

        // Start from what this column already holds — which, because a value
        // holds until the next event, is the value carried from the last
        // column drawn. An untouched lane reads zero, so a ramp is built by
        // walking right and pressing up.
        let current = self
            .nav
            .active_clip()
            .and_then(|c| c.control_value_at_column(stream, col, col_count))
            .unwrap_or(0);
        let value = (current as i32 + delta).clamp(0, 127) as u8;

        let undo_before = self.checkpoint_viewed_track();
        if let Some(clip) = self.nav.active_clip_mut() {
            clip.set_control_point(stream, col, col_count, value);
        }
        self.commit_viewed_track_coalesced(
            undo_before,
            "automation",
            crate::state::undo::UndoGesture::Automation { track_idx },
        );
        self.send_clip_update();
    }


    /// Pull a straight line back from the cursor column to the stream's
    /// previous point. The far end is drawn first with jk; r fills the gap.
    /// One undo step — a ramp is one deliberate action, not a sweep.
    pub(crate) fn automation_ramp(&mut self) {
        let Some(stream) = self.current_automation_stream() else { return };
        let (col, col_count) = {
            let pr = &self.nav.clip_view.piano_roll;
            (pr.column, pr.column_count.max(1))
        };
        let undo_before = self.checkpoint_viewed_track();
        let written = self
            .nav
            .active_clip_mut()
            .and_then(|c| c.ramp_control_to(stream, col, col_count));
        match written {
            None => self.flash("ramp needs a point here and one earlier \u{00b7} draw with jk"),
            Some(0) => self.flash("nothing between the points to fill"),
            Some(n) => {
                self.commit_viewed_track(undo_before, "ramp automation");
                self.send_clip_update();
                self.flash(format!("ramp \u{00b7} {n} points"));
            }
        }
    }

    /// Remove the stream's point in the cursor column.
    pub(crate) fn automation_clear_point(&mut self) {
        let Some(stream) = self.current_automation_stream() else { return };
        let (col, col_count) = {
            let pr = &self.nav.clip_view.piano_roll;
            (pr.column, pr.column_count.max(1))
        };
        let undo_before = self.checkpoint_viewed_track();
        let cleared = self
            .nav
            .active_clip_mut()
            .map(|c| c.clear_control_point(stream, col, col_count))
            .unwrap_or(false);
        if cleared {
            self.commit_viewed_track(undo_before, "clear automation point");
            self.send_clip_update();
        }
    }

    /// Wipe the viewed clip's recorded controllers — the one eraser for a
    /// flubbed wheel sweep until automation lanes give them a face. Undoable
    /// like any clip edit, and honest about what it found.
    pub(crate) fn clear_clip_controls(&mut self) {
        let undo_before = self.checkpoint_viewed_track();
        let count = match self.nav.active_clip_mut() {
            Some(clip) => {
                let count = clip.controls.len();
                clip.controls.clear();
                count
            }
            None => 0,
        };
        if count == 0 {
            self.flash("no controller data in this clip");
            return;
        }
        self.commit_viewed_track(undo_before, "clear controllers");
        self.send_clip_update();
        self.flash(format!(
            "cleared {count} controller event{} (u to undo)",
            if count == 1 { "" } else { "s" }
        ));
    }

    pub(crate) fn send_clip_update(&self) {
        use crate::debug_log as dbg;
        if let Some((track_idx, clip_idx)) = self.nav.clip_view_target {
            if let Some(track) = self.nav.tracks.get(track_idx) {
                if let (Some(mixer_id), Some(clip)) = (track.mixer_id, track.clips.get(clip_idx)) {
                    let events = clip.events_for_audio();
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


    /// Adjust a single note's edge. `right_edge` = true adjusts duration, false adjusts start.
    pub(crate) fn adjust_note_edge(&mut self, col: usize, note_num: u8, delta_ticks: i64, right_edge: bool) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if let Some(clip) = self.nav.active_clip_mut() {
            let (col_start, col_end) = Self::column_tick_range(col, col_count, clip.length_ticks);
            let len = clip.length_ticks;
            for note in &mut clip.notes {
                if note.note == note_num && note.start_tick >= col_start && note.start_tick < col_end {
                    Self::apply_edge_delta(note, delta_ticks, right_edge, len);
                    return;
                }
            }
        }
    }

    /// Get indices of notes that fall within a column's time range.
    pub(crate) fn note_indices_in_column(&self, col: usize) -> Vec<usize> {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        match self.nav.active_clip() {
            Some(clip) => {
                let (col_start, col_end) = Self::column_tick_range(col, col_count, clip.length_ticks);
                clip.notes.iter().enumerate()
                    .filter(|(_, n)| n.start_tick >= col_start && n.start_tick < col_end)
                    .map(|(i, _)| i)
                    .collect()
            }
            None => Vec::new(),
        }
    }

    /// Adjust notes by their stored indices (captured when column was selected).
    pub(crate) fn adjust_column_edges(&mut self, delta_ticks: i64, right_edge: bool) {
        let indices = self.nav.clip_view.piano_roll.selected_note_indices.clone();
        let count = indices.len();
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks;
            for &idx in &indices {
                if let Some(note) = clip.notes.get_mut(idx) {
                    Self::apply_edge_delta(note, delta_ticks, right_edge, len);
                }
            }
        }
        crate::debug_log::system(&format!("adjust {} notes", count));
    }

    /// The tick range [start, end) a column covers in a clip.
    pub(crate) fn column_tick_range(col: usize, col_count: usize, length_ticks: i64) -> (i64, i64) {
        let cc = col_count.max(1) as i64;
        ((col as i64 * length_ticks) / cc, ((col + 1) as i64 * length_ticks) / cc)
    }

    /// Apply a delta to a note's left or right edge. The floor is a 64th
    /// note — the same minimum the recorder keeps — so an edge drag can
    /// shorten a note to a tick of colour but never to nothing.
    pub(crate) fn apply_edge_delta(
        note: &mut phosphor_core::clip::NoteSnapshot,
        delta_ticks: i64,
        right_edge: bool,
        length_ticks: i64,
    ) {
        let min = Transport::PPQ / 16;
        if right_edge {
            let room = (length_ticks - note.start_tick).max(min);
            note.duration_ticks = (note.duration_ticks + delta_ticks).clamp(min, room);
        } else {
            let end = note.end_tick();
            note.start_tick = (note.start_tick + delta_ticks).clamp(0, (end - min).max(0));
            note.duration_ticks = end - note.start_tick;
        }
    }

    /// If a synth param was just adjusted, send the update to the audio thread.
    /// When the patch selector (index 0) changes, sends ALL params to sync preset.
    pub(crate) fn send_synth_param_update(&self) {
        // Either view of the instrument's panel: the narrow strip on the left
        // and the full one in the `[inst]` tab are the same controls on the
        // same cursor, so a knob turned in either has to reach the audio
        // thread. Guarding on the left one alone is why the tab looked like
        // it worked and sounded like it did not.
        let on_panel = match self.nav.clip_view.focus {
            ClipViewFocus::FxPanel => self.nav.clip_view.fx_panel_tab == FxPanelTab::Synth,
            ClipViewFocus::PianoRoll => self.nav.clip_view.clip_tab == ClipTab::InstConfig,
        };
        if self.nav.focused_pane != Pane::ClipView || !on_panel {
            return;
        }
        let idx = self.nav.clip_view.synth_param_cursor;
        if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
            if let Some(mixer_id) = track.mixer_id {
                // A patch selector reloads the whole block, so the whole
                // block goes. The Prophet-6 and the TEO-5 keep their preset
                // in two controls — a bank and a program — and moving either
                // one reloads it, which is what `NavState::adjust_synth_param`
                // does and what this has to match or half a patch arrives.
                let reloaded = idx == 0
                    || (track.instrument_type == Some(InstrumentType::Prophet6)
                        && idx == phosphor_dsp::prophet6::P_BANK)
                    || (track.instrument_type == Some(InstrumentType::Teo5)
                        && idx == phosphor_dsp::teo5::P_BANK);
                if reloaded {
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


    // ── Combined column+row selection operations ──

    /// Delete notes matching the selected columns and/or rows.
    pub(crate) fn delete_selected_notes(
        &mut self,
        col_range: Option<(usize, usize)>,
        row_range: Option<(u8, u8)>,
    ) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let undo_before = self.checkpoint_viewed_track();

        if let Some(clip) = self.nav.active_clip_mut() {
            let mut removed = Vec::new();
            let mut kept = Vec::new();
            let len = clip.length_ticks;

            for n in clip.notes.drain(..) {
                if note_in_selection(&n, len, col_range, row_range, col_count) {
                    removed.push(n);
                } else {
                    kept.push(n);
                }
            }
            clip.notes = kept;

            if !removed.is_empty() {
                let count = removed.len();
                // Invalidate any stale note indices (column-selected mode)
                self.nav.clip_view.piano_roll.selected_note_indices.clear();
                self.status_message = Some((
                    format!("{count} note{} deleted", if count == 1 { "" } else { "s" }),
                    std::time::Instant::now(),
                ));
            }
        }
        self.commit_viewed_track(undo_before, "delete notes");
    }

    /// Yank notes matching the selected columns and/or rows.
    pub(crate) fn yank_selected_notes(
        &mut self,
        col_range: Option<(usize, usize)>,
        row_range: Option<(u8, u8)>,
    ) {
        let col_count = self.nav.clip_view.piano_roll.column_count;
        if col_count == 0 { return; }
        let _row_base = row_range.map_or(self.nav.clip_view.piano_roll.cursor_note, |(lo, _)| lo);

        if let Some(clip) = self.nav.active_clip() {
            let len = clip.length_ticks;
            let col_start_tick =
                col_range.map_or(0, |(cs, _)| (cs as i64 * len) / col_count as i64);
            let mut yanked = Vec::new();
            for n in &clip.notes {
                if note_in_selection(n, len, col_range, row_range, col_count) {
                    let mut copied = *n;
                    copied.start_tick -= col_start_tick;
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
        let note_shift = row_offset.unwrap_or(0);

        let yank_buf = self.nav.clip_view.piano_roll.yank_buffer.clone();
        if yank_buf.is_empty() {
            self.status_message = Some(("nothing to paste".into(), std::time::Instant::now()));
            return;
        }

        let undo_before = self.checkpoint_viewed_track();
        let mut pasted_notes = Vec::new();

        if let Some(clip) = self.nav.active_clip_mut() {
            let paste_tick = (paste_col as i64 * clip.length_ticks) / col_count as i64;
            for n in &yank_buf {
                let new_note = (n.note as i16 + note_shift).clamp(0, 127) as u8;
                let new_start = n.start_tick + paste_tick;
                if new_start + n.duration_ticks <= clip.length_ticks {
                    let pasted = phosphor_core::clip::NoteSnapshot {
                        note: new_note,
                        velocity: n.velocity,
                        start_tick: new_start,
                        duration_ticks: n.duration_ticks,
                        muted: false,
                    };
                    clip.notes.push(pasted);
                    pasted_notes.push(pasted);
                }
            }
        }

        if !pasted_notes.is_empty() {
            let count = pasted_notes.len();
            self.commit_viewed_track(undo_before, "paste notes");
            // Invalidate stale note indices
            self.nav.clip_view.piano_roll.selected_note_indices.clear();
            self.status_message = Some((
                format!("{count} note{} pasted (u to undo)", if count == 1 { "" } else { "s" }),
                std::time::Instant::now(),
            ));
        } else {
            self.status_message = Some(("nothing to paste".into(), std::time::Instant::now()));
        }
    }

    /// Stretch all notes in the highlighted column/row region.
    /// `right_edge` = true adjusts duration, false adjusts start position.
    pub(crate) fn stretch_highlighted_notes(&mut self, delta_ticks: i64, right_edge: bool) {
        use crate::debug_log as dbg;
        let col_range = self.nav.clip_view.piano_roll.highlight_range();
        let row_range = self.nav.clip_view.piano_roll.row_highlight_range();
        let col_count = self.nav.clip_view.piano_roll.column_count;
        let undo_before = self.checkpoint_viewed_track();

        if col_count == 0 { return; }

        let mut touched = 0usize;
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks;
            for note in clip.notes.iter_mut() {
                if note_in_selection(note, len, col_range, row_range, col_count) {
                    touched += 1;
                    Self::apply_edge_delta(note, delta_ticks, right_edge, len);
                }
            }
        }

        if touched > 0 {
            self.commit_viewed_track(undo_before, "stretch notes");
            self.send_clip_update();
            dbg::system(&format!("piano roll: stretched highlighted notes edge={} delta={}",
                if right_edge { "right" } else { "left" }, delta_ticks));
        }
    }

    /// Move all notes in the highlighted column/row region by grid steps and semitones.
    /// Uses the same grid resolution as edit mode for horizontal movement.
    pub(crate) fn move_highlighted_notes(&mut self, grid_steps: i32, semitones: i32) {
        use crate::debug_log as dbg;
        let col_range = self.nav.clip_view.piano_roll.highlight_range();
        let row_range = self.nav.clip_view.piano_roll.row_highlight_range();
        let col_count = self.nav.clip_view.piano_roll.column_count;
        let total_beats = self.nav.clip_view.piano_roll.total_beats;
        let grid = self.nav.clip_view.piano_roll.grid;
        let snap = self.nav.clip_view.piano_roll.snap_enabled;
        let undo_before = self.checkpoint_viewed_track();

        if col_count == 0 { return; }
        let _ = total_beats;
        let step = grid.step_ticks(Transport::PPQ);

        let mut touched = 0usize;
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks;
            for note in clip.notes.iter_mut() {
                if note_in_selection(note, len, col_range, row_range, col_count) {
                    touched += 1;
                    if grid_steps != 0 {
                        let new_tick = note.start_tick + grid_steps as i64 * step;
                        let new_tick = if snap {
                            grid.snap_ticks(new_tick, Transport::PPQ)
                        } else {
                            new_tick
                        };
                        note.start_tick = new_tick.clamp(0, (len - note.duration_ticks).max(0));
                    }
                    if semitones != 0 {
                        let new_note = note.note as i32 + semitones;
                        note.note = new_note.clamp(0, 127) as u8;
                    }
                }
            }
        }

        if touched > 0 {
            self.commit_viewed_track(undo_before, "move notes");
            self.send_clip_update();
            dbg::system(&format!("piano roll: moved highlighted notes steps={} semi={}", grid_steps, semitones));
        }
    }

    pub(crate) fn apply_quantize(&mut self, grid: crate::state::GridResolution, strength: u8) {
        use crate::debug_log as dbg;
        let undo_before = self.checkpoint_viewed_track();
        let strength_frac = strength as f64 / 100.0;

        let mut touched = 0usize;
        if let Some(clip) = self.nav.active_clip_mut() {
            let len = clip.length_ticks;
            for note in clip.notes.iter_mut() {
                let snapped = grid.snap_ticks(note.start_tick, Transport::PPQ);
                if snapped != note.start_tick {
                    touched += 1;
                    // Clamp to keep the note within the clip bounds. Without
                    // this, a note near the end snaps to the clip's end and
                    // vanishes from the piano roll while still playing.
                    // Invariant: start + duration <= length.
                    let moved = note.start_tick
                        + (((snapped - note.start_tick) as f64) * strength_frac).round() as i64;
                    note.start_tick = moved.clamp(0, (len - note.duration_ticks).max(0));
                }
            }
            if touched > 0 {
                // Quantize pulls neighbours onto the same grid line; two
                // copies of one pitch on one line are one note that plays
                // twice as loud and can only be deleted twice. Keep the
                // harder hit.
                clip.notes.sort_by(|a, b| {
                    a.note
                        .cmp(&b.note)
                        .then(a.start_tick.cmp(&b.start_tick))
                        .then(b.velocity.cmp(&a.velocity))
                });
                clip.notes.dedup_by(|a, b| a.note == b.note && a.start_tick == b.start_tick);
            }
        }

        if touched > 0 {
            let count = touched;
            self.commit_viewed_track(undo_before, "quantize");
            self.send_clip_update();
            dbg::system(&format!("quantized {} notes to {} at {}%", count, grid.label(), strength));
            self.status_message = Some((
                format!("{count} note{} quantized", if count == 1 { "" } else { "s" }),
                std::time::Instant::now(),
            ));
        } else {
            self.status_message = Some(("notes already quantized".into(), std::time::Instant::now()));
        }
    }
}
