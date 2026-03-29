//! App methods: undo redo.

use super::*;

impl App {

    // ── Undo / Redo ──

    pub(crate) fn perform_undo(&mut self) {
        use crate::debug_log as dbg;
        let action = match self.nav.undo_stack.pop_undo() {
            Some(a) => a,
            None => {
                dbg::system("undo: stack empty");
                self.status_message = Some(("nothing to undo".into(), std::time::Instant::now()));
                return;
            }
        };

        dbg::system(&format!("undo: popped action {:?}", std::mem::discriminant(&action)));

        match action.clone() {
            UndoAction::DrawNote { track_idx, clip_idx, note } => {
                // Undo draw = remove the note
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        clip.notes.retain(|n| !(n.note == note.note && (n.start_frac - note.start_frac).abs() < 0.001));
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                self.status_message = Some(("undo: note removed".into(), std::time::Instant::now()));
            }
            UndoAction::RemoveNote { track_idx, clip_idx, note } => {
                // Undo remove = add the note back
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        clip.notes.push(note);
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                self.status_message = Some(("undo: note restored".into(), std::time::Instant::now()));
            }
            UndoAction::DeleteNotes { track_idx, clip_idx, notes } => {
                // Undo bulk delete = add all notes back
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        clip.notes.extend(notes);
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                dbg::system("undo: notes restored");
                self.status_message = Some(("undo: notes restored".into(), std::time::Instant::now()));
            }
            UndoAction::PasteNotes { track_idx, clip_idx, notes } => {
                // Undo paste = remove the pasted notes
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        for del in &notes {
                            clip.notes.retain(|n| {
                                !(n.note == del.note
                                    && (n.start_frac - del.start_frac).abs() < 0.001
                                    && (n.duration_frac - del.duration_frac).abs() < 0.001)
                            });
                        }
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                dbg::system(&format!("undo: removed {} pasted notes", notes.len()));
                self.status_message = Some(("undo: paste removed".into(), std::time::Instant::now()));
            }
            UndoAction::MoveNotes { track_idx, clip_idx, ref before } => {
                // Undo move = restore each note to its original position
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        for (idx, original) in before {
                            if let Some(note) = clip.notes.get_mut(*idx) {
                                *note = *original;
                            }
                        }
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                dbg::system(&format!("undo: restored {} moved notes", before.len()));
                self.status_message = Some(("undo: notes restored".into(), std::time::Instant::now()));
            }
            UndoAction::AddClip { track_idx, clip_idx } => {
                // Undo add = remove the clip
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if clip_idx < track.clips.len() {
                        // Remove from audio thread
                        if let Some(mid) = track.mixer_id {
                            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveClip {
                                track_id: mid,
                                clip_index: clip_idx,
                            });
                        }
                        track.clips.remove(clip_idx);
                    }
                }
                // Fix selection
                if let crate::state::TrackElement::Clip(i) = self.nav.track_element {
                    if i >= clip_idx {
                        let num = self.nav.tracks.get(track_idx).map(|t| t.clips.len()).unwrap_or(0);
                        if num > 0 {
                            self.nav.track_element = crate::state::TrackElement::Clip(i.saturating_sub(1).min(num - 1));
                        } else {
                            self.nav.track_element = crate::state::TrackElement::Label;
                        }
                    }
                }
                self.nav.sync_clip_view_target();
                dbg::system(&format!("undo: removed added clip {} on track {}", clip_idx, track_idx));
                self.status_message = Some(("undo: clip removed".into(), std::time::Instant::now()));
            }
            UndoAction::DeleteClip { track_idx, clip_idx, clip } => {
                // Undo clip delete = insert it back
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    let idx = clip_idx.min(track.clips.len());
                    track.clips.insert(idx, clip.clone());
                    // Recreate clip on audio thread
                    if let Some(mid) = track.mixer_id {
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                            track_id: mid,
                            start_tick: clip.start_tick,
                            length_ticks: clip.length_ticks,
                        });
                        let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                            &clip.notes, clip.length_ticks,
                        );
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                            track_id: mid, clip_index: idx, events,
                        });
                    }
                }
                self.status_message = Some(("undo: clip restored".into(), std::time::Instant::now()));
            }
            UndoAction::DeleteTrack { track_idx: _, track, mixer_id: _ } => {
                // Undo track delete = re-create it
                let instrument = match track.instrument_type {
                    Some(i) => i,
                    None => return,
                };
                // Re-create the track on the audio side
                self.create_instrument_track(instrument);
                // Restore the saved state over the newly created track
                let new_idx = self.nav.track_cursor;
                if let Some(new_track) = self.nav.tracks.get_mut(new_idx) {
                    new_track.name = track.name.clone();
                    new_track.muted = track.muted;
                    new_track.soloed = track.soloed;
                    new_track.armed = track.armed;
                    new_track.volume = track.volume;
                    new_track.color_index = track.color_index;
                    new_track.synth_params = track.synth_params.clone();
                    new_track.clips = track.clips.clone();
                    new_track.sync_to_audio();

                    if let Some(mid) = new_track.mixer_id {
                        // Send params to audio
                        for (i, &v) in new_track.synth_params.iter().enumerate() {
                            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetParameter {
                                track_id: mid, param_index: i, value: v,
                            });
                        }
                        // Recreate clips on the audio thread
                        for (ci, clip) in new_track.clips.iter().enumerate() {
                            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                                track_id: mid,
                                start_tick: clip.start_tick,
                                length_ticks: clip.length_ticks,
                            });
                            let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                                &clip.notes, clip.length_ticks,
                            );
                            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                                track_id: mid, clip_index: ci, events,
                            });
                        }
                        dbg::system(&format!("undo: restored {} clips to audio", new_track.clips.len()));
                    }
                }
                self.status_message = Some(("undo: track restored".into(), std::time::Instant::now()));
                dbg::system("undo: track restored");
            }
        }

        self.nav.undo_stack.push_redo(action);
    }


    pub(crate) fn perform_redo(&mut self) {
        let action = match self.nav.undo_stack.pop_redo() {
            Some(a) => a,
            None => {
                self.status_message = Some(("nothing to redo".into(), std::time::Instant::now()));
                return;
            }
        };

        // Redo = do the opposite of undo (re-apply the original action)
        match action.clone() {
            UndoAction::DrawNote { track_idx, clip_idx, note } => {
                // Redo draw = add the note again
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        clip.notes.push(note);
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                self.status_message = Some(("redo: note drawn".into(), std::time::Instant::now()));
            }
            UndoAction::RemoveNote { track_idx, clip_idx, note } => {
                // Redo remove = remove the note again
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        clip.notes.retain(|n| !(n.note == note.note && (n.start_frac - note.start_frac).abs() < 0.001));
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                self.status_message = Some(("redo: note removed".into(), std::time::Instant::now()));
            }
            UndoAction::DeleteNotes { track_idx, clip_idx, ref notes } => {
                // Redo delete = remove the notes again
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        for del_note in notes {
                            clip.notes.retain(|n| !(n.note == del_note.note && (n.start_frac - del_note.start_frac).abs() < 0.001));
                        }
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                self.status_message = Some(("redo: notes deleted".into(), std::time::Instant::now()));
            }
            UndoAction::PasteNotes { track_idx, clip_idx, ref notes } => {
                // Redo paste = add the notes back
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if let Some(clip) = track.clips.get_mut(clip_idx) {
                        clip.notes.extend(notes.iter().cloned());
                    }
                }
                self.send_clip_update_for(track_idx, clip_idx);
                self.status_message = Some(("redo: paste restored".into(), std::time::Instant::now()));
            }
            UndoAction::MoveNotes { .. } => {
                self.status_message = Some(("redo: not available for moves".into(), std::time::Instant::now()));
            }
            UndoAction::AddClip { .. } => {
                self.status_message = Some(("redo: not available for paste/duplicate".into(), std::time::Instant::now()));
            }
            UndoAction::DeleteClip { track_idx, clip_idx, .. } => {
                // Redo clip delete = remove it again
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    if clip_idx < track.clips.len() {
                        if let Some(mid) = track.mixer_id {
                            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveClip {
                                track_id: mid,
                                clip_index: clip_idx,
                            });
                        }
                        track.clips.remove(clip_idx);
                    }
                }
                self.status_message = Some(("redo: clip deleted".into(), std::time::Instant::now()));
            }
            UndoAction::DeleteTrack { track_idx, .. } => {
                // Redo track delete = remove the track again
                if track_idx < self.nav.tracks.len() {
                    if let Some(mid) = self.nav.tracks[track_idx].mixer_id {
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveTrack { track_id: mid });
                    }
                    self.nav.tracks.remove(track_idx);
                    if self.nav.track_cursor >= self.nav.tracks.len() && self.nav.track_cursor > 0 {
                        self.nav.track_cursor -= 1;
                    }
                    self.nav.track_selected = false;
                    self.nav.clip_view_visible = false;
                    self.engine.panic();
                }
                self.status_message = Some(("redo: track deleted".into(), std::time::Instant::now()));
            }
        }

        // Push back to undo stack (without clearing redo) so it can be undone again
        self.nav.undo_stack.push_undo_only(action);
    }

    // ── Piano roll highlight delete ──

}
