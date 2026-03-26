//! NavState methods: params.

use super::*;

impl NavState {

    /// Adjust the currently selected synth parameter by delta.
    /// Returns the (mixer_id, param_index, new_value) if changed, for sending to audio.
    pub(crate) fn adjust_synth_param(&mut self, delta: f32) -> Option<(usize, usize, f32)> {
        let idx = self.clip_view.synth_param_cursor;
        if let Some(track) = self.tracks.get_mut(self.track_cursor) {
            if idx < track.synth_params.len() {
                // Index 0 is always a discrete selector (waveform for synth, kit for drums)
                // Synth: 4 options → step 0.25. Drums: 5 kits → step 0.20
                let is_jupiter = track.instrument_type == Some(InstrumentType::Jupiter8);
                let is_odyssey = track.instrument_type == Some(InstrumentType::Odyssey);
                let is_juno = track.instrument_type == Some(InstrumentType::Juno60);
                let is_discrete = if is_jupiter {
                    phosphor_dsp::jupiter::is_discrete(idx)
                } else if is_odyssey {
                    phosphor_dsp::odyssey::is_discrete(idx)
                } else if is_juno {
                    phosphor_dsp::juno::is_discrete(idx)
                } else {
                    idx == 0
                };
                let actual_delta = if is_discrete {
                    let step = if is_jupiter {
                        match idx {
                            0 => 1.0 / (phosphor_dsp::jupiter::PATCH_COUNT as f32 - 0.01),
                            _ => 0.25,
                        }
                    } else if is_odyssey {
                        match idx {
                            0 => 1.0 / (phosphor_dsp::odyssey::PATCH_COUNT as f32 - 0.01),
                            6 => 0.34, // 3 filter types
                            _ => 0.5,
                        }
                    } else if is_juno {
                        match idx {
                            0 => 1.0 / (phosphor_dsp::juno::PATCH_COUNT as f32 - 0.01),
                            12 => 0.25, // 4 chorus modes
                            _ => 0.5,   // on/off switches
                        }
                    } else {
                        match track.instrument_type {
                            Some(InstrumentType::DrumRack) => 0.1, // 10 kits
                            Some(InstrumentType::DX7) => 1.0 / (phosphor_dsp::dx7::PATCH_COUNT as f32 - 0.01),
                            _ => 0.25,
                        }
                    };
                    if delta > 0.0 { step } else { -step }
                } else {
                    delta
                };
                let new_val = (track.synth_params[idx] + actual_delta).clamp(0.0, 1.0);
                track.synth_params[idx] = new_val;

                // When patch selector changes, sync all params from preset
                if idx == 0 {
                    let new_params = match track.instrument_type {
                        Some(InstrumentType::Jupiter8) => {
                            Some(phosphor_dsp::jupiter::Jupiter8Synth::params_for_patch(new_val))
                        }
                        Some(InstrumentType::Odyssey) => {
                            Some(phosphor_dsp::odyssey::OdysseySynth::params_for_patch(new_val))
                        }
                        Some(InstrumentType::Juno60) => {
                            Some(phosphor_dsp::juno::Juno60Synth::params_for_patch(new_val))
                        }
                        _ => None,
                    };
                    if let Some(preset_params) = new_params {
                        for (i, &v) in preset_params.iter().enumerate() {
                            track.synth_params[i] = v;
                        }
                    }
                }

                if let Some(mixer_id) = track.mixer_id {
                    return Some((mixer_id, idx, new_val));
                }
            }
        }
        None
    }


    /// Show controls for the currently selected track and route MIDI to it.
    /// For instrument tracks: opens clip view with Synth tab, activates MIDI input.
    /// For bus tracks: no clip view, deactivates MIDI.
    pub(crate) fn show_current_track_controls(&mut self) {
        // Deactivate MIDI on ALL tracks first
        for track in &self.tracks {
            if let Some(ref h) = track.handle {
                h.config.midi_active.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }

        if let Some(track) = self.tracks.get(self.track_cursor) {
            if track.is_live() {
                if let Some(ref h) = track.handle {
                    h.config.midi_active.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                self.clip_view_visible = true;
                self.clip_view_target = Some((self.track_cursor, 0));

                // If track has recorded clips, show piano roll. Otherwise show synth.
                if !track.clips.is_empty() {
                    self.clip_view.clip_tab = ClipTab::PianoRoll;
                    self.clip_view.focus = ClipViewFocus::PianoRoll;
                    // Reset piano roll to browsing mode, column 1
                    self.clip_view.piano_roll.focus = PianoRollFocus::Navigation;
                    self.clip_view.piano_roll.column = 0;
                } else {
                    self.clip_view.fx_panel_tab = FxPanelTab::Synth;
                    self.clip_view.focus = ClipViewFocus::FxPanel;
                    self.clip_view.synth_param_cursor = 0;
                }
            } else {
                // Bus track — hide clip view
                self.clip_view_visible = false;
                self.clip_view_target = None;
            }
        }
    }

}
