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

                // Apply mutably
                let track = self.nav.tracks.get_mut(track_idx).unwrap();
                let clip = track.clips.get_mut(clip_idx).unwrap();
                clip.start_tick = new_start;
                dbg::system(&format!("clip move: {} → {} ticks", old_start, new_start));

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

                // Now apply mutably
                let track = self.nav.tracks.get_mut(track_idx).unwrap();
                let clip = track.clips.get_mut(clip_idx).unwrap();

                // Rescale note fractions to preserve absolute tick positions
                let scale = old_len as f64 / new_len as f64;
                for note in &mut clip.notes {
                    note.start_frac *= scale;
                    note.duration_frac *= scale;
                }

                clip.length_ticks = new_len;
                let beats = (new_len as f64 / ppq as f64).ceil() as u16;
                clip.width = beats.max(2);

                dbg::system(&format!("clip right edge: len {} → {} (scale {:.3})", old_len, new_len, scale));

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

                // Now apply mutably
                let track = self.nav.tracks.get_mut(track_idx).unwrap();
                let clip = track.clips.get_mut(clip_idx).unwrap();

                // Rescale note fractions: preserve absolute tick positions
                // Note absolute tick = old_start + frac * old_len
                // New frac = (old_start + frac * old_len - new_start) / new_len
                for note in &mut clip.notes {
                    let abs_tick = old_start as f64 + note.start_frac * old_len as f64;
                    note.start_frac = (abs_tick - new_start as f64) / new_len as f64;
                    note.duration_frac *= old_len as f64 / new_len as f64;
                    // Clamp notes that fall outside
                    note.start_frac = note.start_frac.clamp(0.0, 1.0);
                }

                clip.start_tick = new_start;
                clip.length_ticks = new_len;
                let beats = (new_len as f64 / ppq as f64).ceil() as u16;
                clip.width = beats.max(2);

                dbg::system(&format!("clip left edge: start {} → {}, len {}", old_start, new_start, new_len));

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
                self.yanked_clip = Some(clip.clone());
                self.yanked_clip_start = clip.start_tick;
                self.status_message = Some((
                    format!("clip {} yanked", clip_idx + 1),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    /// Paste yanked clip right after the given clip on the same track.
    pub(crate) fn paste_clip_after(&mut self, clip_idx: usize) {
        use crate::debug_log as dbg;

        let yanked = match self.yanked_clip.clone() {
            Some(c) => c,
            None => {
                self.status_message = Some(("no clip to paste".into(), std::time::Instant::now()));
                return;
            }
        };

        let track_idx = self.nav.track_cursor;
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            // Place right after the current clip
            let after_tick = if let Some(cur) = track.clips.get(clip_idx) {
                cur.start_tick + cur.length_ticks
            } else {
                0
            };

            let mut new_clip = yanked;
            new_clip.start_tick = after_tick;
            new_clip.number = track.clips.len() + 1;

            // Send to audio thread
            if let Some(mixer_id) = track.mixer_id {
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                    track_id: mixer_id,
                    start_tick: new_clip.start_tick,
                    length_ticks: new_clip.length_ticks,
                });
                let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                    &new_clip.notes, new_clip.length_ticks,
                );
                let new_idx = track.clips.len();
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                    track_id: mixer_id,
                    clip_index: new_idx,
                    events,
                });
            }

            let new_idx = track.clips.len();
            track.clips.push(new_clip);
            dbg::system(&format!("pasted clip after clip {} at tick {}", clip_idx + 1, after_tick));

            // Select the newly pasted clip
            self.nav.track_element = crate::state::TrackElement::Clip(new_idx);
            self.nav.open_clip_view(track_idx, new_idx);

            self.status_message = Some((
                format!("clip pasted at beat {}", after_tick / phosphor_core::transport::Transport::PPQ + 1),
                std::time::Instant::now(),
            ));
        }
    }

    /// Paste yanked clip to the current track at the same timeline position.
    pub(crate) fn paste_clip_to_track(&mut self) {
        use crate::debug_log as dbg;

        let yanked = match self.yanked_clip.clone() {
            Some(c) => c,
            None => {
                self.status_message = Some(("no clip to paste".into(), std::time::Instant::now()));
                return;
            }
        };

        let track_idx = self.nav.track_cursor;
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            let mut new_clip = yanked;
            new_clip.start_tick = self.yanked_clip_start; // same position as source
            new_clip.number = track.clips.len() + 1;

            if let Some(mixer_id) = track.mixer_id {
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                    track_id: mixer_id,
                    start_tick: new_clip.start_tick,
                    length_ticks: new_clip.length_ticks,
                });
                let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                    &new_clip.notes, new_clip.length_ticks,
                );
                let new_idx = track.clips.len();
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                    track_id: mixer_id,
                    clip_index: new_idx,
                    events,
                });
            }

            let new_idx = track.clips.len();
            track.clips.push(new_clip);
            dbg::system(&format!("pasted clip to track {} at tick {}", track_idx, self.yanked_clip_start));

            // Select the newly pasted clip
            self.nav.track_element = crate::state::TrackElement::Clip(new_idx);
            self.nav.open_clip_view(track_idx, new_idx);

            self.status_message = Some(("clip pasted to track".into(), std::time::Instant::now()));
        }
    }

    /// Duplicate clip immediately after itself.
    pub(crate) fn duplicate_clip(&mut self, clip_idx: usize) {
        // Yank then paste after
        self.yank_clip(clip_idx);
        self.paste_clip_after(clip_idx);
        self.status_message = Some(("clip duplicated".into(), std::time::Instant::now()));
    }

    /// Sync a clip's data to the audio thread after editing (move, stretch, etc).
    fn sync_clip_to_audio(&self, track_idx: usize, clip_idx: usize) {
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
                let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                    &clip.notes, clip.length_ticks,
                );
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
}
