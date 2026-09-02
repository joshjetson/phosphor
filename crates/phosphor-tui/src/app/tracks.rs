//! App methods: tracks.

use super::*;

impl App {

    pub(crate) fn handle_space_action(&mut self, action: SpaceAction) {
        match action {
            SpaceAction::Stop => {
                use crate::debug_log as dbg;
                dbg::system("stop → return to bar 1");
                self.stop_playback();
                self.engine.transport.stop();
                self.status_message = Some((
                    "stopped · back to bar 1".to_string(),
                    std::time::Instant::now(),
                ));
                self.log_transport_state();
            }
            SpaceAction::PlayPause => {
                self.toggle_play_pause();
            }
            SpaceAction::ToggleRecord => {
                use crate::debug_log as dbg;
                let was_recording = self.engine.transport.is_recording();
                self.engine.transport.toggle_record();
                let now_recording = self.engine.transport.is_recording();
                if was_recording && !now_recording {
                    self.nav.recording_grace = self.armed_recorder_count();
                }
                dbg::system(&format!("toggle record → recording={}", now_recording));
                self.log_transport_state();
            }
            SpaceAction::ToggleLoop => {
                use crate::debug_log as dbg;
                dbg::user("Space+l → focus loop editor");
                self.nav.loop_editor.focus();
            }
            SpaceAction::ToggleMetronome => {
                use crate::debug_log as dbg;
                self.engine.transport.toggle_metronome();
                dbg::system(&format!("metronome={}", self.engine.transport.is_metronome_on()));
            }
            SpaceAction::Panic => {
                self.engine.panic();
                tracing::info!("PANIC: all sound killed");
            }
            SpaceAction::AddInstrument => {
                self.nav.instrument_modal.open = true;
                self.nav.instrument_modal.cursor = 0;
            }
            SpaceAction::Save => {
                self.handle_save();
            }
            SpaceAction::Open => {
                self.nav.input_modal.open_load();
            }
            SpaceAction::Delete => {
                self.handle_delete_request();
            }
            SpaceAction::CycleTheme => {
                crate::theme::next_theme();
                self.status_message = Some((
                    format!("theme: {}", crate::theme::theme_name()),
                    std::time::Instant::now(),
                ));
            }
            SpaceAction::NewTrack => { /* future */ }
            SpaceAction::EditMode => {
                self.enter_edit_mode();
            }
            SpaceAction::Presets => {
                self.open_preset_browser();
            }
            SpaceAction::Quantize => {
                if self.nav.clip_view_target.is_some() {
                    let grid = self.nav.clip_view.piano_roll.grid;
                    self.nav.quantize_modal.open_with(grid);
                } else {
                    self.status_message = Some(("no clip selected".into(), std::time::Instant::now()));
                }
            }
        }
    }
    /// Stop playback and silence all instruments. Called on pause, stop,
    /// and stop-recording. Prevents notes from ringing after playback ends.

    /// Move the selected track's fader one press and report where it landed.
    ///
    /// The fader is the only makeup gain in the application, so where it is
    /// sitting is worth saying out loud: the header cell has three characters
    /// and rounds to the nearest dB, which is fine for reading at a glance
    /// and not enough to tell 0.75 (the default, −2.5 dB) from −2 dB exactly.
    pub(crate) fn step_fader(&mut self, steps: i32) {
        use crate::debug_log as dbg;

        let Some(volume) = self.nav.adjust_volume(steps) else { return };
        let text = if volume <= 0.0 {
            "fader: silent".to_string()
        } else {
            format!("fader: {:+.1} dB", 20.0 * volume.log10())
        };
        dbg::system(&text);
        self.status_message = Some((text, std::time::Instant::now()));
    }

