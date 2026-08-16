//! NavState methods: params.

use super::*;

impl NavState {

    /// Adjust the currently selected synth parameter by delta.
    /// Returns the (mixer_id, param_index, new_value) if changed, for sending to audio.
    pub fn adjust_synth_param(&mut self, delta: f32) -> Option<(usize, usize, f32)> {
        let idx = self.clip_view.synth_param_cursor;
        if let Some(track) = self.tracks.get_mut(self.track_cursor) {
            if idx < track.synth_params.len() {
                // Index 0 is always a discrete selector — the waveform on the
                // phosphor synth, the kit on the drum rack, the patch on the
                // four instruments with factory banks.
                let is_jupiter = track.instrument_type == Some(InstrumentType::Jupiter8);
                let is_odyssey = track.instrument_type == Some(InstrumentType::Odyssey);
                let is_juno = track.instrument_type == Some(InstrumentType::Juno60);
                let is_dx7 = track.instrument_type == Some(InstrumentType::DX7);
                let is_drum = track.instrument_type == Some(InstrumentType::DrumRack);
                let is_discrete = if is_jupiter {
                    phosphor_dsp::jupiter::is_discrete(idx)
                } else if is_odyssey {
                    phosphor_dsp::odyssey::is_discrete(idx)
                } else if is_juno {
                    phosphor_dsp::juno::is_discrete(idx)
                } else if is_dx7 {
                    // Two selectors, not one: the bank knob is as discrete as
                    // the patch knob, and stepping it by a continuous delta
                    // would walk through cartridges a fraction at a time.
                    phosphor_dsp::dx7::is_discrete(idx)
                } else if is_drum {
                    phosphor_dsp::drum_rack::is_discrete(idx)
                } else {
                    idx == 0
                };
                let actual_delta = if is_discrete {
                    let step = if is_odyssey {
                        match idx {
                            0 => 1.0 / (phosphor_dsp::odyssey::PATCH_COUNT as f32 - 0.01),
                            6 => 0.34, // 3 filter types
                            _ => 0.5,
                        }
                    } else {
                        0.25
                    };
                    if delta > 0.0 { step } else { -step }
                } else {
                    delta
                };
                // The DX7, the Juno, the Jupiter and the drum rack step their
                // selectors by index rather than by adding a fraction of the
                // knob's travel: 256 voices, or 56 patches and a
                // three-position range switch, or 64 patches and seven
                // switches, or ten kits, are coarse enough that an accumulated
                // rounding error lands on the wrong side of a step boundary,
                // which reads as a keypress that did nothing.
                let new_val = if is_discrete && is_dx7 {
                    phosphor_dsp::dx7::step_discrete(idx, track.synth_params[idx], delta > 0.0)
                } else if is_discrete && is_drum {
                    phosphor_dsp::drum_rack::step_discrete(idx, track.synth_params[idx], delta > 0.0)
                } else if is_discrete && is_juno {
                    phosphor_dsp::juno::step_discrete(idx, track.synth_params[idx], delta > 0.0)
                } else if is_discrete && is_jupiter {
                    phosphor_dsp::jupiter::step_discrete(idx, track.synth_params[idx], delta > 0.0)
                } else {
                    (track.synth_params[idx] + actual_delta).clamp(0.0, 1.0)
                };
                track.synth_params[idx] = new_val;

                // When patch selector changes, sync all params from preset.
                // The banks no longer agree on how many parameters an
                // instrument has, so this collects rather than matching on a
                // fixed-size array, and writes through a zip so a track
                // carrying a shorter block than its instrument now has cannot
                // index off the end of itself.
                if idx == 0 {
                    let new_params: Option<Vec<f32>> = match track.instrument_type {
                        Some(InstrumentType::Jupiter8) => {
                            Some(phosphor_dsp::jupiter::Jupiter8Synth::params_for_patch(new_val).to_vec())
                        }
                        Some(InstrumentType::Odyssey) => {
                            Some(phosphor_dsp::odyssey::OdysseySynth::params_for_patch(new_val).to_vec())
                        }
                        Some(InstrumentType::Juno60) => {
                            Some(phosphor_dsp::juno::Juno60Synth::params_for_patch(new_val).to_vec())
                        }
                        _ => None,
                    };
                    if let Some(preset_params) = new_params {
                        for (slot, v) in track.synth_params.iter_mut().zip(preset_params) {
                            *slot = v;
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
    pub fn show_current_track_controls(&mut self) {
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

                // Use the currently selected clip element, or default to clip 0
                let clip_idx = match self.track_element {
                    super::TrackElement::Clip(i) if i < track.clips.len() => i,
                    _ => 0,
                };
                self.clip_view_target = Some((self.track_cursor, clip_idx));

                // If track has recorded clips, show piano roll. Otherwise show synth.
                if !track.clips.is_empty() {
                    self.clip_view.clip_tab = ClipTab::PianoRoll;
                    self.clip_view.focus = ClipViewFocus::PianoRoll;
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

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::{drum_rack, dx7, juno, jupiter};

    /// A nav state whose selected track is a DX7 at its default parameters.
    fn dx7_track() -> NavState {
        let mut nav = NavState::new(super::super::initial_tracks());
        let mut track = TrackState::new("dx7", 0, true, TrackKind::Instrument, vec![]);
        track.instrument_type = Some(InstrumentType::DX7);
        track.synth_params = dx7::PARAM_DEFAULTS.to_vec();
        nav.tracks.insert(0, track);
        nav.track_cursor = 0;
        nav
    }

    fn selected(nav: &NavState) -> (usize, usize) {
        let p = &nav.tracks[0].synth_params;
        (dx7::bank_index(p[dx7::P_BANK]), dx7::patch_index(p[dx7::P_PATCH]))
    }

    #[test]
    fn dx7_selectors_move_one_step_per_keypress() {
        // Both DX7 selectors are discrete: a keypress is one voice or one
        // cartridge, not a fraction of the knob's travel. The patch knob alone
        // would otherwise take 256 presses to cross the factory set.
        let mut nav = dx7_track();
        nav.clip_view.synth_param_cursor = dx7::P_PATCH;
        let (bank, patch) = selected(&nav);
        for step in 1..=5 {
            nav.adjust_synth_param(0.05);
            assert_eq!(selected(&nav), (bank, patch + step), "patch knob step {step}");
        }
        for step in (0..5).rev() {
            nav.adjust_synth_param(-0.05);
            assert_eq!(selected(&nav), (bank, patch + step), "patch knob back to {step}");
        }

        nav.clip_view.synth_param_cursor = dx7::P_BANK;
        for step in 1..dx7::BANK_COUNT {
            nav.adjust_synth_param(0.05);
            assert_eq!(selected(&nav), (step, patch), "bank knob step {step}");
        }
        // ...and neither runs off its end.
        for _ in 0..4 {
            nav.adjust_synth_param(0.05);
        }
        assert_eq!(selected(&nav), (dx7::BANK_COUNT - 1, patch));
    }

    /// A nav state whose selected track is a Juno-60 at its default panel.
    fn juno_track() -> NavState {
        let mut nav = NavState::new(super::super::initial_tracks());
        let mut track = TrackState::new("juno", 0, true, TrackKind::Instrument, vec![]);
        track.instrument_type = Some(InstrumentType::Juno60);
        track.synth_params = juno::PARAM_DEFAULTS.to_vec();
        nav.tracks.insert(0, track);
        nav.track_cursor = 0;
        nav
    }

    fn juno_patch(nav: &NavState) -> usize {
        juno::patch_index(nav.tracks[0].synth_params[juno::P_PATCH])
    }

    #[test]
    fn juno_selectors_move_one_step_per_keypress() {
        // 56 factory patches: a keypress is one patch, not a fraction of the
        // knob's travel, and the whole bank has to be reachable from either
        // end. The three-position PWM switch is here too, because a switch
        // that gained a position is the one most likely to be stepped by a
        // stale fraction.
        let mut nav = juno_track();
        nav.clip_view.synth_param_cursor = juno::P_PATCH;
        for step in 1..juno::PATCH_COUNT {
            nav.adjust_synth_param(0.05);
            assert_eq!(juno_patch(&nav), step, "patch knob step {step}");
        }
        nav.adjust_synth_param(0.05);
        assert_eq!(juno_patch(&nav), juno::PATCH_COUNT - 1, "patch knob ran off the top");
        for step in (0..juno::PATCH_COUNT - 1).rev() {
            nav.adjust_synth_param(-0.05);
            assert_eq!(juno_patch(&nav), step, "patch knob back to {step}");
        }

        // Selecting a patch loads its panel: 78 SYNTHESIZER DRUM is the one
        // with the filter at self-oscillation and no oscillator at all.
        for _ in 0..juno::PATCH_COUNT {
            nav.adjust_synth_param(0.05);
        }
        let panel = &nav.tracks[0].synth_params;
        assert_eq!(juno_patch(&nav), juno::PATCH_COUNT - 1);
        assert!((panel[juno::P_RESO] - 1.0).abs() < 1e-6, "res {}", panel[juno::P_RESO]);

        // A fresh panel, because the switch has to start where 11 STRINGS 1
        // leaves it rather than where the last patch of the sweep did.
        let mut nav = juno_track();
        nav.clip_view.synth_param_cursor = juno::P_PWM_MODE;
        let label = |nav: &NavState| {
            juno::discrete_label(juno::P_PWM_MODE, nav.tracks[0].synth_params[juno::P_PWM_MODE])
        };
        assert_eq!(label(&nav), Some("LFO"));
        let mut seen = Vec::new();
        for _ in 0..3 {
            nav.adjust_synth_param(0.05);
            seen.push(label(&nav));
        }
        assert_eq!(seen, [Some("MAN"), Some("ENV"), Some("ENV")]);
    }

    /// A nav state whose selected track is a Jupiter-8 at its default panel.
    fn jupiter_track() -> NavState {
        let mut nav = NavState::new(super::super::initial_tracks());
        let mut track = TrackState::new("jupiter", 0, true, TrackKind::Instrument, vec![]);
        track.instrument_type = Some(InstrumentType::Jupiter8);
        track.synth_params = jupiter::PARAM_DEFAULTS.to_vec();
        nav.tracks.insert(0, track);
        nav.track_cursor = 0;
        nav
    }

    #[test]
    fn jupiter_selectors_move_one_step_per_keypress() {
        // 64 patches and seven switches. The patch knob used to step by
        // 1/(n - 0.01) of the travel, which is a fraction that does not
        // divide the bank: the accumulated error lands on the wrong side of a
        // boundary and the keypress reads as having done nothing.
        let mut nav = jupiter_track();
        nav.clip_view.synth_param_cursor = jupiter::P_PATCH;
        let patch = |nav: &NavState| {
            jupiter::patch_index(nav.tracks[0].synth_params[jupiter::P_PATCH])
        };
        for step in 1..jupiter::PATCH_COUNT {
            nav.adjust_synth_param(0.05);
            assert_eq!(patch(&nav), step, "patch knob step {step}");
        }
        nav.adjust_synth_param(0.05);
        assert_eq!(patch(&nav), jupiter::PATCH_COUNT - 1, "patch knob ran off the top");
        for step in (0..jupiter::PATCH_COUNT - 1).rev() {
            nav.adjust_synth_param(-0.05);
            assert_eq!(patch(&nav), step, "patch knob back to {step}");
        }

        // A fresh panel, because the waveform switch has to start where patch
        // 0 leaves it rather than where the last patch of the sweep did.
        let mut nav = jupiter_track();
        nav.clip_view.synth_param_cursor = jupiter::P_VCO2_WAVE;
        let label = |nav: &NavState| {
            jupiter::discrete_label(
                jupiter::P_VCO2_WAVE,
                nav.tracks[0].synth_params[jupiter::P_VCO2_WAVE],
            )
        };
        assert_eq!(label(&nav), Some("SAW"));
        let mut seen = Vec::new();
        for _ in 0..3 {
            nav.adjust_synth_param(0.05);
            seen.push(label(&nav));
        }
        assert_eq!(seen, [Some("PLS"), Some("NOISE"), Some("NOISE")]);
    }

    /// A nav state whose selected track is a drum rack at its default panel.
    fn drum_track() -> NavState {
        let mut nav = NavState::new(super::super::initial_tracks());
        let mut track = TrackState::new("drums", 0, true, TrackKind::Instrument, vec![]);
        track.instrument_type = Some(InstrumentType::DrumRack);
        track.synth_params = drum_rack::PARAM_DEFAULTS.to_vec();
        nav.tracks.insert(0, track);
        nav.track_cursor = 0;
        nav
    }

    #[test]
    fn the_drum_kit_selector_moves_one_kit_per_keypress() {
        // Ten kits, stepped by index. This used to add a tenth of the knob's
        // travel per press, which does not divide the selector evenly: the
        // accumulated error lands on the wrong side of a boundary and the
        // keypress reads as having done nothing.
        let mut nav = drum_track();
        nav.clip_view.synth_param_cursor = drum_rack::P_KIT;
        let kit = |nav: &NavState| {
            drum_rack::discrete_label(drum_rack::P_KIT, nav.tracks[0].synth_params[drum_rack::P_KIT])
        };
        assert_eq!(kit(&nav), Some("808"));
        for label in drum_rack::KIT_LABELS.iter().skip(1) {
            nav.adjust_synth_param(0.05);
            assert_eq!(kit(&nav), Some(*label));
        }
        nav.adjust_synth_param(0.05);
        assert_eq!(kit(&nav), Some("tsty-5"), "the kit knob ran off the top");
        for label in drum_rack::KIT_LABELS.iter().rev().skip(1) {
            nav.adjust_synth_param(-0.05);
            assert_eq!(kit(&nav), Some(*label));
        }

        // The rest of the panel is continuous, and moving one control moves
        // only that control.
        nav.clip_view.synth_param_cursor = drum_rack::P_BD_DECAY;
        let before = nav.tracks[0].synth_params.clone();
        nav.adjust_synth_param(-0.05);
        let after = &nav.tracks[0].synth_params;
        assert!((after[drum_rack::P_BD_DECAY] - 0.45).abs() < 1e-6);
        for i in 0..after.len() {
            if i != drum_rack::P_BD_DECAY {
                assert_eq!(before[i], after[i], "{} moved with the kick's decay", drum_rack::PARAM_NAMES[i]);
            }
        }
    }

    #[test]
    fn the_dx7_bank_knob_is_the_last_parameter() {
        // Sessions store `synth_params` positionally, so the bank selector was
        // appended rather than filed next to the patch selector: inserting it
        // would load every saved value of every existing session one slot out.
        let nav = dx7_track();
        assert_eq!(nav.tracks[0].synth_params.len(), dx7::PARAM_COUNT);
        assert_eq!(dx7::P_BANK, dx7::PARAM_COUNT - 1);
        assert_eq!(dx7::PARAM_NAMES[dx7::P_GAIN], "gain", "index 0-7 must not move");
    }
}
