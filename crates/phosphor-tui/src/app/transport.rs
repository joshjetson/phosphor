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
            self.nav.recording_grace = self.nav.tracks.iter().filter(|t| t.armed).count();
        }
        self.live_take_notes = 0;
        self.engine.panic();
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
    /// First press: arms track, sets loop range, rewinds, starts record+play.
    /// Second press: stops recording, commits clip.

    /// Toggle loop recording on the current track.
    /// First press: arms track, sets loop range, rewinds, starts record+play.
    /// Second press: stops recording, commits clip.
    pub(crate) fn toggle_loop_record(&mut self) {
        let is_recording = self.engine.transport.is_recording()
            && self.engine.transport.is_playing();

        if is_recording {
            self.engine.transport.stop_loop_record();
            self.nav.recording_grace = self.nav.tracks.iter().filter(|t| t.armed).count();
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
            self.engine.transport.start_loop_record();
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