    /// Create an instrument track in both the audio mixer and the TUI.
    ///
    /// The step sequencer is a choice in this menu but not an instrument: it
    /// makes an ordinary instrument track carrying its default child, with a
    /// pattern player in front of it. See [`phosphor_app::sequencer`].
    pub(crate) fn create_instrument_track(&mut self, instrument: InstrumentType) {
        use crate::debug_log as dbg;
        let sequencer = instrument.is_sequencer();
        let instrument = if sequencer {
            phosphor_app::sequencer::DEFAULT_CHILD
        } else {
            instrument
        };
        let track_id = self.next_track_id;
        self.next_track_id += 1;
        dbg::system(&format!("create_instrument_track: id={track_id} type={:?}", instrument));

        // Create shared handle
        let handle = Arc::new(TrackHandle::new(track_id, TrackKind::Instrument));
        handle.config.armed.store(true, Ordering::Relaxed);

        // Send AddTrack command to the audio mixer
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        });
        dbg::system("  AddTrack sent");

        // Send SetInstrument command based on selection
        dbg::system("  plugin created");
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetInstrument {
            track_id,
            instrument: build_plugin(instrument),
        });
        dbg::system("  SetInstrument sent");

        // Add to TUI track list with the handle wired in
        self.nav.add_instrument_track(instrument, track_id, handle);
        dbg::system(&format!("  track added to TUI, params_len={}", self.nav.tracks[self.nav.track_cursor].synth_params.len()));

        if sequencer {
            let state = phosphor_app::sequencer::SequencerState::new(instrument);
            for sync in self.nav.attach_sequencer(state) {
                let _ = self.engine.shared.mixer_command_tx.send(sync.command());
            }
            if let Some(track) = self.nav.current_track_mut() {
                track.name = "seq".into();
            }
            // Opened on its grid rather than on the child's panel: the
            // sequencer only became one after the track was made, and the
            // tab that was chosen then was chosen for a track without one.
            self.nav.show_current_track_controls();
            dbg::system("  sequencer attached");
        }
    }

    /// [`App::create_instrument_track`], as the player's own act: the new
    /// track goes on the undo stack. Session load and undo's rebuilds call
    /// the plain version — their tracks are not new work, and a load that
    /// pushed one "add track" per session track would bury the player's
    /// history under it.
    pub(crate) fn create_instrument_track_undoable(&mut self, instrument: InstrumentType) {
        use crate::state::undo::StateSlice;
        self.create_instrument_track(instrument);
        let idx = self.nav.track_cursor;
        let Some(track) = self.nav.tracks.get(idx) else { return };
        let saved = Box::new(track.clone());
        self.nav.push_undo_step(
            StateSlice::Track { track_idx: idx, track: None },
            StateSlice::Track { track_idx: idx, track: Some(saved) },
            "add track",
        );
    }

    /// Apply a rename typed into the input modal, as one undo step. Names
    /// are trimmed and capped to the width a strip can draw.
    pub(crate) fn do_rename_track(&mut self, name: &str) {
        let name: String = name.trim().chars().take(8).collect();
        if name.is_empty() {
            return;
        }
        let track_idx = self.nav.track_cursor;
        if self.nav.tracks.get(track_idx).is_none() {
            return;
        }
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackName { track_idx },
        );
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            track.name = name.clone();
        }
        self.nav.commit_undo(before, "rename track");
        self.flash(format!("track renamed \u{00b7} '{name}'"));
    }

    /// Duplicate the track under the cursor — instrument, whole panel,
    /// effects, mix position, clips, sequencer and all — directly below it.
    /// The layering gesture's other half: double the part, then swap the
    /// copy's patch or instrument until it is its own voice. One undo step.
    pub(crate) fn duplicate_current_track(&mut self) {
        use crate::state::undo::StateSlice;
        let src_idx = self.nav.track_cursor;
        let Some(src) = self.nav.tracks.get(src_idx) else { return };
        if !src.is_live() {
            self.flash("only instrument tracks duplicate");
            return;
        }
        let mut saved = src.clone();
        // A short name, like every track name here; the copy says it is one.
        saved.name = format!("{}2", saved.name.chars().take(4).collect::<String>());
        // The copy must not steal the original's audio identity or double
        // its record arm; materialize gives it fresh ones.
        saved.armed = false;

        let dest = src_idx + 1;
        self.materialize_track(dest, &saved);
        let Some(created) = self.nav.tracks.get(dest) else { return };
        self.nav.push_undo_step(
            StateSlice::Track { track_idx: dest, track: None },
            StateSlice::Track { track_idx: dest, track: Some(Box::new(created.clone())) },
            "duplicate track",
        );
        self.flash(format!("track duplicated \u{00b7} '{}'", self.nav.tracks[dest].name));
    }

    /// Take an instrument track out of the session — the UI row, the audio
    /// track, the cursor and clip view that pointed at it. One removal,
    /// shared by delete and by undo/redo, which are the same act reached
    /// from different keys.
    pub(crate) fn remove_instrument_track(&mut self, idx: usize) {
        use crate::debug_log as dbg;
        if idx >= self.nav.tracks.len() {
            return;
        }
        // The bus strips are furniture, not tracks; nothing removes them.
        if self.nav.tracks[idx].kind != TrackKind::Instrument {
            return;
        }
        if let Some(mid) = self.nav.tracks[idx].mixer_id {
            let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveTrack {
                track_id: mid,
            });
            dbg::system(&format!("removed track: mixer_id={}", mid));
        }
        self.nav.tracks.remove(idx);
        if self.nav.track_cursor >= self.nav.tracks.len() && self.nav.track_cursor > 0 {
            self.nav.track_cursor -= 1;
        }
        self.nav.track_selected = false;
        self.nav.clip_view_visible = false;
        self.nav.clip_view_target = None;
        // Undo can remove a track from modes the delete flow never reaches —
        // a locked fader, an open panel, edit mode. Every lock that pointed
        // into this track releases, or the keys keep routing to a ghost.
        self.nav.element_locked = false;
        self.nav.clip_view.sequencer.locked = false;
        self.nav.clip_view.fx.locked = false;
        self.nav.clip_view.piano_roll.edit_mode = false;
        self.nav.clip_view.piano_roll.highlight_locked = false;
        self.engine.panic();
    }

    /// Apply one sequencer edit to the track under the cursor, and send the
    /// audio thread whatever the edit made stale.
    ///
    /// Every key, menu item and controller message that touches a sequencer
    /// comes through here — see [`phosphor_app::sequencer::ops`] for why
    /// there is exactly one way in.
    ///
    /// The step grid's keys are the only caller with a keyboard behind them;
    /// the tests in `test_sequencer` drive the same route.
    pub(crate) fn sequencer_op(&mut self, op: phosphor_app::sequencer::ops::SeqOp) {
        use crate::state::undo::{UndoGesture, UndoScope};
        // Content ops land on the undo stack; cursor moves and the
        // performance controls never do — see `SeqOp::edits_content`. The
        // knob-like ops fold per sweep, so thirty presses on the swing are
        // one press of `u`. A child swap is bigger than the sequencer's own
        // scope — it replaces the instrument and its whole panel — so it is
        // captured whole under a scope of its own.
        let track_idx = self.nav.track_cursor;
        let is_child_swap = matches!(op, phosphor_app::sequencer::ops::SeqOp::SetChild(_));
        let undo_before = if is_child_swap {
            Some(self.nav.undo_checkpoint(UndoScope::SeqChild { track_idx }))
        } else if op.edits_content() {
            Some(self.nav.undo_checkpoint(UndoScope::Sequencer { track_idx }))
        } else {
            None
        };

        let (effect, syncs) = self.nav.sequencer_op(op);
        if effect.child {
            self.reload_child_instrument(track_idx);
        }
        for sync in syncs {
            let _ = self.engine.shared.mixer_command_tx.send(sync.command());
        }

        if let Some(before) = undo_before {
            if is_child_swap {
                // The child knob walks a list; a flick through five
                // instruments is one step back to the one the player left.
                self.nav.commit_undo_coalesced(
                    before,
                    "child instrument",
                    UndoGesture::ChildSwap { track_idx },
                );
            } else if op.is_sweep() {
                self.nav.commit_undo_coalesced(
                    before,
                    op.undo_label(),
                    UndoGesture::Sequencer { track_idx },
                );
            } else {
                self.nav.commit_undo(before, op.undo_label());
            }
        }
    }

    /// Put a track's child instrument in its plugin slot, with its whole
    /// panel behind it.
    pub(crate) fn reload_child_instrument(&mut self, track_index: usize) {
        let Some(track) = self.nav.tracks.get(track_index) else { return };
        let (Some(instrument), Some(track_id)) = (track.instrument_type, track.mixer_id) else {
            return;
        };
        let params = track.synth_params.clone();
        let tx = &self.engine.shared.mixer_command_tx;
        let _ = tx.send(MixerCommand::SetInstrument {
            track_id,
            instrument: build_plugin(instrument),
        });
        for (index, &value) in params.iter().enumerate() {
            let _ = tx.send(MixerCommand::SetParameter {
                track_id,
                param_index: index,
                value,
            });
        }
    }

    // ── Inserts ──

    /// Take whatever the FX menu's cursor is on and put it in the chain.
    ///
    /// The UI's mirror and the audio thread's chain move together or not at
    /// all: the effect is built here, on this thread, and handed over as a
    /// command — the same shape `SetInstrument` has, and for the same reason.
    /// Nothing is ever built on the audio thread.
    pub(crate) fn fx_menu_choose(&mut self) {
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackFx { track_idx: self.nav.track_cursor },
        );
        let outcome = self.nav.fx_menu_select();
        self.apply_fx_add(outcome);
        // A refusal — chain full, effect not built — changed nothing, and
        // the commit's own no-op check knows it.
        self.nav.commit_undo(before, "add effect");
    }

    /// Send an added effect to the audio thread, or say why there is none.
    pub(crate) fn apply_fx_add(&mut self, outcome: phosphor_app::state::FxAdd) {
        use phosphor_app::state::FxAdd;
        let message = match outcome {
            FxAdd::Added { target, slot, fx_type, effect } => {
                let _ = self
                    .engine
                    .shared
                    .mixer_command_tx
                    .send(MixerCommand::AddFx { target, slot, effect });
                // Where it landed is worth saying: an effect is inserted at
                // its canonical place in the chain rather than appended, so
                // "added reverb" alone would leave the player looking for it
                // at the bottom.
                format!("{} added at slot {}", fx_type.label(), slot + 1)
            }
            FxAdd::ChainFull => format!(
                "chain is full ({} slots) \u{b7} remove one first",
                phosphor_core::fx::MAX_FX_SLOTS
            ),
            FxAdd::NotBuilt(fx_type) => format!("{} is not built yet", fx_type.label()),
            FxAdd::Nothing => return,
        };
        crate::debug_log::log("FX", &message);
        self.status_message = Some((message, std::time::Instant::now()));
    }

    /// Put a whole chain on a strip, on both sides.
    ///
    /// Used by the session loader: the mirror is already in place, and this
    /// sends the audio thread the effects to match it. A slot whose effect
    /// this build cannot make is dropped from the mirror too, so the two
    /// never disagree about which slot is which.
    pub(crate) fn install_chain(&mut self, track_index: usize) {
        let Some(track) = self.nav.tracks.get(track_index) else { return };
        let Some(target) = track.fx_target() else { return };
        let chain = track.fx_chain.clone();

        let mut installed = 0usize;
        let mut missing = 0usize;
        let mut meters: Vec<Option<std::sync::Arc<phosphor_core::fx::GrMeter>>> =
            Vec::with_capacity(chain.len());
        for slot in &chain {
            let Some(mut effect) = phosphor_app::fx::build(slot.fx_type) else {
                missing += 1;
                continue;
            };
            for (index, &value) in slot.params.iter().enumerate() {
                effect.set_parameter(index, value);
            }
            // The mirror's meter follows the effect that is going into the
            // slot. Without this a chain that was pasted, reloaded or
            // reinstalled would leave the panel watching an effect that is no
            // longer in the signal path — a gain-reduction bar that never
            // moves, which reads as a broken compressor.
            meters.push(effect.gr_meter());
            let tx = &self.engine.shared.mixer_command_tx;
            let _ = tx.send(MixerCommand::AddFx { target, slot: installed, effect });
            if slot.bypass {
                let _ = tx.send(MixerCommand::SetFxBypass {
                    target,
                    slot: installed,
                    bypass: true,
                });
            }
            installed += 1;
        }

        if missing > 0 {
            // The mirror keeps only what actually reached the audio thread.
            // A slot drawn on screen that is not in the signal path is worse
            // than a missing slot, because it looks like it is working.
            if let Some(track) = self.nav.tracks.get_mut(track_index) {
                track.fx_chain.retain(|slot| phosphor_app::fx::is_built(slot.fx_type));
            }
        }

        // The meters, in the order the slots that survived went out.
        if let Some(track) = self.nav.tracks.get_mut(track_index) {
            for (slot, meter) in track.fx_chain.iter_mut().zip(meters) {
                slot.gr = meter;
            }
        }

        if missing > 0 {
            let message = format!(
                "{missing} effect{} in this session {} not in this build",
                if missing == 1 { "" } else { "s" },
                if missing == 1 { "is" } else { "are" },
            );
            crate::debug_log::log("FX", &message);
            self.status_message = Some((message, std::time::Instant::now()));
        }
    }

    /// Take every effect off a strip, on both sides.
    ///
    /// Slots are removed from the end so that the indices of the ones still
    /// to go do not move underneath the commands already queued.
    pub(crate) fn clear_chain(&mut self, track_index: usize) {
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(target) = track.fx_target() else { return };
        let count = track.fx_chain.len();
        track.fx_chain.clear();
        for slot in (0..count).rev() {
            let _ = self
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::RemoveFx { target, slot });
        }
    }

    /// Move one control on one effect, on both sides.
    ///
    /// In the control's own unit — decibels, hertz, milliseconds — because
    /// that is what the insert layer's parameters are and what the mirror
    /// stores. The mirror is written first and the command sent from what it
    /// then holds, so the two cannot disagree about what was set even if the
    /// caller hands over something out of range: whatever the effect clamps
    /// it to is the effect's business, and both sides asked for the same
    /// thing.
    ///
    /// Silently does nothing for a slot that is not there. A panel drawing a
    /// chain it has just edited is one frame behind the edit often enough
    /// that a panic here would be a crash rather than a bug report.
    ///
    /// The panel's keys are the only caller with a keyboard behind them —
    /// see `app::fx_keys` — and the integration tests drive the same route.
    ///
    /// One knob movement, one (coalesced) undo step: a sweep of the same
    /// slot's controls folds into a single step, so `u` takes the player
    /// back to where the sweep began rather than one tick along it.
    pub(crate) fn set_fx_param(&mut self, track_index: usize, slot: usize, param: usize, value: f32) {
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackFx { track_idx: track_index },
        );
        self.write_fx_param(track_index, slot, param, value);
        self.nav.commit_undo_coalesced(
            before,
            "adjust effect",
            crate::state::undo::UndoGesture::FxSlot { track_idx: track_index, slot },
        );
    }

    /// [`Self::set_fx_param`] without the undo step — the half that undo
    /// itself applies a captured chain through.
    pub(crate) fn write_fx_param(&mut self, track_index: usize, slot: usize, param: usize, value: f32) {
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(target) = track.fx_target() else { return };
        let Some(instance) = track.fx_chain.get_mut(slot) else { return };
        if let Some(stored) = instance.params.get_mut(param) {
            *stored = value;
        }
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetFxParam { target, slot, param, value });
    }

    /// Throw a slot's bypass switch, on both sides. The audio thread
    /// crossfades it; the mirror is what the strip and the session read.
    ///
    /// Thrown from the chain list and from the panel, through the same door
    /// as [`Self::set_fx_param`]. A discrete step, never coalesced: two
    /// throws are two changes of mind, not one sweep.
    pub(crate) fn set_fx_bypass(&mut self, track_index: usize, slot: usize, bypass: bool) {
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackFx { track_idx: track_index },
        );
        self.write_fx_bypass(track_index, slot, bypass);
        self.nav.commit_undo(before, "bypass effect");
    }

    /// [`Self::set_fx_bypass`] without the undo step.
    pub(crate) fn write_fx_bypass(&mut self, track_index: usize, slot: usize, bypass: bool) {
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(target) = track.fx_target() else { return };
        let Some(instance) = track.fx_chain.get_mut(slot) else { return };
        instance.bypass = bypass;
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetFxBypass { target, slot, bypass });
    }

    /// Push a strip's pan, sends and sidechain key to the audio thread.
    pub(crate) fn sync_routing(&self, track_index: usize) {
        let Some(track) = self.nav.tracks.get(track_index) else { return };
        let Some(track_id) = track.mixer_id else { return };
        let tx = &self.engine.shared.mixer_command_tx;
        let _ = tx.send(MixerCommand::SetPan { track_id, pan: track.pan });
        for slot in phosphor_core::fx::SendSlot::ALL {
            let _ = tx.send(MixerCommand::SetSendLevel {
                track_id,
                send: slot,
                gain: track.send(slot),
            });
        }
        let _ = tx.send(MixerCommand::SetKeySource {
            track_id,
            source: track.key_source,
        });
    }

    // ── Delete ──

}

