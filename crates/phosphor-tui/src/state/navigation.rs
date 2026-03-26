//! NavState methods: navigation.

use super::*;

impl NavState {

    // ── Space menu ──

    /// Toggle the space menu open/closed.
    pub(crate) fn toggle_space_menu(&mut self) {
        self.space_menu.toggle();
    }

    /// Handle a key press while the space menu is open.
    /// Returns a SpaceAction if an action should be performed.

    /// Handle a key press while the space menu is open.
    /// Returns a SpaceAction if an action should be performed.
    pub(crate) fn space_menu_handle(&mut self, ch: char) -> Option<SpaceAction> {
        self.space_menu.open = false;
        match ch {
            '1' => { self.focus_pane(Pane::Transport); None }
            '2' => { self.focus_pane(Pane::Tracks); None }
            '3' => { self.focus_pane(Pane::ClipView); None }
            'p' => Some(SpaceAction::PlayPause),
            'r' => Some(SpaceAction::ToggleRecord),
            'l' => Some(SpaceAction::ToggleLoop),
            'm' => Some(SpaceAction::ToggleMetronome),
            '!' => Some(SpaceAction::Panic),
            'a' => Some(SpaceAction::AddInstrument),
            's' => Some(SpaceAction::Save),
            'o' => Some(SpaceAction::Open),
            'd' => Some(SpaceAction::Delete),
            'v' => Some(SpaceAction::CycleTheme),
            'n' => Some(SpaceAction::NewTrack),
            'h' => {
                self.space_menu.open = true;
                self.space_menu.section = SpaceMenuSection::Help;
                self.space_menu.cursor = 0;
                None
            }
            _ => None,
        }
    }

    // ── Pane focus ──


    // ── Pane focus ──

    pub(crate) fn focus_pane(&mut self, pane: Pane) {
        if self.focused_pane == Pane::Tracks { self.track_selected = false; }
        self.focused_pane = pane;
        crate::debug_log::system(&format!("focused pane: {:?}", pane));
    }


    pub(crate) fn focus_next_pane(&mut self) {
        self.focus_pane(self.focused_pane.next());
    }

    // ── Navigation ──


    // ── Navigation ──

    pub(crate) fn move_up(&mut self) {
        if self.instrument_modal.open { self.instrument_modal.move_up(); return; }
        if self.space_menu.open { self.space_menu.move_up(); return; }
        if self.fx_menu.open { self.fx_menu.move_up(); return; }
        match self.focused_pane {
            Pane::Transport => {} // no vertical nav in transport
            Pane::Tracks if !self.track_selected => {
                if self.track_cursor > 0 {
                    self.track_cursor -= 1;
                    if self.track_cursor < self.track_scroll {
                        self.track_scroll = self.track_cursor;
                    }
                }
            }
            Pane::Tracks => {
                // Track is selected — j/k locked, does nothing here
                // (future: could navigate within track elements)
            }
            Pane::ClipView => {
                match self.clip_view.focus {
                    ClipViewFocus::PianoRoll if self.clip_view.clip_tab == ClipTab::InstConfig => {
                        if self.clip_view.inst_config_cursor > 0 {
                            self.clip_view.inst_config_cursor -= 1;
                        }
                    }
                    ClipViewFocus::PianoRoll => self.clip_view.piano_roll.move_up(),
                    ClipViewFocus::FxPanel => {
                        if self.clip_view.fx_panel_tab == FxPanelTab::Synth {
                            if self.clip_view.synth_param_cursor > 0 {
                                self.clip_view.synth_param_cursor -= 1;
                            }
                        } else if self.clip_view.fx_cursor > 0 {
                            self.clip_view.fx_cursor -= 1;
                        }
                    }
                }
            }
        }
    }


    pub(crate) fn move_down(&mut self) {
        if self.instrument_modal.open { self.instrument_modal.move_down(); return; }
        if self.space_menu.open { self.space_menu.move_down(); return; }
        if self.fx_menu.open { self.fx_menu.move_down(); return; }
        match self.focused_pane {
            Pane::Transport => {}
            Pane::Tracks if !self.track_selected => {
                if self.track_cursor + 1 < self.tracks.len() {
                    self.track_cursor += 1;
                    if self.track_cursor >= self.track_scroll + MAX_VISIBLE_TRACKS {
                        self.track_scroll = self.track_cursor + 1 - MAX_VISIBLE_TRACKS;
                    }
                }
            }
            Pane::Tracks => {
                // Track is selected — j/k locked
            }
            Pane::ClipView => {
                match self.clip_view.focus {
                    ClipViewFocus::PianoRoll if self.clip_view.clip_tab == ClipTab::InstConfig => {
                        if self.clip_view.inst_config_cursor + 1 < INST_CONFIG_PARAM_COUNT {
                            self.clip_view.inst_config_cursor += 1;
                        }
                    }
                    ClipViewFocus::PianoRoll => self.clip_view.piano_roll.move_down(),
                    ClipViewFocus::FxPanel => {
                        if self.clip_view.fx_panel_tab == FxPanelTab::Synth {
                            let max = self.current_track().map(|t| t.synth_params.len()).unwrap_or(0);
                            if self.clip_view.synth_param_cursor + 1 < max {
                                self.clip_view.synth_param_cursor += 1;
                            }
                        } else {
                            let max = self.active_fx_chain_len();
                            if self.clip_view.fx_cursor + 1 < max {
                                self.clip_view.fx_cursor += 1;
                            }
                        }
                    }
                }
            }
        }
    }


