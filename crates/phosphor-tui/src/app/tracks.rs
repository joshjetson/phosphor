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
                use crate::debug_log as dbg;
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
                    self.engine.transport.play();
                }
                self.log_transport_state();
            }
            SpaceAction::ToggleRecord => {
                use crate::debug_log as dbg;
                let was_recording = self.engine.transport.is_recording();
                self.engine.transport.toggle_record();
                let now_recording = self.engine.transport.is_recording();
                if was_recording && !now_recording {
                    self.nav.recording_grace = self.nav.tracks.iter().filter(|t| t.armed).count();
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
        let (effect, syncs) = self.nav.sequencer_op(op);
        if effect.child {
            self.reload_child_instrument();
        }
        for sync in syncs {
            let _ = self.engine.shared.mixer_command_tx.send(sync.command());
        }
    }

    /// Put the current track's child instrument in its plugin slot, with its
    /// whole panel behind it.
    fn reload_child_instrument(&mut self) {
        let Some(track) = self.nav.current_track() else { return };
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
        InstrumentType::Odyssey => Box::new(phosphor_dsp::odyssey::OdysseySynth::new()),
        InstrumentType::Juno60 => Box::new(phosphor_dsp::juno::Juno60Synth::new()),
        InstrumentType::Rhodes => Box::new(phosphor_dsp::rhodes::RhodesPiano::new()),
        InstrumentType::LittlePhatty => Box::new(phosphor_dsp::phatty::LittlePhatty::new()),
    }
}
