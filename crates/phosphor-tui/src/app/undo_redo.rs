//! App methods: undo redo.
//!
//! The stack holds [`UndoStep`]s — before/after captures of one slice of
//! state (see `phosphor_app::state::undo`). Undo applies a step's `before`,
//! redo applies its `after`, and [`App::apply_slice`] is the one routine
//! that does either: it cannot tell which direction it is working in, so
//! there is no inverse logic to write and none to get wrong. The first
//! version of this file wrote every inverse by hand, and three kinds of
//! redo were "not available" because nobody had.
//!
//! While the transport is recording, `u` means what it means on a looper:
//! peel the newest layer and keep rolling — see
//! [`App::undo_while_recording`].

use super::*;
use crate::state::undo::StateSlice;

impl App {

    // ── Undo / Redo ──

    pub(crate) fn perform_undo(&mut self) {
        use crate::debug_log as dbg;
        if self.undo_while_recording() {
            return;
        }
        let Some(step) = self.nav.undo_stack.pop_undo() else {
            dbg::system("undo: stack empty");
            self.flash("nothing to undo");
            return;
        };
        dbg::system(&format!("undo: {}", step.label));
        self.apply_slice(&step.before);
        self.flash(format!("undo: {}", step.label));
        self.nav.undo_stack.push_redo(step);
    }

    pub(crate) fn perform_redo(&mut self) {
        use crate::debug_log as dbg;
        let Some(step) = self.nav.undo_stack.pop_redo() else {
            self.flash("nothing to redo");
            return;
        };
        dbg::system(&format!("redo: {}", step.label));
        self.apply_slice(&step.after);
        self.flash(format!("redo: {}", step.label));
        self.nav.undo_stack.push_undo_only(step);
    }

