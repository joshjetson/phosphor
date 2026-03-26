//! App methods: session io.

use super::*;

impl App {

    // ── Session save/load ──

    pub(crate) fn handle_save(&mut self) {
        if let Some(ref path) = self.session_path.clone() {
            // Quick save to existing path
            self.do_save(&path.display().to_string());
        } else {
            // First save — prompt for filename
            self.nav.input_modal.open_save("untitled.phos");
        }
    }


    pub(crate) fn do_save(&mut self, path_str: &str) {
        let path = std::path::PathBuf::from(path_str);
        // Ensure .phos extension
        let path = if path.extension().map(|e| e == "phos").unwrap_or(false) {
            path
        } else {
            path.with_extension("phos")
        };

        match crate::session::save(&path, &self.nav, &self.engine.transport) {
            Ok(()) => {
                self.session_path = Some(path.clone());
                self.status_message = Some((
                    format!("saved: {}", path.display()),
                    std::time::Instant::now(),
                ));
            }
            Err(e) => {
                self.status_message = Some((
                    format!("save failed: {e}"),
                    std::time::Instant::now(),
                ));
            }
        }
    }


    pub(crate) fn do_load(&mut self, path_str: &str) {
        let path = std::path::PathBuf::from(path_str);
        let session = match crate::session::load(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status_message = Some((
                    format!("open failed: {e}"),
                    std::time::Instant::now(),
                ));
                return;
            }
        };

        // Apply transport settings
        self.engine.transport.set_tempo(session.transport.tempo_bpm);
        if session.transport.metronome != self.engine.transport.is_metronome_on() {
            self.engine.transport.toggle_metronome();
        }
        self.nav.loop_editor.start_bar = session.transport.loop_start_bar;
        self.nav.loop_editor.end_bar = session.transport.loop_end_bar;
        self.nav.loop_editor.enabled = session.transport.loop_enabled;
        self.sync_loop_to_transport();

        // Remove existing instrument tracks (keep bus tracks)
        // Clear all instrument tracks from TUI state
        self.nav.tracks.retain(|t| t.instrument_type.is_none());
        self.nav.track_cursor = 0;
        self.nav.track_scroll = 0;
        self.nav.track_selected = false;
        self.nav.clip_view_visible = false;

        // Kill all sound
        self.engine.panic();

        // Recreate tracks from session
        for st in &session.tracks {
            let instrument = match crate::session::parse_instrument_type(&st.instrument_type) {
                Some(i) => i,
                None => continue, // skip unknown instruments
            };

            self.create_instrument_track(instrument);

            // Restore track state
            let track_idx = self.nav.track_cursor;
            if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                track.name = st.name.clone();
                track.muted = st.muted;
                track.soloed = st.soloed;
                track.armed = st.armed;
                track.volume = st.volume;
                track.color_index = st.color_index;

                // Restore synth params
                for (i, &val) in st.synth_params.iter().enumerate() {
                    if i < track.synth_params.len() {
                        track.synth_params[i] = val;
                    }
                }

                // Send all params to audio thread
                if let Some(mixer_id) = track.mixer_id {
                    for (i, &val) in track.synth_params.iter().enumerate() {
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetParameter {
                            track_id: mixer_id,
                            param_index: i,
                            value: val,
                        });
                    }
                }

                // Restore clips
                track.clips.clear();
                for sc in &st.clips {
                    let notes = crate::session::session_notes_to_snapshots(&sc.notes);
                    track.clips.push(crate::state::Clip {
                        number: track.clips.len() + 1,
                        width: 4, // will be recalculated by renderer
                        has_content: !notes.is_empty(),
                        start_tick: sc.start_tick,
                        length_ticks: sc.length_ticks,
                        notes,
                    });

                    // Send clip to audio thread: create then update events
                    if let Some(mixer_id) = track.mixer_id {
                        let clip_idx = track.clips.len() - 1;
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                            track_id: mixer_id,
                            start_tick: sc.start_tick,
                            length_ticks: sc.length_ticks,
                        });
                        let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                            &crate::session::session_notes_to_snapshots(&sc.notes),
                            sc.length_ticks,
                        );
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                            track_id: mixer_id,
                            clip_index: clip_idx,
                            events,
                        });
                    }
                }

                track.sync_to_audio();
            }
        }

        self.session_path = Some(path.clone());
        self.status_message = Some((
            format!("opened: {}", path.display()),
            std::time::Instant::now(),
        ));
    }
}