    pub(crate) fn move_left(&mut self) {
        if self.focused_pane == Pane::Tracks && self.track_selected {
            self.track_element = self.track_element.move_left();
        } else if self.focused_pane == Pane::ClipView {
            match self.clip_view.focus {
                ClipViewFocus::PianoRoll if self.clip_view.clip_tab == ClipTab::InstConfig => {
                    // h = placeholder for future inst config param adjustment
                }
                ClipViewFocus::PianoRoll => {
                    self.clip_view.focus = ClipViewFocus::FxPanel;
                }
                ClipViewFocus::FxPanel if self.clip_view.fx_panel_tab == FxPanelTab::Synth => {
                    // h = decrease parameter value
                    self.adjust_synth_param(-0.05);
                }
                _ => {}
            }
        }
    }


    pub(crate) fn move_right(&mut self) {
        if self.focused_pane == Pane::Tracks && self.track_selected {
            let num_clips = self.current_track().map(|t| t.clips.len()).unwrap_or(0);
            self.track_element = self.track_element.move_right(num_clips);
        } else if self.focused_pane == Pane::ClipView {
            match self.clip_view.focus {
                ClipViewFocus::PianoRoll if self.clip_view.clip_tab == ClipTab::InstConfig => {
                    // l = placeholder for future inst config param adjustment
                }
                ClipViewFocus::FxPanel if self.clip_view.fx_panel_tab == FxPanelTab::Synth => {
                    // l = increase parameter value
                    self.adjust_synth_param(0.05);
                }
                ClipViewFocus::FxPanel => {
                    self.clip_view.focus = ClipViewFocus::PianoRoll;
                }
                _ => {}
            }
        }
    }

    /// Adjust the currently selected synth parameter by delta.
    /// Returns the (mixer_id, param_index, new_value) if changed, for sending to audio.

    pub(crate) fn enter(&mut self) -> Option<SpaceAction> {
        // Space menu open → select item via space_menu_handle using the key from cursor position
        if self.space_menu.open {
            match self.space_menu.section {
                SpaceMenuSection::Actions => {
                    if let Some((key, _, _)) = SPACE_ACTIONS.get(self.space_menu.cursor) {
                        // Extract the char after "spc+"
                        if let Some(ch) = key.strip_prefix("spc+").and_then(|s| s.chars().next()) {
                            return self.space_menu_handle(ch);
                        }
                    }
                    self.space_menu.open = false;
                    return None;
                }
                SpaceMenuSection::Help => {
                    // Help topics just show info — no action
                    return None;
                }
            }
        }
        // FX menu open → select item
        if self.fx_menu.open {
            self.fx_menu_select();
            return None;
        }

        match self.focused_pane {
            Pane::Transport => {} // transport elements handled by app
            Pane::Tracks => {
                if !self.track_selected {
                    self.track_selected = true;
                    self.track_element = TrackElement::Label;
                    self.show_current_track_controls();
                } else {
                    self.activate_element();
                }
            }
            Pane::ClipView => {}
        }
        None
    }


    pub(crate) fn escape(&mut self) {
        if self.instrument_modal.open {
            self.instrument_modal.open = false;
            return;
        }
        if self.space_menu.open {
            self.space_menu.open = false;
            return;
        }
        if self.fx_menu.open {
            self.fx_menu.open = false;
            return;
        }
        match self.focused_pane {
            Pane::Transport => {} // no escape action in transport
            Pane::Tracks => {
                if self.track_selected {
                    self.track_selected = false;
                    self.track_element = TrackElement::Label;
                    self.clip_view_visible = false;
                    self.clip_view_target = None;
                }
            }
            Pane::ClipView => self.focus_pane(Pane::Tracks),
        }
    }

    /// Cycle tabs in the clip view (FX panel or piano roll side).
    /// Cycle through ALL tabs in buffer 3: trk fx → synth → inst config → piano → auto → trk fx...

    /// Cycle tabs in the clip view (FX panel or piano roll side).
    /// Cycle through ALL tabs in buffer 3: trk fx → synth → inst config → piano → auto → trk fx...
    pub(crate) fn cycle_tab(&mut self) {
        if self.focused_pane != Pane::ClipView { return; }

        match (self.clip_view.focus, self.clip_view.fx_panel_tab, self.clip_view.clip_tab) {
            // FX panel: trk fx → synth
            (ClipViewFocus::FxPanel, FxPanelTab::TrackFx, _) => {
                self.clip_view.fx_panel_tab = FxPanelTab::Synth;
            }
            // FX panel: synth → inst config
            (ClipViewFocus::FxPanel, FxPanelTab::Synth, _) => {
                self.clip_view.focus = ClipViewFocus::PianoRoll;
                self.clip_view.clip_tab = ClipTab::InstConfig;
                self.clip_view.inst_config_cursor = 0;
            }
            // Inst config → piano roll
            (ClipViewFocus::PianoRoll, _, ClipTab::InstConfig) => {
                self.clip_view.clip_tab = ClipTab::PianoRoll;
                self.clip_view.piano_roll.focus = PianoRollFocus::Navigation;
                self.clip_view.piano_roll.column = 0;
            }
            // Piano roll: piano → auto
            (ClipViewFocus::PianoRoll, _, ClipTab::PianoRoll) => {
                self.clip_view.clip_tab = ClipTab::Automation;
            }
            // Piano roll: auto → back to trk fx
            (ClipViewFocus::PianoRoll, _, ClipTab::Automation) => {
                self.clip_view.focus = ClipViewFocus::FxPanel;
                self.clip_view.fx_panel_tab = FxPanelTab::TrackFx;
                self.clip_view.fx_cursor = 0;
            }
        }
    }

}
