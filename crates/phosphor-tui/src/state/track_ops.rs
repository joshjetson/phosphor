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
            crate::debug_log::system(&format!(
                "jump_to_clip: looking for #{}, track has {} clips: {:?}",
                clip_number, track.clips.len(),
                track.clips.iter().map(|c| (c.number, c.start_tick, c.length_ticks)).collect::<Vec<_>>()
            ));
            if let Some(idx) = track.clips.iter().position(|c| c.number == clip_number) {
                self.track_element = TrackElement::Clip(idx);
                self.open_clip_view(self.track_cursor, idx);
                crate::debug_log::system(&format!("jump_to_clip: selected idx={}", idx));
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
        crate::debug_log::system(&format!(
            "open_clip_view: track={} clip={} (notes={})",
            track_idx, clip_idx,
            self.tracks.get(track_idx).and_then(|t| t.clips.get(clip_idx)).map(|c| c.notes.len()).unwrap_or(0)
        ));
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

    /// Remove phantom clips: when two clips overlap at the same start position,
    /// keep the longer one and absorb the shorter one's notes (rescaled).
    /// Returns (mixer_id, removed_clip_index) pairs so the caller can sync audio.
    pub(crate) fn dedup_clips(&mut self) -> Vec<(usize, usize)> {
        let ppq = phosphor_core::transport::Transport::PPQ;
        let tolerance = ppq;
        let mut removed = Vec::new();

        for track in &mut self.tracks {
            if track.clips.len() < 2 { continue; }

            track.clips.sort_by(|a, b| {
                a.start_tick.cmp(&b.start_tick)
                    .then(b.length_ticks.cmp(&a.length_ticks))
            });

            let mut i = 0;
            while i + 1 < track.clips.len() {
                let starts_close = (track.clips[i].start_tick - track.clips[i + 1].start_tick).abs() <= tolerance;
                if starts_close {
                    let shorter_len = track.clips[i + 1].length_ticks;
                    let longer_len = track.clips[i].length_ticks;
                    if longer_len > 0 {
                        let scale = shorter_len as f64 / longer_len as f64;
                        let absorbed: Vec<_> = track.clips[i + 1].notes.iter().map(|n| {
                            let mut rescaled = *n;
                            rescaled.start_frac *= scale;
                            rescaled.duration_frac *= scale;
                            rescaled
                        }).collect();
                        track.clips[i].notes.extend(absorbed);
                    }
                    crate::debug_log::system(&format!(
                        "dedup: absorbed clip #{} (len={}) into clip #{} (len={}) on '{}'",
                        track.clips[i + 1].number, shorter_len,
                        track.clips[i].number, longer_len, track.name
                    ));
                    // Record the removal for audio thread sync
                    if let Some(mid) = track.mixer_id {
                        removed.push((mid, i + 1));
                    }
                    track.clips.remove(i + 1);
                } else {
                    i += 1;
                }
            }

            for (idx, clip) in track.clips.iter_mut().enumerate() {
                clip.number = idx + 1;
            }
        }
        removed
    }

    // ── Accessors ──


    /// Receive a clip snapshot from the audio thread and add it to the
    /// corresponding TUI track's clip list.
    /// `is_recording` = true when transport is actively recording (snapshots are fresh overdubs).
    /// When NOT recording, snapshots matching the viewed clip are stale (from panic/reset) and ignored.
    /// Returns (mixer_id, count_absorbed) so caller can send RemoveClip commands to audio.
    pub(crate) fn receive_clip_snapshot(&mut self, snap: phosphor_core::clip::ClipSnapshot, is_recording: bool) -> Option<(usize, usize)> {
        crate::debug_log::system(&format!(
            "clip received: track={} events={} notes={} ticks={}..{} recording={}",
            snap.track_id, snap.event_count, snap.notes.len(),
            snap.start_tick, snap.start_tick + snap.length_ticks, is_recording,
        ));

        // When NOT recording AND no grace remaining, ignore snapshots.
        // These are stale commits from panic/reset_all that would re-add
        // deleted notes or create phantom clips.
        // Accept if: (a) currently recording, OR (b) grace counter > 0
        // (final commits from tracks that just stopped recording).
        if !is_recording && self.recording_grace == 0 {
            crate::debug_log::system(
                "  IGNORED: snapshot while not recording (stale from panic/reset)"
            );
            return None;
        }
        // Decrement grace after accepting a post-recording snapshot
        if !is_recording && self.recording_grace > 0 {
            self.recording_grace -= 1;
        }

        // Find the track index (we need it for clip_view_target fixup)
        let track_idx = match self.tracks.iter().position(|t| t.mixer_id == Some(snap.track_id)) {
            Some(idx) => idx,
            None => return None,
        };

        let mut absorbed_count = 0usize;
        {
            let track = &mut self.tracks[track_idx];
            let ppq = phosphor_core::transport::Transport::PPQ;
            let beats = (snap.length_ticks as f64 / ppq as f64).ceil() as u16;
            let width = beats.max(2);
            let snap_end = snap.start_tick + snap.length_ticks;

            // Absorb any clips that the new recording fully covers.
            // A clip is covered if it starts within the snap range and ends within it.
            let mut absorbed_notes = Vec::new();
            track.clips.retain(|c| {
                let c_end = c.start_tick + c.length_ticks;
                let covered = c.start_tick >= snap.start_tick && c_end <= snap_end;
                if covered {
                    crate::debug_log::system(&format!(
                        "  absorbing clip #{}: tick {}..{} (snap covers {}..{})",
                        c.number, c.start_tick, c_end, snap.start_tick, snap_end
                    ));
                    // Rescale notes to snap's coordinate space
                    let offset = (c.start_tick - snap.start_tick) as f64 / snap.length_ticks as f64;
                    let scale = c.length_ticks as f64 / snap.length_ticks as f64;
                    for mut n in c.notes.clone() {
                        n.start_frac = n.start_frac * scale + offset;
                        n.duration_frac *= scale;
                        absorbed_notes.push(n);
                    }
                    absorbed_count += 1;
                    false
                } else {
                    true
                }
            });

            // Combine absorbed notes with the new recording's notes
            let mut all_notes = absorbed_notes;
            all_notes.extend(snap.notes);

            // Try to merge into an existing clip with a close start
            let merge_tolerance = ppq;
            if let Some(existing) = track.clips.iter_mut().find(|c| {
                (c.start_tick - snap.start_tick).abs() <= merge_tolerance
            }) {
                // Rescale if lengths differ
                if snap.length_ticks != existing.length_ticks && existing.length_ticks > 0 {
                    let scale = snap.length_ticks as f64 / existing.length_ticks as f64;
                    let offset = (snap.start_tick - existing.start_tick) as f64 / existing.length_ticks as f64;
                    for n in &mut all_notes {
                        n.start_frac = n.start_frac * scale + offset;
                        n.duration_frac *= scale;
                    }
                }
                existing.notes.extend(all_notes);
                existing.has_content = true;
                existing.length_ticks = existing.length_ticks.max(snap.length_ticks);
                existing.width = width.max(existing.width);
                crate::debug_log::system(&format!(
                    "  merged into existing clip: now {} notes, len={}",
                    existing.notes.len(), existing.length_ticks
                ));
            } else {
                // Create new clip
                let clip_number = track.clips.len() + 1;
                crate::debug_log::system(&format!(
                    "  new clip: #{} at tick {} len {} ({} notes, absorbed {})",
                    clip_number, snap.start_tick, snap.length_ticks, all_notes.len(), absorbed_count
                ));
                track.clips.push(Clip {
                    number: clip_number,
                    width,
                    has_content: true,
                    start_tick: snap.start_tick,
                    length_ticks: snap.length_ticks,
                    notes: all_notes,
                    hidden_notes: Vec::new(),
                });
            }

            // Renumber clips sequentially
            for (i, c) in track.clips.iter_mut().enumerate() {
                c.number = i + 1;
            }
        }

        // Fix clip_view_target if it was pointing at this track
        // (clips may have been absorbed/reordered)
        if let Some((ti, ci)) = self.clip_view_target {
            if ti == track_idx {
                let num_clips = self.tracks[track_idx].clips.len();
                if num_clips == 0 {
                    self.clip_view_target = None;
                    self.clip_view_visible = false;
                } else if ci >= num_clips {
                    // Target was past the end — point to the last clip
                    self.clip_view_target = Some((track_idx, num_clips - 1));
                    crate::debug_log::system(&format!(
                        "  clip_view_target fixed: {} → {}", ci, num_clips - 1
                    ));
                }
            }
        }

        // Return absorption info so caller can sync removed clips to audio
        if absorbed_count > 0 {
            Some((snap.track_id, absorbed_count))
        } else {
            None
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