    /// Put a brief line on the status bar.
    pub(crate) fn flash(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), std::time::Instant::now()));
    }

    /// Checkpoint the clip list of the track the clip view is looking at —
    /// `None` when it is looking at nothing, which is also when the piano
    /// roll's edits have nothing to land on. Pair with
    /// [`App::commit_viewed_track`].
    pub(crate) fn checkpoint_viewed_track(&self) -> Option<StateSlice> {
        let (track_idx, _) = self.nav.clip_view_target?;
        Some(self.nav.undo_checkpoint(crate::state::undo::UndoScope::TrackClips { track_idx }))
    }

    pub(crate) fn commit_viewed_track(
        &mut self,
        before: Option<StateSlice>,
        label: &'static str,
    ) {
        if let Some(before) = before {
            self.nav.commit_undo(before, label);
        }
    }

    /// [`Self::commit_viewed_track`] for a continuous gesture — an
    /// automation sweep — so a ramp drawn across many columns folds into
    /// one undo step.
    pub(crate) fn commit_viewed_track_coalesced(
        &mut self,
        before: Option<StateSlice>,
        label: &'static str,
        gesture: crate::state::undo::UndoGesture,
    ) {
        if let Some(before) = before {
            self.nav.commit_undo_coalesced(before, label, gesture);
        }
    }

    // ── Undo while the transport is recording ──

    /// The looper's undo: peel the newest layer, keep rolling.
    ///
    /// The layers, newest first, are the uncommitted pass in the recorder's
    /// buffer and then the committed takes on the undo stack. A pass with
    /// notes in it is scrapped with [`MixerCommand::DiscardRecording`] — the
    /// audio thread empties the buffer and keeps recording. An empty pass
    /// means the newest layer is the last committed take, which comes off
    /// the stack like any other step.
    ///
    /// The one refinement is the wrap grace: in loop overdub the player does
    /// not hear a flub until it comes round again — *after* it committed —
    /// so by the time they press `u` they are usually a beat into the next
    /// pass, often with the first note of the fix already played. Just past
    /// the wrap, those notes are the fix, not the mistake: the take is
    /// peeled and the young pass is left alone.
    ///
    /// `u` here reaches takes and only takes. Edits made before the
    /// recording are behind a wall until the transport stops — a hot `u`
    /// pressed once too often must not eat the quantize that happened
    /// before the take.
    ///
    /// Returns true when the press was handled here.
    fn undo_while_recording(&mut self) -> bool {
        let transport = &self.engine.transport;
        if !(transport.is_recording() && transport.is_playing()) {
            return false;
        }
        if !self.nav.tracks.iter().any(|t| t.armed && t.is_live()) {
            return false;
        }

        if self.live_take_notes > 0 && !self.in_wrap_grace() {
            let _ = self
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::DiscardRecording);
            self.live_take_notes = 0;
            crate::debug_log::system("undo: discarded in-flight pass");
            self.flash("undo: pass discarded · still recording");
            return true;
        }

        if self.nav.undo_stack.top_is_take() {
            let step = self.nav.undo_stack.pop_undo().expect("top_is_take saw one");
            self.apply_slice(&step.before);
            self.nav.undo_stack.push_redo(step);
            crate::debug_log::system("undo: peeled last take");
            self.flash("undo: take removed · still recording");
        } else {
            self.flash("nothing recorded to undo · stop to undo edits");
        }
        true
    }

    /// Whether the playhead is within the first eighth of the loop — the
    /// window where in-flight notes read as the started fix rather than as
    /// the mistake. Never true when not looping: a linear recording has no
    /// wrap for a flub to come back around.
    fn in_wrap_grace(&self) -> bool {
        let transport = &self.engine.transport;
        if !transport.is_looping() {
            return false;
        }
        let len = transport.loop_end() - transport.loop_start();
        len > 0 && (transport.position_ticks() - transport.loop_start()) * 8 < len
    }

    // ── Applying a slice ──

    /// Make the application match a captured slice — state, selection,
    /// audio thread. Direction-blind: undo hands it a `before`, redo an
    /// `after`, and nothing here knows the difference.
    fn apply_slice(&mut self, slice: &StateSlice) {
        match slice {
            StateSlice::TrackClips { track_idx, clips } => {
                self.apply_clips_slice(*track_idx, clips);
            }
            StateSlice::Track { track_idx, track } => match track {
                Some(saved) => self.restore_track_from_slice(*track_idx, saved),
                None => self.remove_instrument_track(*track_idx),
            },
            StateSlice::TrackFx { track_idx, chain } => {
                self.apply_fx_slice(*track_idx, chain);
            }
            StateSlice::TrackMidiFx { track_idx, chain } => {
                let chain = chain.clone();
                self.apply_midi_fx_slice(*track_idx, &chain);
            }
            StateSlice::ClipsAndMidiFx { track_idx, clips, chain } => {
                self.apply_clips_slice(*track_idx, clips);
                let chain = chain.clone();
                self.apply_midi_fx_slice(*track_idx, &chain);
            }
            StateSlice::SynthParams { track_idx, params } => {
                if let Some(track) = self.nav.tracks.get_mut(*track_idx) {
                    track.synth_params = params.clone();
                }
                self.push_params_to_audio(*track_idx);
            }
            StateSlice::Sequencer { track_idx, content } => {
                if let Some(content) = content {
                    for sync in self.nav.restore_sequencer_content(*track_idx, content) {
                        let _ = self.engine.shared.mixer_command_tx.send(sync.command());
                    }
                }
            }
            StateSlice::SeqChild { track_idx, instrument, params, content } => {
                if let Some(track) = self.nav.tracks.get_mut(*track_idx) {
                    track.instrument_type = *instrument;
                    track.synth_params = params.clone();
                }
                // The plugin slot follows the track's word, the same road a
                // live child swap takes.
                self.reload_child_instrument(*track_idx);
                if let Some(content) = content {
                    for sync in self.nav.restore_sequencer_content(*track_idx, content) {
                        let _ = self.engine.shared.mixer_command_tx.send(sync.command());
                    }
                }
                // A panel cursor deeper than the restored child's panel is
                // pointing at a control that no longer exists.
                let len = params.len();
                if len > 0 && self.nav.clip_view.synth_param_cursor >= len {
                    self.nav.clip_view.synth_param_cursor = len - 1;
                }
            }
            StateSlice::TrackMix { track_idx, volume, pan, sends, muted } => {
                if let Some(track) = self.nav.tracks.get_mut(*track_idx) {
                    track.volume = *volume;
                    track.pan = *pan;
                    track.sends = *sends;
                    track.muted = *muted;
                    // Volume and mute ride the handle's atomics; pan and the
                    // sends go as commands. Same two roads every fader press
                    // takes.
                    track.sync_to_audio();
                }
                self.sync_routing(*track_idx);
            }
            StateSlice::TrackName { track_idx, name } => {
                if let Some(track) = self.nav.tracks.get_mut(*track_idx) {
                    track.name = name.clone();
                }
            }
            StateSlice::Tempo { bpm } => {
                self.engine.transport.set_tempo(f64::from(*bpm));
                self.nav.tempo_bpm = *bpm;
            }
            StateSlice::LoopRange { start_bar, end_bar } => {
                self.nav.loop_editor.start_bar = *start_bar;
                self.nav.loop_editor.end_bar = *end_bar;
                self.sync_loop_to_transport();
            }
        }
    }

    /// Put a captured insert chain back on a strip.
    ///
    /// Two roads, chosen by whether the chain's *shape* — which effects, in
    /// which order — matches what is installed. Same shape means the step
    /// was knobs and switches, and the differences are written through the
    /// same calls the panel uses: the effects keep running and a delay's
    /// tail survives its own knob being undone. A different shape rebuilds
    /// the chain the way a session load does.
    fn apply_fx_slice(&mut self, track_idx: usize, chain: &[crate::state::FxInstance]) {
        let same_shape = self.nav.tracks.get(track_idx).is_some_and(|t| {
            t.fx_chain.len() == chain.len()
                && t.fx_chain.iter().zip(chain).all(|(a, b)| a.fx_type == b.fx_type)
        });

        if same_shape {
            let mut params = Vec::new();
            let mut switches = Vec::new();
            if let Some(track) = self.nav.tracks.get(track_idx) {
                for (slot, wanted) in chain.iter().enumerate() {
                    let current = &track.fx_chain[slot];
                    for (param, &value) in wanted.params.iter().enumerate() {
                        if current.params.get(param).copied() != Some(value) {
                            params.push((slot, param, value));
                        }
                    }
                    if current.bypass != wanted.bypass {
                        switches.push((slot, wanted.bypass));
                    }
                }
            }
            for (slot, param, value) in params {
                self.write_fx_param(track_idx, slot, param, value);
            }
            for (slot, bypass) in switches {
                self.write_fx_bypass(track_idx, slot, bypass);
            }
            return;
        }

        self.clear_chain(track_idx);
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            track.fx_chain = chain.to_vec();
        }
        self.install_chain(track_idx);

        // The cursor and any open panel follow the chain that now exists.
        let len = self.nav.tracks.get(track_idx).map(|t| t.fx_chain.len()).unwrap_or(0);
        self.nav.clip_view.fx_cursor = self.nav.clip_view.fx_cursor.min(len.saturating_sub(1));
        if let Some(open) = self.nav.clip_view.fx.slot {
            if open >= len {
                self.nav.clip_view.fx.close();
                self.nav.clip_view.clip_tab = crate::state::ClipTab::InstConfig;
                self.nav.clip_view.focus = crate::state::ClipViewFocus::FxPanel;
            }
        }
    }

    fn apply_clips_slice(&mut self, track_idx: usize, clips: &[crate::state::Clip]) {
        let Some(track) = self.nav.tracks.get_mut(track_idx) else { return };
        // What the audio thread holds right now is what the UI held a moment
        // ago — the resync needs that count to clear it fully.
        let audio_clip_count = track.clips.len();
        track.clips = clips.to_vec();
        self.nav.clamp_clip_selection(track_idx);
        self.nav.sync_clip_view_target();
        self.resync_track_clips_to_audio(track_idx, audio_clip_count);
    }

    /// Rebuild a track from its captured state.
    ///
    /// The capture's `mixer_id` and handle are a dead track's — the audio
    /// thread freed that track when it was deleted — so the rebuild goes
    /// through [`App::create_instrument_track`] for a live identity and then
    /// lays the saved state over it: fields, parameters, clips, inserts,
    /// routing, sequencer. The same order a session load uses, because it is
    /// the same job.
    fn restore_track_from_slice(&mut self, track_idx: usize, saved: &crate::state::TrackState) {
        self.materialize_track(track_idx, saved);
    }

    /// Build a live track from a captured [`TrackState`] at `track_idx` —
    /// the shared machinery under undo's track restore and the duplicate
    /// gesture, which are the same job: a dead state made to sound again
    /// with a fresh audio identity.
    pub(crate) fn materialize_track(&mut self, track_idx: usize, saved: &crate::state::TrackState) {
        use crate::debug_log as dbg;
        let Some(instrument) = saved.instrument_type else { return };
        self.create_instrument_track(instrument);

        // The new track went in at the end of the instruments; the saved one
        // may have lived higher up. Put it back where it was so undo does
        // not shuffle the running order.
        let created_idx = self.nav.track_cursor;
        let final_idx = track_idx.min(created_idx);
        if final_idx != created_idx {
            let track = self.nav.tracks.remove(created_idx);
            self.nav.tracks.insert(final_idx, track);
        }
        self.nav.track_cursor = final_idx;

        if let Some(track) = self.nav.tracks.get_mut(final_idx) {
            track.name = saved.name.clone();
            track.muted = saved.muted;
            track.soloed = saved.soloed;
            track.armed = saved.armed;
            track.volume = saved.volume;
            track.color_index = saved.color_index;
            track.pan = saved.pan;
            track.sends = saved.sends;
            track.synth_params = saved.synth_params.clone();
            track.clips = saved.clips.clone();
            track.fx_chain = saved.fx_chain.clone();
            track.midi_fx = saved.midi_fx.clone();
            track.sync_to_audio();
        }
        self.push_params_to_audio(final_idx);

        // The freshly created track has no clips on the audio side yet.
        self.resync_track_clips_to_audio(final_idx, 0);
        self.install_chain(final_idx);
        self.install_midi_fx(final_idx);
        self.sync_routing(final_idx);

        if let Some(sequencer) = saved.sequencer.clone() {
            for sync in self.nav.attach_sequencer(*sequencer) {
                let _ = self.engine.shared.mixer_command_tx.send(sync.command());
            }
        }

        // Its own key first, then anyone who was keying off it.
        if let Some(source) = saved.key_source {
            let live = self.nav.tracks.iter().any(|t| t.mixer_id == Some(source));
            if live {
                self.set_key_source(final_idx, Some(source));
            }
        }
        self.reattach_dangling_keys(final_idx);

        dbg::system(&format!(
            "track restored: '{}' at {} with {} clips",
            saved.name, final_idx, saved.clips.len()
        ));
    }

    /// Point any dangling sidechain key that named this track back at it.
    ///
    /// A key is stored as a track *identity*, and a track that comes back
    /// from the undo stack comes back with a new one — so the key would stay
    /// broken for the rest of the session without this. The name is only ever
    /// used here, only for keys that no longer resolve to anything, and only
    /// against the name the key was set from; a key that still points at a
    /// live track is left exactly where it is.
    fn reattach_dangling_keys(&mut self, restored: usize) {
        let Some((restored_id, name)) = self
            .nav
            .tracks
            .get(restored)
            .and_then(|t| Some((t.mixer_id?, t.name.clone())))
        else {
            return;
        };
        let live: Vec<usize> = self.nav.tracks.iter().filter_map(|t| t.mixer_id).collect();
        let repaired: Vec<usize> = self
            .nav
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.key_source.is_some_and(|id| !live.contains(&id))
                    && t.key_source_name.as_deref() == Some(name.as_str())
            })
            .map(|(index, _)| index)
            .collect();
        for index in repaired {
            self.set_key_source(index, Some(restored_id));
        }
    }
}
