//! NavState methods: track ops.

use super::*;

impl NavState {

    pub(crate) fn toggle_mute(&mut self) {
        if let Some(t) = self.current_track_mut() {
            t.muted = !t.muted;
            t.sync_to_audio();
        }
    }


    pub(crate) fn toggle_solo(&mut self) {
        if let Some(t) = self.current_track_mut() {
            t.soloed = !t.soloed;
            t.sync_to_audio();
        }
    }


    pub(crate) fn toggle_arm(&mut self) {
        if let Some(t) = self.current_track_mut() {
            t.armed = !t.armed;
            t.sync_to_audio();
        }
    }


    pub(crate) fn digit_input(&mut self, ch: char) {
        if self.focused_pane == Pane::Tracks && self.track_selected {
            self.number_buf.push_digit(ch);
        }
    }


    pub(crate) fn tick(&mut self) {
        if let Some(clip_num) = self.number_buf.check_timeout() {
            self.jump_to_clip(clip_num);
        }
    }


    pub(crate) fn jump_to_clip(&mut self, clip_number: usize) {
        if let Some(track) = self.current_track() {
            if let Some(idx) = track.clips.iter().position(|c| c.number == clip_number) {
                self.track_element = TrackElement::Clip(idx);
                self.open_clip_view(self.track_cursor, idx);
            }
        }
    }


    pub(crate) fn activate_element(&mut self) {
        match self.track_element {
            TrackElement::Mute => self.toggle_mute(),
            TrackElement::Solo => self.toggle_solo(),
            TrackElement::RecordArm => self.toggle_arm(),
            TrackElement::Fx => {
                self.fx_menu.open = true;
                self.fx_menu.cursor = 0;
            }
            TrackElement::Volume => { /* future: volume slider */ }
            TrackElement::Clip(idx) => {
                self.clip_locked = true;
                self.open_clip_view(self.track_cursor, idx);
                self.clip_view.clip_tab = ClipTab::PianoRoll;
                self.clip_view.focus = ClipViewFocus::PianoRoll;
                crate::debug_log::system(&format!(
                    "clip locked: track={} clip={} start={} len={}",
                    self.track_cursor, idx,
                    self.tracks.get(self.track_cursor).and_then(|t| t.clips.get(idx)).map(|c| c.start_tick).unwrap_or(-1),
                    self.tracks.get(self.track_cursor).and_then(|t| t.clips.get(idx)).map(|c| c.length_ticks).unwrap_or(-1),
                ));
            }
            _ => {}
        }
    }

    /// Add a new instrument track. Inserts before the send/master tracks.
    /// `handle` is the shared audio-thread handle for this track.
    /// `mixer_id` is the track's ID in the mixer.

    /// Add a new instrument track. Inserts before the send/master tracks.
    /// `handle` is the shared audio-thread handle for this track.
    /// `mixer_id` is the track's ID in the mixer.
    pub(crate) fn add_instrument_track(
        &mut self,
        instrument: InstrumentType,
        mixer_id: usize,
        handle: std::sync::Arc<phosphor_core::project::TrackHandle>,
    ) {
        let name = match instrument {
            InstrumentType::Synth => "synth",
            InstrumentType::DrumRack => "drums",
            InstrumentType::DX7 => "dx7",
            InstrumentType::Jupiter8 => "jup8",
            InstrumentType::Odyssey => "odyss",
            InstrumentType::Juno60 => "juno",
            InstrumentType::Sampler => "smplr",
        };

        // Find insert position: before sends/master
        let insert_pos = self.tracks.iter().position(|t| {
            matches!(t.kind, TrackKind::SendA | TrackKind::SendB | TrackKind::Master)
        }).unwrap_or(self.tracks.len());

        let color = insert_pos % 8;
        let mut track = TrackState::new(name, color, true, TrackKind::Instrument, vec![]);
        track.mixer_id = Some(mixer_id);
        track.handle = Some(handle);
        track.instrument_type = Some(instrument);
        track.synth_params = match instrument {
            InstrumentType::Synth | InstrumentType::Sampler => {
                phosphor_dsp::synth::PARAM_DEFAULTS.to_vec()
            }
            InstrumentType::DrumRack => {
                phosphor_dsp::drum_rack::PARAM_DEFAULTS.to_vec()
            }
            InstrumentType::DX7 => {
                phosphor_dsp::dx7::PARAM_DEFAULTS.to_vec()
            }
            InstrumentType::Jupiter8 => {
                phosphor_dsp::jupiter::PARAM_DEFAULTS.to_vec()
            }
            InstrumentType::Odyssey => {
                phosphor_dsp::odyssey::PARAM_DEFAULTS.to_vec()
            }
            InstrumentType::Juno60 => {
                phosphor_dsp::juno::PARAM_DEFAULTS.to_vec()
            }
        };
        // Sync the initial armed state to audio
        track.sync_to_audio();
        self.tracks.insert(insert_pos, track);

        // Move cursor to the new track and open clip view with synth controls
        self.track_cursor = insert_pos;
        if self.track_cursor >= self.track_scroll + MAX_VISIBLE_TRACKS {
            self.track_scroll = self.track_cursor + 1 - MAX_VISIBLE_TRACKS;
        }

        // Select the track, show synth controls, and route MIDI to it
        self.track_selected = true;
        self.track_element = TrackElement::Label;
        self.show_current_track_controls();
    }