/// The plugin behind an instrument type.
///
/// One factory, because there were about to be two: a track being created
/// builds one, and a sequencer track being pointed at a different child
/// builds another. Two lists of instruments is one list that eventually
/// forgets an instrument.
///
/// The sequencer has no plugin — it drives one — and answers with the
/// phosphor synth so that the slot is never left empty; nothing reaches this
/// with it, because a sequencer track carries its child's type.
pub(crate) fn build_plugin(
    instrument: InstrumentType,
) -> Box<dyn phosphor_plugin::Plugin + Send> {
    match instrument {
        InstrumentType::Synth | InstrumentType::Sampler | InstrumentType::Sequencer => {
            Box::new(PhosphorSynth::new())
        }
        InstrumentType::DrumRack => Box::new(phosphor_dsp::drum_rack::DrumRack::new()),
        InstrumentType::DX7 => Box::new(phosphor_dsp::dx7::Dx7Synth::new()),
        InstrumentType::Jupiter8 => Box::new(phosphor_dsp::jupiter::Jupiter8Synth::new()),
        InstrumentType::Prophet6 => Box::new(phosphor_dsp::prophet6::Prophet6::new()),
        InstrumentType::Teo5 => Box::new(phosphor_dsp::teo5::Teo5::new()),
        InstrumentType::Odyssey => Box::new(phosphor_dsp::odyssey::OdysseySynth::new()),
        InstrumentType::Juno60 => Box::new(phosphor_dsp::juno::Juno60Synth::new()),
        InstrumentType::Rhodes => Box::new(phosphor_dsp::rhodes::RhodesPiano::new()),
        InstrumentType::LittlePhatty => Box::new(phosphor_dsp::phatty::LittlePhatty::new()),
    }
}
