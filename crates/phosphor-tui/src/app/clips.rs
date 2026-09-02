//! App methods: clip manipulation (move, stretch, yank, paste, duplicate).

use super::*;

impl App {
    /// Move a clip left/right by one beat (changes start_tick, keeps length).
    pub(crate) fn move_clip(&mut self, clip_idx: usize, direction: i64) {
        use crate::debug_log as dbg;
        let ppq = phosphor_core::transport::Transport::PPQ;
        let beat_ticks = ppq;

        let track_idx = self.nav.track_cursor;
        if let Some(track) = self.nav.tracks.get(track_idx) {
            if let Some(clip) = track.clips.get(clip_idx) {
                let old_start = clip.start_tick;
                let clip_len = clip.length_ticks;
                let mut new_start = (old_start + direction * beat_ticks).max(0);

                // Collision: don't overlap adjacent clips
                if direction < 0 {
                    let prev_end = track.clips.iter()
                        .filter(|c| c.start_tick < old_start)
                        .map(|c| c.start_tick + c.length_ticks)
                        .max();
                    if let Some(pe) = prev_end {
                        new_start = new_start.max(pe);
                    }
                } else {
                    let next_start = track.clips.iter()
                        .filter(|c| c.start_tick > old_start)
                        .map(|c| c.start_tick)
                        .min();
                    if let Some(ns) = next_start {
                        new_start = new_start.min(ns - clip_len).max(0);
                    }
                }

                if new_start == old_start { return; }

                let undo_before = self.nav.undo_checkpoint(
                    crate::state::undo::UndoScope::TrackClips { track_idx },
                );

                let track = self.nav.tracks.get_mut(track_idx).unwrap();
                let clip = track.clips.get_mut(clip_idx).unwrap();
                clip.start_tick = new_start;
                dbg::system(&format!("clip move: {} → {} ticks", old_start, new_start));

                self.nav.commit_undo(undo_before, "move clip");
                self.sync_clip_to_audio(track_idx, clip_idx);
                self.status_message = Some((
                    format!("clip moved to beat {}", new_start / ppq + 1),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    /// Stretch/shrink right edge of clip by one beat.
    pub(crate) fn move_clip_right_edge(&mut self, clip_idx: usize, direction: i64) {
        use crate::debug_log as dbg;
        let ppq = phosphor_core::transport::Transport::PPQ;
        let beat_ticks = ppq;

        let track_idx = self.nav.track_cursor;
        if let Some(track) = self.nav.tracks.get(track_idx) {
            if let Some(clip) = track.clips.get(clip_idx) {
                let old_len = clip.length_ticks;
                let clip_start = clip.start_tick;
                let mut new_len = (old_len + direction * beat_ticks).max(ppq); // min 1 beat

                // Collision: don't extend past the start of the next clip
                let next_start = track.clips.iter()
                    .filter(|c| c.start_tick > clip_start)
                    .map(|c| c.start_tick)
                    .min();
                if let Some(ns) = next_start {
                    new_len = new_len.min(ns - clip_start).max(ppq);
                }

                if new_len == old_len { return; }

                let undo_before = self.nav.undo_checkpoint(
                    crate::state::undo::UndoScope::TrackClips { track_idx },
                );

                let track = self.nav.tracks.get_mut(track_idx).unwrap();
                let clip = track.clips.get_mut(clip_idx).unwrap();

                // Convert all notes to absolute tick offsets, change clip length,
                // then convert back. Notes outside the new boundary get hidden.
                // Hidden notes are stored as tick offsets so they survive any
                // number of shrink/expand cycles.

                // Step 1: notes already live in tick offsets
                let mut all_ticks: Vec<(i64, i64, u8, u8)> = clip.notes.drain(..)
                    .map(|n| (n.start_tick, n.duration_ticks, n.note, n.velocity))
                    .collect();

                // Include previously hidden notes
                all_ticks.extend(clip.hidden_notes.drain(..));

                // Step 2: partition into visible (within new_len) and hidden
                let mut visible = Vec::new();
                let mut hidden = Vec::new();
                for (st, dur, note, vel) in all_ticks {
                    if st < new_len {
                        visible.push(phosphor_core::clip::NoteSnapshot {
                            note,
                            velocity: vel,
                            start_tick: st,
                            duration_ticks: dur.min(new_len - st).max(1),
                            muted: false,
                        });
                    } else {
                        hidden.push((st, dur, note, vel));
                    }
                }

                clip.notes = visible;
                clip.hidden_notes = hidden;
                clip.length_ticks = new_len;
                let beats = (new_len as f64 / ppq as f64).ceil() as u16;
                clip.width = beats.max(2);

                dbg::system(&format!(
                    "clip right edge: len {} → {}, {} visible, {} hidden",
                    old_len, new_len, clip.notes.len(), clip.hidden_notes.len()
                ));

                self.nav.commit_undo(undo_before, "resize clip");
                self.sync_clip_to_audio(track_idx, clip_idx);
                self.status_message = Some((
                    format!("clip length: {} beats", new_len / ppq),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    /// Trim left edge of clip (start moves, right edge stays fixed, length changes).
    pub(crate) fn move_clip_left_edge(&mut self, clip_idx: usize, direction: i64) {
        use crate::debug_log as dbg;
        let ppq = phosphor_core::transport::Transport::PPQ;
        let beat_ticks = ppq;

        let track_idx = self.nav.track_cursor;
        if let Some(track) = self.nav.tracks.get(track_idx) {
            if let Some(clip) = track.clips.get(clip_idx) {
                let old_start = clip.start_tick;
                let old_len = clip.length_ticks;
                let end_tick = old_start + old_len;
                let mut new_start = (old_start + direction * beat_ticks).max(0);

                // Don't let start pass the end (min 1 beat)
                if new_start >= end_tick - ppq { return; }

                // Collision: don't move start past the end of the previous clip
                let prev_end = track.clips.iter()
                    .filter(|c| c.start_tick < old_start)
                    .map(|c| c.start_tick + c.length_ticks)
                    .max();
                if let Some(pe) = prev_end {
                    new_start = new_start.max(pe);
                }

                if new_start == old_start { return; }

                let new_len = end_tick - new_start;

                let undo_before = self.nav.undo_checkpoint(
                    crate::state::undo::UndoScope::TrackClips { track_idx },
                );

                let track = self.nav.tracks.get_mut(track_idx).unwrap();
                let clip = track.clips.get_mut(clip_idx).unwrap();

                // Convert notes to absolute timeline ticks, move start, convert back.
                // Notes that fall before the new start get hidden.
                let mut all_ticks: Vec<(i64, i64, u8, u8)> = clip.notes.drain(..)
                    .map(|n| (old_start + n.start_tick, n.duration_ticks, n.note, n.velocity))
                    .collect();
                // Include hidden notes (stored as tick offsets from old clip start)
                for (st, dur, note, vel) in clip.hidden_notes.drain(..) {
                    all_ticks.push((old_start + st, dur, note, vel));
                }

                let mut visible = Vec::new();
                let mut hidden = Vec::new();
                for (abs_st, dur, note, vel) in all_ticks {
                    let rel = abs_st - new_start;
                    if rel >= 0 && rel < new_len {
                        visible.push(phosphor_core::clip::NoteSnapshot {
                            note, velocity: vel,
                            start_tick: rel,
                            duration_ticks: dur.min(new_len - rel).max(1),
                            muted: false,
                        });
                    } else {
                        // Store as offset from new clip start (may be negative for left-trimmed)
                        hidden.push((abs_st - new_start, dur, note, vel));
                    }
                }

                clip.notes = visible;
                clip.hidden_notes = hidden;
                // The controllers ride ticks-from-clip-start, so a moved
                // start shifts them the opposite way; ones trimmed off the
                // front keep negative ticks and come back if the edge does.
                for e in &mut clip.controls {
                    e.tick += old_start - new_start;
                }
                clip.start_tick = new_start;
                clip.length_ticks = new_len;
                let beats = (new_len as f64 / ppq as f64).ceil() as u16;
                clip.width = beats.max(2);

                dbg::system(&format!(
                    "clip left edge: start {} → {}, len {}, {} visible, {} hidden",
                    old_start, new_start, new_len, clip.notes.len(), clip.hidden_notes.len()
                ));

                self.nav.commit_undo(undo_before, "trim clip");
                self.sync_clip_to_audio(track_idx, clip_idx);
                self.status_message = Some((
                    format!("clip start: beat {}", new_start / ppq + 1),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    /// Yank (copy) a clip to the clipboard.
    pub(crate) fn yank_clip(&mut self, clip_idx: usize) {
        let track_idx = self.nav.track_cursor;
        if let Some(track) = self.nav.tracks.get(track_idx) {
            if let Some(clip) = track.clips.get(clip_idx) {
                self.yanked_clips = vec![clip.clone()];
                self.status_message = Some((
                    format!("clip {} yanked", clip_idx + 1),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    /// Yank a track's whole arrangement — every clip, with the bars each
    /// one sits on. `P` on another track then lays the lot down at the same
    /// bars, which is how a recorded part gets doubled by a second
    /// instrument without walking it over clip by clip.
    pub(crate) fn yank_all_clips(&mut self) {
        let track_idx = self.nav.track_cursor;
        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        if track.clips.is_empty() {
            self.flash("nothing to yank \u{b7} this track has no clips");
            return;
        }
        self.yanked_clips = track.clips.clone();
        let count = self.yanked_clips.len();
        self.flash(format!(
            "{count} clip{} yanked \u{b7} P lays them on another track",
            if count == 1 { "" } else { "s" }
        ));
    }

    /// Whether the current track can take clips at all, saying so if not.
    /// A clip pasted on a bus strip would draw on screen and never play,
    /// because the mixer's buses hold no clips.
    fn track_takes_clips(&mut self, track_idx: usize) -> bool {
        let Some(track) = self.nav.tracks.get(track_idx) else { return false };
        if track.is_live() {
            return true;
        }
        self.flash("clips live on instrument tracks");
        false
    }

    /// Whether `start_tick..start_tick + length_ticks` on this track is
    /// clear of clips. Moving, stretching, trimming, recording and pasting
    /// all obey the same law: the timeline never holds two clips on the
    /// same bars, because overlap double-fires every note.
    fn clip_room(&self, track_idx: usize, start_tick: i64, length_ticks: i64) -> bool {
        let Some(track) = self.nav.tracks.get(track_idx) else { return false };
        let end_tick = start_tick + length_ticks;
        !track
            .clips
            .iter()
            .any(|c| start_tick < c.start_tick + c.length_ticks && c.start_tick < end_tick)
    }

    /// Put one clip on a track, on both sides of the fence. No checks and
    /// no undo step — the caller has already done both.
    fn place_clip(&mut self, track_idx: usize, mut clip: crate::state::Clip) {
        let Some(track) = self.nav.tracks.get_mut(track_idx) else { return };
        clip.number = track.clips.len() + 1;
        if let Some(mixer_id) = track.mixer_id {
            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                track_id: mixer_id,
                start_tick: clip.start_tick,
                length_ticks: clip.length_ticks,
            });
            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                track_id: mixer_id,
                clip_index: track.clips.len(),
                events: clip.events_for_audio(),
            });
        }
        track.clips.push(clip);
    }

    /// Paste the yanked clip at a specific start tick on the current track.
    /// Returns whether it landed, so a caller does not announce a paste
    /// that was refused.
    fn paste_clip_at(&mut self, start_tick: i64) -> bool {
        use crate::debug_log as dbg;

        let Some(yanked) = self.yanked_clips.first().cloned() else {
            self.flash("no clip to paste");
            return false;
        };
        let track_idx = self.nav.track_cursor;
        if !self.track_takes_clips(track_idx) {
            return false;
        }
        if !self.clip_room(track_idx, start_tick, yanked.length_ticks) {
            let ppq = phosphor_core::transport::Transport::PPQ;
            self.flash(format!(
                "no room at beat {} \u{b7} clips cannot overlap",
                start_tick / ppq + 1
            ));
            return false;
        }

        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackClips { track_idx },
        );
        let mut clip = yanked;
        clip.start_tick = start_tick;
        self.place_clip(track_idx, clip);
        dbg::system(&format!("pasted clip to track {} at tick {}", track_idx, start_tick));
        self.nav.commit_undo(undo_before, "paste clip");

        // Select the newly pasted clip
        let new_idx = self.nav.tracks[track_idx].clips.len() - 1;
        self.nav.track_element = crate::state::TrackElement::Clip(new_idx);
        self.nav.open_clip_view(track_idx, new_idx);
        true
    }

    /// Paste yanked clip right after the given clip on the same track.
    pub(crate) fn paste_clip_after(&mut self, clip_idx: usize) -> bool {
        let track_idx = self.nav.track_cursor;
        let after_tick = self.nav.tracks.get(track_idx)
            .and_then(|t| t.clips.get(clip_idx))
            .map(|c| c.start_tick + c.length_ticks)
            .unwrap_or(0);

        let landed = self.paste_clip_at(after_tick);
        if landed {
            self.flash(format!(
                "clip pasted at beat {}",
                after_tick / phosphor_core::transport::Transport::PPQ + 1
            ));
        }
        landed
    }

    /// Lay the yanked clips onto the current track, each on the bars it was
    /// yanked from. One clip is the cross-track paste; a whole arrangement
    /// — `y` on the track label — is the layering gesture: record a part,
    /// yank the lot, put it under a different instrument in two keys.
    ///
    /// All or nothing: if any clip has no room, nothing lands and the
    /// refusal says which beat is blocked. Half an arrangement is not an
    /// arrangement.
    pub(crate) fn paste_clip_to_track(&mut self) {
        if self.yanked_clips.len() <= 1 {
            let Some(start_tick) = self.yanked_clips.first().map(|c| c.start_tick) else {
                self.flash("no clip to paste");
                return;
            };
            if self.paste_clip_at(start_tick) {
                self.flash("clip pasted to track");
            }
            return;
        }

        let track_idx = self.nav.track_cursor;
        if !self.track_takes_clips(track_idx) {
            return;
        }
        let clips = self.yanked_clips.clone();
        for clip in &clips {
            if !self.clip_room(track_idx, clip.start_tick, clip.length_ticks) {
                let ppq = phosphor_core::transport::Transport::PPQ;
                self.flash(format!(
                    "no room at beat {} \u{b7} clips cannot overlap",
                    clip.start_tick / ppq + 1
                ));
                return;
            }
        }

        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackClips { track_idx },
        );
        let count = clips.len();
        for clip in clips {
            self.place_clip(track_idx, clip);
        }
        self.nav.commit_undo(undo_before, "paste clips");
        self.nav.track_element = crate::state::TrackElement::Clip(0);
        self.nav.open_clip_view(track_idx, 0);
        self.flash(format!("{count} clips laid on their bars"));
    }

    /// Duplicate clip immediately after itself.
    pub(crate) fn duplicate_clip(&mut self, clip_idx: usize) {
        // Yank then paste after
        self.yank_clip(clip_idx);
        if self.paste_clip_after(clip_idx) {
            self.flash("clip duplicated");
        }
    }

    /// Make the audio thread's clip list for one track identical to the UI's.
    ///
    /// `audio_clip_count` is how many clips the audio side currently holds
    /// for this track — the caller knows, because it is the one that just
    /// diverged from it: an absorbed recording left the audio side with the
    /// UI's count plus the absorbed ones, an undo left it with the count the
    /// UI had before the slice was applied. Everything is removed from the
    /// top down and the UI's list recreated, which is the one order that
    /// cannot leave a stale clip playing under a renumbered index.
    pub(crate) fn resync_track_clips_to_audio(&self, track_idx: usize, audio_clip_count: usize) {
        use crate::debug_log as dbg;
        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        let Some(mixer_id) = track.mixer_id else { return };
        let tx = &self.engine.shared.mixer_command_tx;
        for i in (0..audio_clip_count).rev() {
            let _ = tx.send(MixerCommand::RemoveClip { track_id: mixer_id, clip_index: i });
        }
        for (ci, clip) in track.clips.iter().enumerate() {
            let _ = tx.send(MixerCommand::CreateClip {
                track_id: mixer_id,
                start_tick: clip.start_tick,
                length_ticks: clip.length_ticks,
            });
            let _ = tx.send(MixerCommand::UpdateClip {
                track_id: mixer_id, clip_index: ci, events: clip.events_for_audio(),
            });
        }
        dbg::system(&format!(
            "clip resync: track={} removed {} rebuilt {}",
            mixer_id, audio_clip_count, track.clips.len()
        ));
    }

    /// Sync a clip's data to the audio thread after editing (move, stretch, etc).
    pub(crate) fn sync_clip_to_audio(&self, track_idx: usize, clip_idx: usize) {
        use crate::debug_log as dbg;
        if let Some(track) = self.nav.tracks.get(track_idx) {
            if let (Some(mixer_id), Some(clip)) = (track.mixer_id, track.clips.get(clip_idx)) {
                // Update position and length
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClipPosition {
                    track_id: mixer_id,
                    clip_index: clip_idx,
                    start_tick: clip.start_tick,
                    length_ticks: clip.length_ticks,
                });
                // Update events
                let events = clip.events_for_audio();
                let event_count = events.len();
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                    track_id: mixer_id,
                    clip_index: clip_idx,
                    events,
                });
                dbg::system(&format!(
                    "sync clip audio: track={} clip={} start={} len={} events={}",
                    mixer_id, clip_idx, clip.start_tick, clip.length_ticks, event_count
                ));
            }
        }
    }

    /// Run dedup on TUI clips and sync removals + updates to the audio thread.
    pub(crate) fn sync_dedup_to_audio(&mut self) {
        use crate::debug_log as dbg;
        let removed = self.nav.dedup_clips();

        // Send RemoveClip for each absorbed phantom (process in reverse index order
        // so indices stay valid)
        for &(mixer_id, clip_index) in removed.iter().rev() {
            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveClip {
                track_id: mixer_id,
                clip_index,
            });
            dbg::system(&format!("dedup audio: removed clip {} on mixer {}", clip_index, mixer_id));
        }

        // After removals, resync all remaining clips on affected tracks
        // (positions and events may have changed from absorption)
        let affected_mixers: Vec<usize> = removed.iter().map(|&(mid, _)| mid).collect();
        for track in &self.nav.tracks {
            if let Some(mid) = track.mixer_id {
                if !affected_mixers.contains(&mid) { continue; }
                for (ci, clip) in track.clips.iter().enumerate() {
                    let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClipPosition {
                        track_id: mid,
                        clip_index: ci,
                        start_tick: clip.start_tick,
                        length_ticks: clip.length_ticks,
                    });
                    let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                        track_id: mid,
                        clip_index: ci,
                        events: clip.events_for_audio(),
                    });
                }
                dbg::system(&format!("dedup audio: resynced {} clips on mixer {}", track.clips.len(), mid));
            }
        }
    }
}
