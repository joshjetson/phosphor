//! App methods: transport.

use super::*;

impl App {
    /// Stop playback and silence all instruments. Called on pause, stop,
    /// and stop-recording. Prevents notes from ringing after playback ends.
    pub(crate) fn stop_playback(&mut self) {
        let was_recording = self.engine.transport.is_recording();
        self.engine.transport.pause();
        // The mixer clears its own key listen on the stop edge; this keeps the
        // mirror in step so the panel and the status bar stop blinking in the
        // same frame the sound comes back.
        self.set_key_listen(None);
        if was_recording {
            self.nav.recording_grace = self.armed_recorder_count();
        }
        self.live_take_notes = 0;
        // A stop is a stop: a countdown still running dies with it.
        self.engine.transport.cancel_count_in();
        self.engine.panic();
    }


    /// Space+p — one door for play and pause, and the count-in's trigger.
    ///
    /// The count-in fires on exactly three conditions together: the setting
    /// is on (transport pane), the record switch is armed (space+r), and
    /// this press. Play without record armed just plays; record armed
    /// without a count-in set rolls immediately, as it always did. A press
    /// during the countdown cancels it and nothing starts.
    pub(crate) fn toggle_play_pause(&mut self) {
        use crate::debug_log as dbg;
        if self.engine.transport.is_counting_in() {
            self.engine.transport.cancel_count_in();
            dbg::system("play/pause → count-in cancelled");
            self.flash("count-in cancelled");
            self.log_transport_state();
            return;
        }
        if self.engine.transport.is_playing() {
            dbg::system("play/pause → stop playback");
            self.stop_playback();
        } else {
            if self.nav.loop_editor.enabled {
                let start = self.nav.loop_editor.start_ticks();
                dbg::system(&format!("play/pause → play from loop start (tick {start})"));
                self.engine.transport.set_position(start);
            } else {
                dbg::system("play/pause → play from current position");
            }
            self.sync_loop_to_transport();
            let bars = self.engine.transport.count_in_bars();
            if self.engine.transport.is_recording() && bars > 0 {
                self.engine.transport.begin_count_in(false);
                dbg::system(&format!("play/pause → count-in, {bars} bars"));
                self.flash(format!(
                    "count-in \u{b7} {bars} bar{}",
                    if bars == 1 { "" } else { "s" }
                ));
            } else {
                self.engine.transport.play();
            }
        }
        self.log_transport_state();
    }

