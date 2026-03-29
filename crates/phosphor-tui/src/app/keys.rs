//! App methods: keys.

use super::*;

impl App {

    pub(crate) fn handle_event(&mut self, event: Event) {
        use crate::debug_log as dbg;

        let Event::Key(key) = event else { return };

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            dbg::user("Ctrl+C → quit");
            self.running = false;
            return;
        }

        // Ctrl+S → quick save
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            dbg::user("Ctrl+S → save");
            self.handle_save();
            return;
        }

        // Undo (u) / Redo (Ctrl+r) — works globally except in modals
        if key.code == KeyCode::Char('u') {
            dbg::user(&format!("u key received: modifiers={:?} space={} input={} confirm={} instr={} fx={}",
                key.modifiers, self.nav.space_menu.open, self.nav.input_modal.open,
                self.nav.confirm_modal.open, self.nav.instrument_modal.open, self.nav.fx_menu.open));
        }
        if !self.nav.space_menu.open && !self.nav.input_modal.open && !self.nav.confirm_modal.open
            && !self.nav.instrument_modal.open && !self.nav.fx_menu.open
        {
            if key.code == KeyCode::Char('u') && !key.modifiers.contains(KeyModifiers::SHIFT) {
                dbg::user("u → performing undo");
                self.perform_undo();
                return;
            }
            if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.perform_redo();
                return;
            }
        }

        // Confirmation modal — y/n
        if self.nav.confirm_modal.open {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let kind = self.nav.confirm_modal.kind;
                    self.nav.confirm_modal.close();
                    self.execute_delete(kind);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.nav.confirm_modal.close();
                }
                _ => {}
            }
            return;
        }

        // Input modal active — capture all keys for text entry
        if self.nav.input_modal.open {
            match key.code {
                KeyCode::Esc => {
                    self.nav.input_modal.close();
                }
                KeyCode::Enter => {
                    let path = self.nav.input_modal.value().to_string();
                    let kind = self.nav.input_modal.kind;
                    self.nav.input_modal.close();
                    if !path.is_empty() {
                        match kind {
                            InputModalKind::SaveAs => self.do_save(&path),
                            InputModalKind::Open => self.do_load(&path),
                        }
                    }
                }
                KeyCode::Backspace => { self.nav.input_modal.backspace(); }
                KeyCode::Delete => { self.nav.input_modal.delete(); }
                KeyCode::Left => { self.nav.input_modal.move_left(); }
                KeyCode::Right => { self.nav.input_modal.move_right(); }
                KeyCode::Home => { self.nav.input_modal.move_home(); }
                KeyCode::End => { self.nav.input_modal.move_end(); }
                KeyCode::Char(ch) => { self.nav.input_modal.type_char(ch); }
                _ => {}
            }
            return;
        }

        // Loop editor active — controls locked to loop markers
        // BUT Space passes through to open the space menu (so user can play/pause)
        if self.nav.loop_editor.active
            && key.code != KeyCode::Char(' ')
            && key.code != KeyCode::Tab
            && key.code != KeyCode::BackTab
        {
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Esc => {
                    dbg::user("loop editor: Esc → unfocus");
                    self.nav.loop_editor.unfocus();
                }
                KeyCode::Enter => {
                    self.nav.loop_editor.toggle_enabled();
                    dbg::user(&format!("loop editor: Enter → enabled={}", self.nav.loop_editor.enabled));
                    self.sync_loop_to_transport();
                    self.log_transport_state();
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    if shift {
                        dbg::user("loop editor: Shift+h → move end left");
                        self.nav.loop_editor.move_end_left();
                    } else {
                        dbg::user("loop editor: h → move start left");
                        self.nav.loop_editor.move_start_left();
                    }
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                    self.sync_loop_to_transport();
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    if shift {
                        dbg::user("loop editor: Shift+l → move end right");
                        self.nav.loop_editor.move_end_right();
                    } else {
                        dbg::user("loop editor: l → move start right");
                        self.nav.loop_editor.move_start_right();
                    }
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                    self.sync_loop_to_transport();
                }
                KeyCode::Char('H') => {
                    dbg::user("loop editor: H → move end left");
                    self.nav.loop_editor.move_end_left();
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                    self.sync_loop_to_transport();
                }
                KeyCode::Char('L') => {
                    dbg::user("loop editor: L → move end right");
                    self.nav.loop_editor.move_end_right();
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                    self.sync_loop_to_transport();
                }
                _ => {
                    dbg::user(&format!("loop editor: ignored key {:?}", key.code));
                }
            }
            return;
        }

        // Instrument modal open
        if self.nav.instrument_modal.open {
            match key.code {
                KeyCode::Esc => {
                    dbg::user("instrument modal: Esc → close");
                    self.nav.escape();
                }
                KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
                KeyCode::Enter => {
                    let instrument = self.nav.instrument_modal.selected();
                    dbg::user(&format!("instrument modal: Enter → selected {:?}", instrument));
                    self.nav.instrument_modal.open = false;
                    self.create_instrument_track(instrument);
                }
                _ => {}
            }
            return;
        }

        // Space menu open
        if self.nav.space_menu.open {
            match key.code {
                KeyCode::Char(' ') | KeyCode::Esc => {
                    dbg::user("space menu: close");
                    self.nav.space_menu.open = false;
                }
                KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
                KeyCode::Tab => self.nav.space_menu.switch_section(),
                KeyCode::Enter => {
                    if let Some(action) = self.nav.enter() {
                        dbg::user(&format!("space menu: Enter → {:?}", action));
                        self.handle_space_action(action);
                    }
                }
                KeyCode::Char(ch) => {
                    dbg::user(&format!("space menu: '{ch}'"));
                    if let Some(action) = self.nav.space_menu_handle(ch) {
                        dbg::system(&format!("space action: {:?}", action));
                        self.handle_space_action(action);
                    }
                }
                _ => {}
            }
            return;
        }

        // Space → open space menu
        if key.code == KeyCode::Char(' ') {
            dbg::user("Space → open space menu");
            self.nav.toggle_space_menu();
            return;
        }

        // Tab — blocked while piano roll is in column/row editing mode
        match key.code {
            KeyCode::Tab if self.nav.focused_pane == Pane::ClipView
                && self.nav.clip_view.piano_roll.focus != PianoRollFocus::Navigation => {
                // Tab blocked in column/row mode — controls are locked
                return;
            }
            KeyCode::Tab if self.nav.focused_pane == Pane::ClipView => {
                dbg::user("Tab → cycle clip view tab");
                self.nav.cycle_tab();
                return;
            }
            KeyCode::Tab => {
                dbg::user("Tab → next pane");
                self.nav.focus_next_pane();
                return;
            }
            KeyCode::BackTab => {
                dbg::user("Shift+Tab → prev pane");
                self.nav.focus_pane(self.nav.focused_pane.prev());
                return;
            }
            _ => {}
        }

        // Global BPM adjustment (+/- always work)
        match key.code {
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let bpm = self.engine.transport.tempo_bpm() + 1.0;
                self.engine.transport.set_tempo(bpm);
                dbg::system(&format!("bpm={:.0}", bpm));
                return;
            }
            KeyCode::Char('-') => {
                let bpm = (self.engine.transport.tempo_bpm() - 1.0).max(20.0);
                self.engine.transport.set_tempo(bpm);
                dbg::system(&format!("bpm={:.0}", bpm));
                return;
            }
            _ => {}
        }

        match self.nav.focused_pane {
            Pane::Transport => self.handle_transport_keys(key),
            Pane::Tracks => self.handle_tracks_keys(key),
            Pane::ClipView => self.handle_clip_view_keys(key),
        }
    }


    pub(crate) fn handle_transport_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crate::debug_log as dbg;
        let tu = &mut self.nav.transport_ui;

        if tu.editing {
            // Controls locked to the current element
            match tu.element {
                TransportElement::Bpm => match key.code {
                    KeyCode::Char('l') | KeyCode::Right => {
                        let bpm = self.engine.transport.tempo_bpm() + 1.0;
                        self.engine.transport.set_tempo(bpm);
                        dbg::system(&format!("bpm={:.0}", bpm));
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        let bpm = (self.engine.transport.tempo_bpm() - 1.0).max(20.0);
                        self.engine.transport.set_tempo(bpm);
                        dbg::system(&format!("bpm={:.0}", bpm));
                    }
                    KeyCode::Esc | KeyCode::Enter => {
                        dbg::user("transport: release BPM edit");
                        tu.editing = false;
                    }
                    _ => {}
                },
                TransportElement::Loop => {
                    // Delegate to loop editor
                    // Enter on loop when already editing → just unfocus
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => {
                            tu.editing = false;
                        }
                        _ => {}
                    }
                }
                _ => {
                    // Record and Metronome don't have editing mode, just release
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => { tu.editing = false; }
                        _ => {}
                    }
                }
            }
            return;
        }

        // Not editing — navigate between elements
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                tu.element = tu.element.move_left();
                dbg::user(&format!("transport: → {}", tu.element.label()));
            }
            KeyCode::Char('l') | KeyCode::Right => {
                tu.element = tu.element.move_right();
                dbg::user(&format!("transport: → {}", tu.element.label()));
            }
            KeyCode::Enter => {
                dbg::user(&format!("transport: Enter on {}", tu.element.label()));
                match tu.element {
                    TransportElement::Bpm => { tu.editing = true; }
                    TransportElement::Record => {
                        self.engine.transport.toggle_record();
                        dbg::system(&format!("recording={}", self.engine.transport.is_recording()));
                    }
                    TransportElement::Loop => {
                        self.nav.loop_editor.focus();
                    }
                    TransportElement::Metronome => {
                        self.engine.transport.toggle_metronome();
                        dbg::system(&format!("metronome={}", self.engine.transport.is_metronome_on()));
                    }
                }
            }
            KeyCode::Char('q') => { self.running = false; }
            KeyCode::Esc => { dbg::user("transport: Esc → deselect"); }
            _ => {}
        }
    }


    pub(crate) fn handle_tracks_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crate::debug_log as dbg;

        // ── Clip locked mode: Enter was pressed on a clip ──
        // h/l = move clip, Shift+H/L = stretch right edge, Ctrl+H/L = trim left edge
        // y/p/d/P = yank/paste/duplicate, Esc = unlock
        if self.nav.clip_locked {
            if let crate::state::TrackElement::Clip(idx) = self.nav.track_element {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => {
                        dbg::user("clip locked: Esc → unlock");
                        self.nav.escape();
                    }
                    // Ctrl+h/l: trim left edge
                    KeyCode::Char('h') | KeyCode::Left if ctrl => {
                        self.move_clip_left_edge(idx, -1);
                    }
                    KeyCode::Char('l') | KeyCode::Right if ctrl => {
                        self.move_clip_left_edge(idx, 1);
                    }
                    // Shift+H/L: stretch/shrink right edge
                    KeyCode::Char('H') | KeyCode::Char('h') | KeyCode::Left if shift => {
                        self.move_clip_right_edge(idx, -1);
                    }
                    KeyCode::Char('L') | KeyCode::Char('l') | KeyCode::Right if shift => {
                        self.move_clip_right_edge(idx, 1);
                    }
                    // Plain h/l: move clip left/right
                    KeyCode::Char('h') | KeyCode::Left => {
                        self.move_clip(idx, -1);
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        self.move_clip(idx, 1);
                    }
                    // Clip operations
                    KeyCode::Char('y') => { self.yank_clip(idx); }
                    KeyCode::Char('p') => { self.paste_clip_after(idx); }
                    KeyCode::Char('P') => { self.paste_clip_to_track(); }
                    KeyCode::Char('d') => { self.duplicate_clip(idx); }
                    _ => {}
                }
            }
            return;
        }

        // ── Normal tracks mode ──
        match key.code {
            KeyCode::Char('q') if !self.nav.track_selected && !self.nav.fx_menu.open => {
                dbg::user("q → quit");
                self.running = false;
            }
            KeyCode::Esc => {
                dbg::user("Esc → back");
                self.nav.escape();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                dbg::user(&format!("j/Down → move down (cursor was {})", self.nav.track_cursor));
                self.nav.move_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                dbg::user(&format!("k/Up → move up (cursor was {})", self.nav.track_cursor));
                self.nav.move_up();
            }
            KeyCode::Char('y')
                if self.nav.track_selected
                && matches!(self.nav.track_element, crate::state::TrackElement::Clip(_))
                && !self.nav.fx_menu.open =>
            {
                if let crate::state::TrackElement::Clip(idx) = self.nav.track_element {
                    self.yank_clip(idx);
                }
            }
            KeyCode::Char('p')
                if self.nav.track_selected
                && matches!(self.nav.track_element, crate::state::TrackElement::Clip(_))
                && !self.nav.fx_menu.open =>
            {
                if let crate::state::TrackElement::Clip(idx) = self.nav.track_element {
                    self.paste_clip_after(idx);
                }
            }
            KeyCode::Char('P')
                if self.nav.track_selected
                && !self.nav.fx_menu.open =>
            {
                self.paste_clip_to_track();
            }
            KeyCode::Char('d')
                if self.nav.track_selected
                && matches!(self.nav.track_element, crate::state::TrackElement::Clip(_))
                && !self.nav.fx_menu.open =>
            {
                if let crate::state::TrackElement::Clip(idx) = self.nav.track_element {
                    self.duplicate_clip(idx);
                }
            }
            KeyCode::Char('h') | KeyCode::Left => self.nav.move_left(),
            KeyCode::Char('l') | KeyCode::Right => self.nav.move_right(),
            KeyCode::Enter => {
                dbg::user(&format!("Enter → select (track_selected={})", self.nav.track_selected));
                self.nav.enter();
            }
            KeyCode::Char('m') if !self.nav.fx_menu.open => {
                dbg::user("m → toggle mute");
                self.nav.toggle_mute();
            }
            KeyCode::Char('s') if !self.nav.fx_menu.open => {
                dbg::user("s → toggle solo");
                self.nav.toggle_solo();
            }
            KeyCode::Char('r') if !self.nav.fx_menu.open => {
                dbg::user("r → toggle arm");
                self.nav.toggle_arm();
            }
            KeyCode::Char('R') if !self.nav.fx_menu.open => {
                dbg::user("R → toggle loop record");
                self.toggle_loop_record();
                self.log_transport_state();
            }
            KeyCode::Char(ch @ '0'..='9') if self.nav.track_selected && !self.nav.fx_menu.open => {
                self.nav.digit_input(ch);
            }
            _ => {}
        }
    }


    pub(crate) fn handle_clip_view_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crate::debug_log as dbg;
        use crate::state::PianoRollFocus;

        // Edit mode intercepts all keys
        if self.nav.clip_view.piano_roll.edit_mode {
            self.handle_edit_mode_keys(key);
            return;
        }

        // If we're in the FX panel side, use the old synth/fx controls
        if self.nav.clip_view.focus == ClipViewFocus::FxPanel {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.nav.escape(),
                KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
                KeyCode::Char('h') | KeyCode::Left => {
                    self.nav.move_left();
                    self.send_synth_param_update();
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.nav.move_right();
                    self.send_synth_param_update();
                }
                _ => {}
            }
            return;
        }

        // Piano roll side — route by focus level
        // Read focus level and state before any mutable borrows
        let focus = self.nav.clip_view.piano_roll.focus;
        let col = self.nav.clip_view.piano_roll.column;
        let cursor_note = self.nav.clip_view.piano_roll.cursor_note;

        match focus {
            // Browsing: h/l navigates columns, Enter selects a column
            PianoRollFocus::Navigation => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        // Clear any highlights first, otherwise escape
                        let pr = &self.nav.clip_view.piano_roll;
                        if pr.highlight_start.is_some() || pr.row_highlight_low.is_some() {
                            self.nav.clip_view.piano_roll.clear_all_highlights();
                        } else {
                            self.nav.escape();
                        }
                    }
                    KeyCode::Char('J') | KeyCode::Down if shift => {
                        // Shift+j or Shift+Down: start or expand row highlight downward
                        self.nav.clip_view.piano_roll.highlight_down();
                        dbg::user(&format!("piano roll: row highlight {:?}", self.nav.clip_view.piano_roll.row_highlight_range()));
                    }
                    KeyCode::Char('K') | KeyCode::Up if shift => {
                        // Shift+k or Shift+Up: start or expand row highlight upward
                        self.nav.clip_view.piano_roll.highlight_up();
                        dbg::user(&format!("piano roll: row highlight {:?}", self.nav.clip_view.piano_roll.row_highlight_range()));
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        // If rows are highlighted, moving j/k unhighlights them
                        if self.nav.clip_view.piano_roll.row_highlight_low.is_some() {
                            self.nav.clip_view.piano_roll.clear_row_highlight();
                        }
                        self.nav.clip_view.piano_roll.move_down();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        if self.nav.clip_view.piano_roll.row_highlight_low.is_some() {
                            self.nav.clip_view.piano_roll.clear_row_highlight();
                        }
                        self.nav.clip_view.piano_roll.move_up();
                    }
                    KeyCode::Char('H') if shift => {
                        // Shift+h: start or expand highlight left
                        self.nav.clip_view.piano_roll.start_highlight();
                        self.nav.clip_view.piano_roll.highlight_left();
                        dbg::user(&format!("piano roll: highlight left, range {:?}", self.nav.clip_view.piano_roll.highlight_range()));
                    }
                    KeyCode::Char('L') if shift => {
                        // Shift+l: start or expand highlight right
                        self.nav.clip_view.piano_roll.start_highlight();
                        self.nav.clip_view.piano_roll.highlight_right();
                        dbg::user(&format!("piano roll: highlight right, range {:?}", self.nav.clip_view.piano_roll.highlight_range()));
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        self.nav.clip_view.piano_roll.move_column_left();
                        dbg::user(&format!("piano roll: col {}", self.nav.clip_view.piano_roll.column_display()));
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        self.nav.clip_view.piano_roll.move_column_right();
                        dbg::user(&format!("piano roll: col {}", self.nav.clip_view.piano_roll.column_display()));
                    }
                    KeyCode::Char('d') => {
                        // Delete notes in highlighted region (columns, rows, or both)
                        let col_range = self.nav.clip_view.piano_roll.highlight_range();
                        let row_range = self.nav.clip_view.piano_roll.row_highlight_range();
                        if col_range.is_some() || row_range.is_some() {
                            self.delete_selected_notes(col_range, row_range);
                            self.nav.clip_view.piano_roll.clear_all_highlights();
                            self.send_clip_update();
                            // Kill any currently sounding notes from the deleted events
                            self.engine.panic();
                            dbg::user("piano roll: deleted highlighted notes");
                        }
                    }
                    KeyCode::Char('y') => {
                        // Yank notes in highlighted region
                        let col_range = self.nav.clip_view.piano_roll.highlight_range();
                        let row_range = self.nav.clip_view.piano_roll.row_highlight_range();
                        if col_range.is_some() || row_range.is_some() {
                            self.yank_selected_notes(col_range, row_range);
                            self.nav.clip_view.piano_roll.clear_all_highlights();
                            dbg::user("piano roll: yanked highlighted notes");
                        }
                    }
                    KeyCode::Char('p') => {
                        // Paste yanked notes at highlighted position or cursor
                        let col_start = self.nav.clip_view.piano_roll.highlight_range()
                            .map(|(s, _)| s)
                            .unwrap_or(self.nav.clip_view.piano_roll.column);

                        // Row offset: shift yanked notes so the highest yanked note
                        // lands on the highest highlighted row (or cursor note)
                        let yank_buf = &self.nav.clip_view.piano_roll.yank_buffer;
                        let yank_max = yank_buf.iter().map(|n| n.note).max().unwrap_or(60);
                        let target_note = self.nav.clip_view.piano_roll.row_highlight_range()
                            .map(|(_, hi)| hi)
                            .unwrap_or(self.nav.clip_view.piano_roll.cursor_note);
                        let row_offset = Some(target_note as i16 - yank_max as i16);

                        self.paste_selected_notes(col_start, row_offset);
                        self.nav.clip_view.piano_roll.clear_all_highlights();
                        self.send_clip_update();
                        dbg::user(&format!("piano roll: pasted notes (shift={})", row_offset.unwrap_or(0)));
                    }
                    KeyCode::Enter => {
                        let indices = self.note_indices_in_column(col);
                        dbg::user(&format!("piano roll: Enter → column {} selected ({} notes)", self.nav.clip_view.piano_roll.column_display(), indices.len()));
                        self.nav.clip_view.piano_roll.enter(indices);
                    }
                    KeyCode::Char(ch @ '0'..='9') => {
                        if self.nav.clip_view.piano_roll.type_digit(ch) {
                            dbg::user(&format!("piano roll: jump to col {}", self.nav.clip_view.piano_roll.column_display()));
                        }
                    }
                    _ => {}
                }
            }

            // Column selected (Right Left Trick):
            //   h/l = adjust LEFT edge of ALL notes in column
            //   H/L = adjust RIGHT edge of ALL notes in column
            //   j/k = go deeper → individual note (Row mode)
            //   Esc = back to Browsing
            PianoRollFocus::Selected => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Esc => {
                        dbg::user("piano roll: Esc → browsing");
                        self.nav.clip_view.piano_roll.escape();
                    }
                    KeyCode::Char('h') | KeyCode::Left if !shift => {
                        self.adjust_column_edges(-0.01, false);
                        self.send_clip_update();
                        dbg::user("piano roll: col left \u{2190}");
                    }
                    KeyCode::Char('l') | KeyCode::Right if !shift => {
                        self.adjust_column_edges(0.01, false);
                        self.send_clip_update();
                        dbg::user("piano roll: col left \u{2192}");
                    }
                    KeyCode::Char('H') | KeyCode::Char('h') | KeyCode::Left => {
                        self.adjust_column_edges(-0.01, true);
                        self.send_clip_update();
                        dbg::user("piano roll: col right \u{2190}");
                    }
                    KeyCode::Char('L') | KeyCode::Char('l') | KeyCode::Right => {
                        self.adjust_column_edges(0.01, true);
                        self.send_clip_update();
                        dbg::user("piano roll: col right \u{2192}");
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        // Enter row mode starting at the top of the visible area
                        let pr = &mut self.nav.clip_view.piano_roll;
                        let top = pr.view_bottom_note.saturating_add(pr.view_height).saturating_sub(1);
                        pr.cursor_note = top.min(127);
                        pr.enter_row();
                        dbg::user(&format!("piano roll: → row at note {}", pr.cursor_note));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let pr = &mut self.nav.clip_view.piano_roll;
                        let top = pr.view_bottom_note.saturating_add(pr.view_height).saturating_sub(1);
                        pr.cursor_note = top.min(127);
                        pr.enter_row();
                        dbg::user(&format!("piano roll: → row at note {}", pr.cursor_note));
                    }
                    _ => {}
                }
            }

            // Row selected (Right Left Trick on single note):
            //   h/l = adjust LEFT edge of this note
            //   H/L = adjust RIGHT edge of this note
            //   j/k = move to next/prev note in column
            //   Esc = back to Column (column-level control restored)
            PianoRollFocus::Row => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    KeyCode::Esc => {
                        dbg::user("piano roll: Esc → column mode");
                        self.nav.clip_view.piano_roll.escape();
                    }
                    KeyCode::Char('h') | KeyCode::Left if !shift => {
                        self.adjust_note_edge(col, cursor_note, -0.01, false);
                        self.send_clip_update();
                        dbg::user("piano roll: note left \u{2190}");
                    }
                    KeyCode::Char('l') | KeyCode::Right if !shift => {
                        self.adjust_note_edge(col, cursor_note, 0.01, false);
                        self.send_clip_update();
                        dbg::user("piano roll: note left \u{2192}");
                    }
                    KeyCode::Char('H') | KeyCode::Char('h') | KeyCode::Left => {
                        self.adjust_note_edge(col, cursor_note, -0.01, true);
                        self.send_clip_update();
                        dbg::user("piano roll: note right \u{2190}");
                    }
                    KeyCode::Char('L') | KeyCode::Char('l') | KeyCode::Right => {
                        self.adjust_note_edge(col, cursor_note, 0.01, true);
                        self.send_clip_update();
                        dbg::user("piano roll: note right \u{2192}");
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.nav.clip_view.piano_roll.move_down();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.nav.clip_view.piano_roll.move_up();
                    }
                    KeyCode::Char('n') => {
                        self.draw_note(col, cursor_note);
                        self.send_clip_update();
                        dbg::user(&format!("piano roll: draw note {} at col {}", cursor_note, col + 1));
                    }
                    _ => {}
                }
            }
        }
    }
}
