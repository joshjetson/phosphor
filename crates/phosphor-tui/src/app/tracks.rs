//! App methods: tracks.

use super::*;

impl App {

    pub(crate) fn handle_space_action(&mut self, action: SpaceAction) {
        match action {
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
                self.engine.transport.toggle_record();
                dbg::system(&format!("toggle record → recording={}", self.engine.transport.is_recording()));
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
        }
    }
    /// Stop playback and silence all instruments. Called on pause, stop,
    /// and stop-recording. Prevents notes from ringing after playback ends.

    /// Create an instrument track in both the audio mixer and the TUI.
    pub(crate) fn create_instrument_track(&mut self, instrument: InstrumentType) {
        use crate::debug_log as dbg;
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
        let plugin: Box<dyn phosphor_plugin::Plugin + Send> = match instrument {
            InstrumentType::Synth | InstrumentType::Sampler => Box::new(PhosphorSynth::new()),
            InstrumentType::DrumRack => Box::new(phosphor_dsp::drum_rack::DrumRack::new()),
            InstrumentType::DX7 => Box::new(phosphor_dsp::dx7::Dx7Synth::new()),
            InstrumentType::Jupiter8 => Box::new(phosphor_dsp::jupiter::Jupiter8Synth::new()),
            InstrumentType::Odyssey => Box::new(phosphor_dsp::odyssey::OdysseySynth::new()),
            InstrumentType::Juno60 => Box::new(phosphor_dsp::juno::Juno60Synth::new()),
        };
        dbg::system("  plugin created");
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetInstrument {
            track_id,
            instrument: plugin,
        });
        dbg::system("  SetInstrument sent");

        // Add to TUI track list with the handle wired in
        self.nav.add_instrument_track(instrument, track_id, handle);
        dbg::system(&format!("  track added to TUI, params_len={}", self.nav.tracks[self.nav.track_cursor].synth_params.len()));
    }

    // ── Delete ──

}