    /// Clear the current track's clips where they overlap the loop range —
    /// the re-record mode's clean slate, taken as one undo step.
    fn clear_loop_range_clips(&mut self) {
        let track_idx = self.nav.track_cursor;
        let start = self.nav.loop_editor.start_ticks();
        let end = self.nav.loop_editor.end_ticks();
        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackClips { track_idx },
        );
        let Some(track) = self.nav.tracks.get_mut(track_idx) else { return };
        let audio_len = track.clips.len();
        let before = track.clips.len();
        track.clips.retain(|c| {
            let c_end = c.start_tick + c.length_ticks;
            !(c.start_tick < end && start < c_end)
        });
        let cleared = before - track.clips.len();
        if cleared == 0 {
            return;
        }
        for (i, c) in track.clips.iter_mut().enumerate() {
            c.number = i + 1;
        }
        self.nav.clamp_clip_selection(track_idx);
        self.nav.commit_undo(undo_before, "clear for re-record");
        self.resync_track_clips_to_audio(track_idx, audio_len);
        self.flash(format!(
            "re-record \u{00b7} cleared {cleared} clip{} (u restores)",
            if cleared == 1 { "" } else { "s" }
        ));
    }

    /// How many tracks will actually commit a take when recording stops:
    /// the armed ones the MIDI is routed to, which is zero or one. Counting
    /// every armed track here used to leave the grace counter holding
    /// credit for commits that never come, and a later stale snapshot could
    /// spend it and land as a phantom clip.
    pub(crate) fn armed_recorder_count(&self) -> usize {
        self.nav
            .tracks
            .iter()
            .filter(|t| {
                t.armed
                    && t.handle
                        .as_ref()
                        .is_some_and(|h| h.config.is_midi_active())
            })
            .count()
    }

    /// Move the tempo by whole BPM — the one door every tempo key goes
    /// through, so the change lands on the undo stack (one step per ride)
    /// and the [`NavState::tempo_bpm`] mirror moves in the same breath,
    /// which is what makes the mirror safe for undo to checkpoint from.
    pub(crate) fn nudge_tempo(&mut self, delta: f64) {
        let before = self.nav.undo_checkpoint(crate::state::undo::UndoScope::Tempo);
        let bpm = (self.engine.transport.tempo_bpm() + delta).max(20.0);
        self.engine.transport.set_tempo(bpm);
        self.nav.tempo_bpm = bpm as f32;
        crate::debug_log::system(&format!("bpm={:.0}", bpm));
        self.nav.commit_undo_coalesced(
            before,
            "tempo",
            crate::state::undo::UndoGesture::Tempo,
        );
    }

    /// One press on a loop brace handle, captured for undo and synced to
    /// the transport. The *range* is an edit; the loop's on/off switch is
    /// transport state and never comes through here.
    pub(crate) fn edit_loop_range(&mut self, edit: impl FnOnce(&mut crate::state::LoopEditor)) {
        let before = self.nav.undo_checkpoint(crate::state::undo::UndoScope::LoopRange);
        edit(&mut self.nav.loop_editor);
        self.nav.commit_undo_coalesced(
            before,
            "loop range",
            crate::state::undo::UndoGesture::LoopRange,
        );
        self.sync_loop_to_transport();
    }

    pub(crate) fn sync_loop_to_transport(&self) {
        use crate::debug_log as dbg;
        let le = &self.nav.loop_editor;
        self.engine.transport.set_loop_range(le.start_ticks(), le.end_ticks());
        if le.enabled != self.engine.transport.is_looping() {
            self.engine.transport.toggle_loop();
        }
        dbg::system(&format!(
            "loop sync: editor_enabled={} transport_looping={} range={}..{} ticks (bars {})",
            le.enabled, self.engine.transport.is_looping(),
            le.start_ticks(), le.end_ticks(), le.display(),
        ));
    }


    pub(crate) fn log_transport_state(&self) {
        use crate::debug_log as dbg;
        let t = &self.engine.transport;
        dbg::transport(
            t.is_playing(), t.is_recording(), t.is_looping(),
            t.position_ticks(), t.loop_start(), t.loop_end(),
        );
    }

    /// Toggle loop recording on the current track.
    /// First press: arms track, sets loop range, rewinds, starts record+play
    /// — or the count-in, when one is set.
    /// Second press: stops recording, commits clip.
    pub(crate) fn toggle_loop_record(&mut self) {
        // R during the countdown backs out of it, the same as play/pause.
        if self.engine.transport.is_counting_in() {
            self.engine.transport.cancel_count_in();
            self.flash("count-in cancelled");
            self.log_transport_state();
            return;
        }
        let is_recording = self.engine.transport.is_recording()
            && self.engine.transport.is_playing();

        if is_recording {
            self.engine.transport.stop_loop_record();
            self.nav.recording_grace = self.armed_recorder_count();
            self.live_take_notes = 0;
            self.engine.panic(); // silence all notes
        } else {
            // Make sure current track is armed and has a synth
            if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
                if !track.is_live() {
                    tracing::info!("Cannot record on a non-instrument track");
                    return;
                }
            } else {
                return;
            }

            // Arm the track if not already
            if let Some(track) = self.nav.tracks.get_mut(self.nav.track_cursor) {
                track.armed = true;
                track.sync_to_audio();
            }

            // Ensure this track is selected for MIDI
            self.nav.show_current_track_controls();

            // Sync loop range from editor to transport, then start
            self.sync_loop_to_transport();
            self.live_take_notes = 0;
            self.nav.take_count = 0;

            // Re-record: the loop range starts clean. The clear is an undo
            // step of its own, so `u` after a change of heart brings the
            // old material straight back.
            if self.nav.record_replace {
                self.clear_loop_range_clips();
            }

            // Punch in: the music is already rolling — recording joins it
            // where it is. No rewind, no count-in; the band was the count.
            if self.engine.transport.is_playing() {
                self.engine.transport.toggle_record();
                self.flash("punched in \u{00b7} recording");
                self.log_transport_state();
                return;
            }

            let bars = self.engine.transport.count_in_bars();
            if bars > 0 {
                // R is record intent and play in one press, so the count-in's
                // three conditions are all in hand; the countdown ends in the
                // full loop-record start, on the audio thread, on the bar.
                self.engine.transport.begin_count_in(true);
                self.flash(format!(
                    "count-in \u{b7} {bars} bar{}",
                    if bars == 1 { "" } else { "s" }
                ));
            } else {
                self.engine.transport.start_loop_record();
            }
            tracing::info!(
                "Loop recording started: bars {}..{} (ticks {}..{})",
                self.engine.transport.loop_start() / (Transport::PPQ * 4) + 1,
                self.engine.transport.loop_end() / (Transport::PPQ * 4),
                self.engine.transport.loop_start(),
                self.engine.transport.loop_end(),
            );
        }
    }
}
