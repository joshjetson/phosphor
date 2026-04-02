//! App methods: delete.

use super::*;

impl App {

    // ── Delete ──

    pub(crate) fn handle_delete_request(&mut self) {
        use crate::debug_log as dbg;

        // What are we deleting? Depends on context.
        if self.nav.focused_pane == Pane::Tracks && self.nav.track_selected {
            // Check if we're on a clip element
            if let crate::state::TrackElement::Clip(clip_idx) = self.nav.track_element {
                if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
                    if clip_idx < track.clips.len() {
                        let msg = format!("Delete clip {} on '{}'?  y/n", clip_idx + 1, track.name);
                        dbg::user(&format!("delete request: clip {} on track {}", clip_idx, self.nav.track_cursor));
                        self.nav.confirm_modal.show(ConfirmKind::DeleteClip, &msg);
                        return;
                    }
                }
            }

            // Otherwise delete the track itself
            if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
                if track.instrument_type.is_some() {
                    let msg = format!("Delete track '{}'?  y/n", track.name);
                    dbg::user(&format!("delete request: track {}", self.nav.track_cursor));
                    self.nav.confirm_modal.show(ConfirmKind::DeleteTrack, &msg);
                }
            }
        } else if self.nav.focused_pane == Pane::Tracks && !self.nav.track_selected {
            // Track not selected but cursor is on it — delete the track
            if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
                if track.instrument_type.is_some() {
                    let msg = format!("Delete track '{}'?  y/n", track.name);
                    self.nav.confirm_modal.show(ConfirmKind::DeleteTrack, &msg);
                }
            }
        }
    }


    pub(crate) fn execute_delete(&mut self, kind: ConfirmKind) {
        use crate::debug_log as dbg;
        match kind {
            ConfirmKind::DeleteTrack => {
                let idx = self.nav.track_cursor;
                if let Some(track) = self.nav.tracks.get(idx) {
                    if track.instrument_type.is_none() { return; }

                    let mixer_id = track.mixer_id.unwrap_or(0);

                    // Push undo BEFORE removing
                    let track_clone = self.nav.tracks[idx].clone();
                    self.nav.undo_stack.push(UndoAction::DeleteTrack {
                        track_idx: idx, track: track_clone, mixer_id,
                    });

                    if let Some(mid) = self.nav.tracks[idx].mixer_id {
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveTrack {
                            track_id: mid,
                        });
                        dbg::system(&format!("deleted track: mixer_id={}", mid));
                    }

                    self.nav.tracks.remove(idx);
                    if self.nav.track_cursor >= self.nav.tracks.len() && self.nav.track_cursor > 0 {
                        self.nav.track_cursor -= 1;
                    }
                    self.nav.track_selected = false;
                    self.nav.clip_view_visible = false;
                    self.nav.clip_view_target = None;
                    self.engine.panic();

                    self.status_message = Some(("track deleted (u to undo)".into(), std::time::Instant::now()));
                }
            }
            ConfirmKind::DeleteClip => {
                if let crate::state::TrackElement::Clip(clip_idx) = self.nav.track_element {
                    let track_idx = self.nav.track_cursor;
                    let mut deleted_mixer_id = None;
                    let mut remaining = 0usize;

                    if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                        if clip_idx < track.clips.len() {
                            // Remove from audio thread
                            if let Some(mixer_id) = track.mixer_id {
                                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveClip {
                                    track_id: mixer_id,
                                    clip_index: clip_idx,
                                });
                                deleted_mixer_id = Some(mixer_id);
                            }

                            let removed_clip = track.clips.remove(clip_idx);
                            self.nav.undo_stack.push(UndoAction::DeleteClip {
                                track_idx, clip_idx, clip: removed_clip,
                            });
                            dbg::system(&format!("deleted clip {} on track {}", clip_idx, track_idx));
                            remaining = track.clips.len();
                        }
                    }

                    // Only proceed if we actually deleted something
                    if deleted_mixer_id.is_some() || remaining > 0 || self.nav.tracks.get(track_idx).map(|t| t.clips.is_empty()).unwrap_or(false) {
                        // Drain pending clip snapshots for the deleted track's mixer
                        // so a recording commit doesn't re-add the clip
                        if let Some(mid) = deleted_mixer_id {
                            let mut keep = Vec::new();
                            while let Ok(snap) = self.clip_rx.try_recv() {
                                if snap.track_id == mid {
                                    dbg::system(&format!("discarded snapshot for track {} after clip delete", snap.track_id));
                                } else {
                                    keep.push(snap);
                                }
                            }
                            let is_recording = self.engine.transport.is_recording();
                            for snap in keep {
                                let _ = self.nav.receive_clip_snapshot(snap, is_recording);
                            }
                        }

                        // Select adjacent clip instead of falling back to Label
                        if remaining > 0 && clip_idx < remaining {
                            self.nav.track_element = crate::state::TrackElement::Clip(clip_idx);
                            self.nav.open_clip_view(track_idx, clip_idx);
                        } else if remaining > 0 {
                            let prev = remaining - 1;
                            self.nav.track_element = crate::state::TrackElement::Clip(prev);
                            self.nav.open_clip_view(track_idx, prev);
                        } else {
                            self.nav.track_element = crate::state::TrackElement::Label;
                            self.nav.clip_view_visible = false;
                            self.nav.clip_view_target = None;
                        }
                        self.status_message = Some(("clip deleted (u to undo)".into(), std::time::Instant::now()));
                    }
                }
            }
        }
    }

    // ── Undo / Redo ──

}