    pub(crate) fn open_clip_view(&mut self, track_idx: usize, clip_idx: usize) {
        self.clip_view_visible = true;
        self.clip_view_target = Some((track_idx, clip_idx));
        self.clip_view.fx_cursor = 0;
    }

    /// Show controls for the currently selected track and route MIDI to it.
    /// For instrument tracks: opens clip view with Synth tab, activates MIDI input.
    /// For bus tracks: no clip view, deactivates MIDI.

    pub(crate) fn fx_menu_select(&mut self) {
        // Add FX
        if let Some(fx_type) = FxType::ALL.get(self.fx_menu.cursor) {
            let inst = FxInstance::new(*fx_type);
            if let Some(t) = self.current_track_mut() {
                t.fx_chain.push(inst);
            }
        }
        self.fx_menu.open = false;
    }


    pub(crate) fn active_fx_chain_len(&self) -> usize {
        match self.clip_view.fx_panel_tab {
            FxPanelTab::TrackFx | FxPanelTab::Synth => {
                self.current_track().map(|t| t.fx_chain.len().max(1)).unwrap_or(1)
            }
        }
    }

    /// Keep clip_view_target in sync with the currently selected clip element.
    /// Called every frame as a safety net and after clip-modifying operations.
    pub(crate) fn sync_clip_view_target(&mut self) {
        if self.track_selected {
            if let TrackElement::Clip(idx) = self.track_element {
                let track_idx = self.track_cursor;
                if let Some(track) = self.tracks.get(track_idx) {
                    if idx < track.clips.len() {
                        self.clip_view_target = Some((track_idx, idx));
                        self.clip_view_visible = true;
                        return;
                    }
                }
            }
        }
    }

    // ── Accessors ──


    /// Receive a clip snapshot from the audio thread and add it to the
    /// corresponding TUI track's clip list.
    pub(crate) fn receive_clip_snapshot(&mut self, snap: phosphor_core::clip::ClipSnapshot) {
        crate::debug_log::system(&format!(
            "clip received: track={} events={} notes={} ticks={}..{}",
            snap.track_id, snap.event_count, snap.notes.len(),
            snap.start_tick, snap.start_tick + snap.length_ticks,
        ));
        if let Some(track) = self.tracks.iter_mut().find(|t| t.mixer_id == Some(snap.track_id)) {
            let ppq = phosphor_core::transport::Transport::PPQ;
            let beats = (snap.length_ticks as f64 / ppq as f64).ceil() as u16;
            let width = beats.max(2);

            // Only merge into an existing clip if it has the EXACT same start and length
            // (i.e. it's the same loop region being overdubbed). Overlapping but different
            // clips (from duplicate, paste, move) should NOT be merged.
            if let Some(existing) = track.clips.iter_mut().find(|c| {
                c.start_tick == snap.start_tick && c.length_ticks == snap.length_ticks
            }) {
                for n in &snap.notes {
                    crate::debug_log::system(&format!(
                        "  overdub note: pitch={} frac={:.4} dur={:.4} (snap len={})",
                        n.note, n.start_frac, n.duration_frac, snap.length_ticks
                    ));
                }

                existing.notes.extend(snap.notes);
                existing.has_content = true;
                crate::debug_log::system(&format!(
                    "  overdub merged (exact match): now {} notes", existing.notes.len()
                ));
            } else {
                // New clip — no exact match found
                let clip_number = track.clips.len() + 1;
                crate::debug_log::system(&format!(
                    "  new clip created: #{} at tick {} len {}",
                    clip_number, snap.start_tick, snap.length_ticks
                ));
                track.clips.push(Clip {
                    number: clip_number,
                    width,
                    has_content: true,
                    start_tick: snap.start_tick,
                    length_ticks: snap.length_ticks,
                    notes: snap.notes,
                });
            }
        }
    }
}

// ── Initial Data ──

/// Initial tracks: just the bus tracks. Instruments are added by the user via Space+A.
pub fn initial_tracks() -> Vec<TrackState> {
    vec![
        TrackState::new("snd a", 5, false, TrackKind::SendA, vec![]),
        TrackState::new("snd b", 6, false, TrackKind::SendB, vec![]),
        TrackState::new("mstr", 7, false, TrackKind::Master, vec![]),
    ]
}
