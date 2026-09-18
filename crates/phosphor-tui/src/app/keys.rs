//! App methods: keys.

use super::*;

impl App {
    /// One keystroke, and then the bookkeeping that has to happen whichever
    /// way through [`App::dispatch_event`] it went.
    ///
    /// The dispatcher below returns from forty places, which is the right
    /// shape for a key router and the wrong one for "and afterwards, check
    /// this". So the door is here: everything that must be true after *any*
    /// key belongs in this function, where it cannot be skipped by an early
    /// return somebody adds next year.
    pub(crate) fn handle_event(&mut self, event: Event) {
        self.dispatch_event(event);
        // A sampler audition is a sound the engine holds until it is told to
        // stop, and most of the ways out of the pad map are keys that know
        // nothing about it.
        self.reconcile_sampler_preview();
        // Source mode borrows a track's plugin slot, and the keys that can
        // take that track away — delete, undo, a session load — know
        // nothing about the borrow.
        self.reconcile_sampler_source();
        // Root-learn is armed at one zone, and an arming left standing
        // would take the next note played anywhere in the box and retune
        // that zone with it.
        self.reconcile_sampler_learn();
    }

    fn dispatch_event(&mut self, event: Event) {
        use crate::debug_log as dbg;

        let Event::Key(key) = event else { return };

        // One keystroke, one trip through everything below.
        //
        // A Unix terminal reports a keystroke once and there is nothing else
        // to report, so this used to be the whole story. The Windows console
        // reports the key going down *and* coming back up — `ReadConsoleInput`
        // hands back both records and crossterm turns the second into
        // `KeyEventKind::Release` — so every action in this function ran
        // twice per key. A note played twice, a selector stepped two
        // positions, and worst of all a toggle flipped and flipped straight
        // back, which reads as a control that does nothing at all.
        //
        // `Repeat` is treated as a press. Holding a key is how a knob gets
        // swept, and a Unix terminal's own auto-repeat already arrives here as
        // a stream of presses; dropping the kind that means the same thing
        // would make a held key worse than it is now. Nothing emits `Repeat`
        // as this is built — crossterm reports it only for the kitty keyboard
        // protocol, which needs `PushKeyboardEnhancementFlags` and this
        // application never sends it — so accepting it costs nothing today and
        // keeps held keys working on the day it does.
        //
        // Filtered here rather than beside `event::read`, because this is the
        // door the tests knock on; a filter the tests cannot reach is a filter
        // that gets removed.
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }

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

        // Undo (u) / Redo (Ctrl+r) — global, like every DAW's.
        //
        // Blocked only where `u` is a letter (text entry) or an answer
        // (menus and modals). A locked fader, a locked sequencer grid and an
        // open effect panel all let it through: none of them bind `u` to
        // anything, and a key that undoes here and does nothing there is a
        // key nobody can trust. Edit mode routes its own `u` so its submodes
        // can decide.
        if key.code == KeyCode::Char('u') {
            dbg::user(&format!("u key received: modifiers={:?} space={} input={} confirm={} instr={} fx={}",
                key.modifiers, self.nav.space_menu.open, self.nav.input_modal.open,
                self.nav.confirm_modal.open, self.nav.instrument_modal.open, self.nav.fx_menu.open));
        }
        if !self.nav.space_menu.open && !self.nav.input_modal.open && !self.nav.confirm_modal.open
            && !self.nav.instrument_modal.open && !self.nav.fx_menu.open
            && !self.nav.preset_modal.open
            && !self.nav.file_picker.open
            && !self.nav.prog_editor.open
            && !self.nav.practice.open
            && !self.nav.clip_view.piano_roll.edit_mode
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
                    self.execute_confirm(kind);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.nav.confirm_modal.close();
                    // Whatever the answer was about is no longer pending.
                    self.nav.preset_modal.pending_name.clear();
                }
                _ => {}
            }
            return;
        }

        // Quantize modal — j/k navigate, h/l adjust, Enter applies
        // The practice room's keys, when it is open. Unclaimed keys fall
        // through, so the space menu and the transport stay reachable.
        if self.nav.practice.open
            && !self.nav.input_modal.open
            && !self.nav.file_picker.open
            && !self.nav.space_menu.open
            && self.handle_practice_keys(key)
        {
            return;
        }

        // After the input modal's check by way of the guard here: the name
        // prompt opens over the editor, and typing must land in the field.
        if self.nav.prog_editor.open && !self.nav.input_modal.open {
            self.handle_prog_editor_keys(key);
            return;
        }

        if self.nav.quantize_modal.open {
            match key.code {
                KeyCode::Esc => { self.nav.quantize_modal.close(); }
                KeyCode::Char('j') | KeyCode::Down => { self.nav.quantize_modal.move_down(); }
                KeyCode::Char('k') | KeyCode::Up => { self.nav.quantize_modal.move_up(); }
                KeyCode::Char('h') | KeyCode::Left => { self.nav.quantize_modal.adjust(-1); }
                KeyCode::Char('l') | KeyCode::Right => { self.nav.quantize_modal.adjust(1); }
                KeyCode::Enter => {
                    if self.nav.quantize_modal.cursor == 2 {
                        let grid = self.nav.quantize_modal.grid;
                        let strength = self.nav.quantize_modal.strength;
                        self.nav.quantize_modal.close();
                        self.apply_quantize(grid, strength);
                    }
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
                    // What was typed, or the suggestion when nothing was —
                    // never a filename the player did not choose.
                    let path = self.nav.input_modal.resolved();
                    let kind = self.nav.input_modal.kind;
                    self.nav.input_modal.close();
                    if !path.is_empty() {
                        match kind {
                            InputModalKind::SaveAs => self.do_save(&path),
                            InputModalKind::Open => self.do_load(&path),
                            InputModalKind::PresetName => self.request_preset_save(&path),
                            InputModalKind::RenameTrack => self.do_rename_track(&path),
                            InputModalKind::ProgressionName => {
                                let trimmed = path.trim().to_string();
                                if !trimmed.is_empty() {
                                    self.nav.prog_editor.name = trimmed;
                                }
                            }
                            InputModalKind::SamplePath => self.do_load_sample(&path),
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

        // The file picker — the list that opens a project or puts a sound
        // on a pad. Checked after the input modal for the preset browser's
        // reason: `/` swaps the list for a typed path, and while that field
        // is up it has the keys.
        //
        // Nothing falls out of the handler below. Every letter in it is a
        // filter character, and a letter that reached the pane underneath
        // would be a `d` inside a filename deleting something.
        if self.nav.file_picker.open {
            self.handle_file_picker_keys(key);
            return;
        }

        // Preset browser — j/k navigate, Enter saves or loads, d deletes.
        // Checked after the confirm and input modals, because both of those
        // are raised *from* here: the name prompt and the overwrite/delete
        // questions take the keys while they are up, and the browser is still
        // underneath when they close.
        if self.nav.preset_modal.open {
            match key.code {
                KeyCode::Esc => {
                    dbg::user("preset browser: close");
                    self.nav.preset_modal.close();
                }
                KeyCode::Char('j') | KeyCode::Down => self.nav.preset_modal.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.preset_modal.move_up(),
                KeyCode::Enter => match self.nav.preset_modal.selected_preset() {
                    Some(index) => self.do_load_preset(index),
                    None => self.request_preset_name(),
                },
                KeyCode::Char('d') => self.request_preset_delete(),
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
                        self.edit_loop_range(|l| l.move_end_left());
                    } else {
                        dbg::user("loop editor: h → move start left");
                        self.edit_loop_range(|l| l.move_start_left());
                    }
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    if shift {
                        dbg::user("loop editor: Shift+l → move end right");
                        self.edit_loop_range(|l| l.move_end_right());
                    } else {
                        dbg::user("loop editor: l → move start right");
                        self.edit_loop_range(|l| l.move_start_right());
                    }
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                }
                KeyCode::Char('H') => {
                    dbg::user("loop editor: H → move end left");
                    self.edit_loop_range(|l| l.move_end_left());
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                }
                KeyCode::Char('L') => {
                    dbg::user("loop editor: L → move end right");
                    self.edit_loop_range(|l| l.move_end_right());
                    dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
                }
                // The brace is the cursor: j/k walk it along the song by
                // one grid step, J/K leap it by its own length.
                KeyCode::Char('j') | KeyCode::Down => self.slide_loop_brace(true, false),
                KeyCode::Char('k') | KeyCode::Up => self.slide_loop_brace(false, false),
                KeyCode::Char('J') => self.slide_loop_brace(true, true),
                KeyCode::Char('K') => self.slide_loop_brace(false, true),
                KeyCode::Char('y') => self.yank_loop_section(),
                KeyCode::Char('x') | KeyCode::Char('d') => self.cut_loop_section(),
                KeyCode::Char('p') => self.paste_loop_section(true),
                KeyCode::Char('P') => self.paste_loop_section(false),
                KeyCode::Char('g') => {
                    self.nav.loop_editor.cycle_step();
                    self.flash(format!(
                        "loop grid: {} \u{00b7} markers move by it",
                        self.nav.loop_editor.step.label()
                    ));
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
                    let target = self.nav.instrument_modal.target;
                    dbg::user(&format!("instrument modal: Enter → selected {:?}", instrument));
                    self.nav.instrument_modal.open = false;
                    // One menu, two answers. Which one this is was decided
                    // by the door that opened it — see `InstrumentPick`.
                    match target {
                        crate::state::InstrumentPick::NewTrack => {
                            self.create_instrument_track_undoable(instrument);
                        }
                        pick => self.enter_sampler_source(pick, instrument),
                    }
                }
                _ => {}
            }
            return;
        }

        // Space menu open
        if self.nav.space_menu.open {
            // A help card takes the keys while it is up: j/k read it, Esc
            // puts it down. The shortcuts underneath do not fire — a page of
            // text is not a menu, and `p` in the middle of one should not
            // start the transport.
            if self.nav.space_menu.topic.is_some() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        self.nav.space_menu.close_topic();
                    }
                    KeyCode::Char(' ') => {
                        self.nav.space_menu.close_topic();
                        self.nav.space_menu.open = false;
                    }
                    KeyCode::Char('j') | KeyCode::Down => self.nav.space_menu.scroll_body(1),
                    KeyCode::Char('k') | KeyCode::Up => self.nav.space_menu.scroll_body(-1),
                    KeyCode::PageDown => self.nav.space_menu.scroll_body(8),
                    KeyCode::PageUp => self.nav.space_menu.scroll_body(-8),
                    KeyCode::Char('g') | KeyCode::Home => self.nav.space_menu.scroll_body(-999),
                    KeyCode::Char('G') | KeyCode::End => self.nav.space_menu.scroll_body(999),
                    _ => {}
                }
                return;
            }
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

        // Space → open space menu (blocked in edit mode when in clip view,
        // and while source mode has the track's plugin slot on loan).
        //
        // Source mode refuses every other key in words — "source mode is on
        // pad C3 · r records · esc puts the sampler back" — because while it
        // is on the sampler is out of the slot and nothing else on the screen
        // means what it says. The space menu is a door into saving, loading
        // and half the box, and it was the one key that walked straight past
        // the refusal. It is refused here rather than swallowed, and in the
        // mode's own words, so the answer is the same from every tab.
        let in_edit = self.nav.focused_pane == Pane::ClipView && self.nav.clip_view.piano_roll.edit_mode;
        if key.code == KeyCode::Char(' ') && !in_edit {
            if self.in_sampler_source() {
                dbg::user("Space → refused: source mode is on");
                self.flash_sampler_source_keys();
                return;
            }
            dbg::user("Space → open space menu");
            self.nav.toggle_space_menu();
            return;
        }

        // Tab — blocked while piano roll is in column/row editing mode, and
        // while a step grid knob is being held: a locked control takes every
        // key, or "h adjusts" and "h moves" are the same press.
        //
        // The trim strip is blocked on the same grounds, and it is the same
        // stated contract: the strip "owns every key while it is open", so
        // `esc` is the way out of it and Tab is not. A held knob and a mode
        // with its own key table are the same promise from the player's side,
        // and two guards that said different things would be the kind of
        // difference nobody can predict.
        match key.code {
            KeyCode::Tab | KeyCode::BackTab
                if self.nav.focused_pane == Pane::ClipView
                    && ((self.nav.clip_view.clip_tab == ClipTab::Sequencer
                        && self.nav.clip_view.sequencer.locked)
                        || (self.nav.clip_view.clip_tab == ClipTab::Fx
                            && self.nav.clip_view.fx.locked)
                        || (self.nav.clip_view.clip_tab == ClipTab::Pads
                            && (self.nav.clip_view.sampler.locked
                                || self.nav.clip_view.sampler.trim.is_some()))) =>
            {
                return;
            }
            KeyCode::Tab if self.nav.focused_pane == Pane::ClipView
                && (self.nav.clip_view.piano_roll.focus != PianoRollFocus::Navigation
                    || self.nav.clip_view.piano_roll.edit_mode) => {
                // Tab blocked in column/row/edit mode — controls are locked
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
                self.nudge_tempo(1.0);
                return;
            }
            KeyCode::Char('-') => {
                self.nudge_tempo(-1.0);
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

        if self.nav.transport_ui.editing {
            // Controls locked to the current element
            match self.nav.transport_ui.element {
                TransportElement::Bpm => match key.code {
                    KeyCode::Char('l') | KeyCode::Right => {
                        self.nudge_tempo(1.0);
                    }
                    KeyCode::Char('h') | KeyCode::Left => {
                        self.nudge_tempo(-1.0);
                    }
                    KeyCode::Esc | KeyCode::Enter => {
                        dbg::user("transport: release BPM edit");
                        self.nav.transport_ui.editing = false;
                    }
                    _ => {}
                },
                // Loop delegates to the loop editor; Record and Metronome
                // have no editing mode. Enter or Esc releases any of them.
                _ => {
                    if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                        self.nav.transport_ui.editing = false;
                    }
                }
            }
            return;
        }

        // Not editing — navigate between elements
        let tu = &mut self.nav.transport_ui;
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
                    TransportElement::RecordMode => {
                        self.nav.record_replace = !self.nav.record_replace;
                        self.flash(if self.nav.record_replace {
                            "take mode: re-record · R clears the loop range first"
                        } else {
                            "take mode: overdub · passes layer up"
                        });
                    }
                    TransportElement::CountIn => {
                        let bars = self.engine.transport.cycle_count_in();
                        dbg::system(&format!("count-in bars={bars}"));
                        self.flash(match bars {
                            0 => "count-in: off".to_string(),
                            1 => "count-in: 1 bar before recording".to_string(),
                            n => format!("count-in: {n} bars before recording"),
                        });
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

        // ── Element locked mode: Enter was pressed on the selected element ──
        // Clip: h/l = move, Shift+H/L = stretch right edge, Ctrl+H/L = trim
        //       left edge, y/p/d/P = yank/paste/duplicate, Esc = unlock
        // Volume: h/l = fader down/up, Esc or Enter = release
        if self.nav.element_locked {
            // Pan and the sends: the fader's contract on the routing cells.
            // The readout goes to the status bar because the header cell has
            // three characters and a send that reads `-12` is not the same
            // information as "send A, twelve decibels down".
            if matches!(
                self.nav.track_element,
                crate::state::TrackElement::Pan
                    | crate::state::TrackElement::SendA
                    | crate::state::TrackElement::SendB
            ) {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter => self.nav.escape(),
                    KeyCode::Char('h') | KeyCode::Left => self.step_routing(-1),
                    KeyCode::Char('l') | KeyCode::Right => self.step_routing(1),
                    KeyCode::Char('H') => self.step_routing(-5),
                    KeyCode::Char('L') => self.step_routing(5),
                    _ => {}
                }
                return;
            }
            if self.nav.track_element == crate::state::TrackElement::Volume {
                match key.code {
                    KeyCode::Esc | KeyCode::Enter => {
                        dbg::user("fader: release");
                        self.nav.escape();
                    }
                    KeyCode::Char('h') | KeyCode::Left => self.step_fader(-1),
                    KeyCode::Char('l') | KeyCode::Right => self.step_fader(1),
                    _ => {}
                }
                return;
            }
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
            // On the label, y takes the whole arrangement — every clip with
            // its bars — so P can lay it under another instrument.
            KeyCode::Char('y')
                if self.nav.track_selected
                && self.nav.track_element == crate::state::TrackElement::Label
                && !self.nav.fx_menu.open =>
            {
                self.yank_all_clips();
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
            // The layering gesture's other half: double the whole track —
            // instrument, panel, effects, clips — directly below itself.
            KeyCode::Char('D')
                if self.nav.track_selected && !self.nav.fx_menu.open =>
            {
                self.duplicate_current_track();
            }
            // On the label, n names the track.
            KeyCode::Char('n')
                if self.nav.track_selected
                && self.nav.track_element == crate::state::TrackElement::Label
                && !self.nav.fx_menu.open =>
            {
                if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
                    if track.is_live() {
                        let current = track.name.clone();
                        self.nav.input_modal.open_rename(&current);
                    }
                }
            }
            KeyCode::Char('h') | KeyCode::Left => self.nav.move_left(),
            KeyCode::Char('l') | KeyCode::Right => self.nav.move_right(),
            KeyCode::Enter if self.nav.fx_menu.open => {
                dbg::user("Enter → choose fx");
                self.fx_menu_choose();
            }
            KeyCode::Enter => {
                dbg::user(&format!("Enter → select (track_selected={})", self.nav.track_selected));
                self.nav.enter();
                if self.nav.element_locked
                    && self.nav.track_element == crate::state::TrackElement::Volume
                {
                    self.status_message = Some((
                        "fader: h/l = -1/+1 dB, Esc to release".into(),
                        std::time::Instant::now(),
                    ));
                }
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

        // The chain list takes its own keys: `b`, `[`, `]`, `d` and `a` mean
        // nothing to a parameter strip and everything to a slot list, and
        // Enter opens a panel rather than adjusting anything.
        // The menu-open case stays INSIDE the handler: skipping the handler
        // while the menu was up sent Enter to the generic panel branch below,
        // and the chosen effect was never added.
        if self.nav.clip_view.focus == ClipViewFocus::FxPanel
            && self.nav.clip_view.fx_panel_tab == FxPanelTab::TrackFx
            && self.handle_fx_chain_keys(key)
        {
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
                // A sampler's panel is also where sounds go on — and this
                // strip is where a fresh, clipless track lands, so the
                // gesture works from the first moment the track exists.
                // A no-op on every other instrument.
                KeyCode::Char('a') => self.open_sample_prompt(),
                _ => {}
            }
            return;
        }

        // An effect's panel takes its own keys, for the same reason the step
        // grid does: every one of h, j, k, l, Enter and the digits means
        // something different on a grid of bands.
        if self.nav.clip_view.focus == ClipViewFocus::PianoRoll
            && self.nav.clip_view.clip_tab == ClipTab::Fx
        {
            self.handle_fx_panel_keys(key);
            return;
        }

        // The step grid takes its own keys. Checked before the tabs below it
        // because every one of them — j, k, h, l, Enter, the digits — means
        // something different on a step grid, and a key that falls through to
        // the piano roll from here would move a cursor nobody can see.
        if self.nav.clip_view.focus == ClipViewFocus::PianoRoll
            && self.nav.clip_view.clip_tab == ClipTab::Sequencer
        {
            self.handle_sequencer_keys(key);
            return;
        }

        // The pad map takes its own keys, for the step grid's reason: h, l,
        // the digits and Enter all mean something different on a keyboard
        // full of pads, and a key that fell through from here would move a
        // cursor nobody can see.
        if self.nav.clip_view.focus == ClipViewFocus::PianoRoll
            && self.nav.clip_view.clip_tab == ClipTab::Pads
        {
            self.handle_sampler_keys(key);
            return;
        }

        // The instrument tab is the instrument's own panel, so it takes the
        // keys the panel takes. Without this the tab rendered controls and
        // the keys fell through to the piano roll underneath it, which is
        // what made it feel like a mock-up: it looked like a panel and
        // nothing typed into it arrived anywhere.
        //
        // In source mode this is the *borrowed* instrument's panel — see
        // `NavState::panel` — and the keys are the same keys, deliberately.
        // `Esc` included: on this tab it means "out of the panel" on every
        // track in the box, and a second meaning here would be the one key
        // that tore a mode down from a place that never says the mode is on.
        // The mode's own `Esc` is on the pads tab, where the banner offering
        // it is.
        if self.nav.clip_view.focus == ClipViewFocus::PianoRoll
            && self.nav.clip_view.clip_tab == ClipTab::InstConfig
        {
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
                // On a sampler, the panel is also where sounds go on: `a`
                // asks for a file for the current pad — the pad the keys
                // last played. Other instruments have nothing to add.
                KeyCode::Char('a') => self.open_sample_prompt(),
                _ => {}
            }
            return;
        }

        // Settings tab — route directly to nav methods which handle settings internally
        if self.nav.clip_view.focus == ClipViewFocus::PianoRoll
            && self.nav.clip_view.clip_tab == ClipTab::Settings
        {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.nav.escape(),
                KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
                KeyCode::Char('h') | KeyCode::Left => self.nav.move_left(),
                KeyCode::Char('l') | KeyCode::Right => self.nav.move_right(),
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

                // Automation lane has the keys: h/l walk columns (shared with
                // the note grid, so the highlight lines up), k/j raise/lower
                // the curve at the cursor and draw it, [ ] change lane, d
                // clears the point, A hands the keys back, Esc closes.
                if self.nav.clip_view.piano_roll.automation_focus {
                    match key.code {
                        KeyCode::Char('h') | KeyCode::Left => {
                            self.nav.clip_view.piano_roll.move_column_left();
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            self.nav.clip_view.piano_roll.move_column_right();
                        }
                        KeyCode::Char('k') | KeyCode::Up => self.automation_draw(if shift { 16 } else { 4 }),
                        KeyCode::Char('j') | KeyCode::Down => self.automation_draw(if shift { -16 } else { -4 }),
                        KeyCode::Char('[') => self.automation_cycle_stream(-1),
                        KeyCode::Char(']') => self.automation_cycle_stream(1),
                        KeyCode::Char('d') => self.automation_clear_point(),
                        KeyCode::Char('r') => self.automation_ramp(),
                        KeyCode::Char('A') => self.toggle_automation_lane(),
                        KeyCode::Esc => self.close_automation_lane(),
                        _ => {}
                    }
                    return;
                }

                // Highlight-locked stretch mode: Enter was pressed while highlights
                // were active. h/l adjusts left edge, H/L adjusts right edge.
                if self.nav.clip_view.piano_roll.highlight_locked {
                    let step = self.nav.clip_view.piano_roll.grid
                        .step_ticks(phosphor_core::transport::Transport::PPQ);
                    match key.code {
                        KeyCode::Esc => {
                            self.nav.clip_view.piano_roll.highlight_locked = false;
                            dbg::user("piano roll: stretch unlocked");
                        }
                        KeyCode::Char('h') | KeyCode::Left if !shift => {
                            self.stretch_highlighted_notes(-step, false);
                        }
                        KeyCode::Char('l') | KeyCode::Right if !shift => {
                            self.stretch_highlighted_notes(step, false);
                        }
                        KeyCode::Char('H') | KeyCode::Char('h') | KeyCode::Left if shift => {
                            self.stretch_highlighted_notes(-step, true);
                        }
                        KeyCode::Char('L') | KeyCode::Char('l') | KeyCode::Right if shift => {
                            self.stretch_highlighted_notes(step, true);
                        }
                        KeyCode::Char('d') => {
                            let col_range = self.nav.clip_view.piano_roll.highlight_range();
                            let row_range = self.nav.clip_view.piano_roll.row_highlight_range();
                            self.delete_selected_notes(col_range, row_range);
                            self.nav.clip_view.piano_roll.clear_all_highlights();
                            self.send_clip_update();
                            self.engine.panic();
                            dbg::user("piano roll: deleted highlighted notes");
                        }
                        _ => {}
                    }
                    return;
                }

                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
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
                        let has_highlights = self.nav.clip_view.piano_roll.has_highlights();
                        if has_highlights {
                            // Move highlighted notes down by 1 semitone
                            self.move_highlighted_notes(0, -1);
                        } else {
                            self.nav.clip_view.piano_roll.move_down();
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let has_highlights = self.nav.clip_view.piano_roll.has_highlights();
                        if has_highlights {
                            // Move highlighted notes up by 1 semitone
                            self.move_highlighted_notes(0, 1);
                        } else {
                            self.nav.clip_view.piano_roll.move_up();
                        }
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
                        let has_highlights = self.nav.clip_view.piano_roll.has_highlights();
                        if has_highlights {
                            self.move_highlighted_notes(-1, 0);
                        } else {
                            self.nav.clip_view.piano_roll.move_column_left();
                            dbg::user(&format!("piano roll: col {}", self.nav.clip_view.piano_roll.column_display()));
                        }
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        let has_highlights = self.nav.clip_view.piano_roll.has_highlights();
                        if has_highlights {
                            self.move_highlighted_notes(1, 0);
                        } else {
                            self.nav.clip_view.piano_roll.move_column_right();
                            dbg::user(&format!("piano roll: col {}", self.nav.clip_view.piano_roll.column_display()));
                        }
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
                        let row_offset = target_note as i16 - yank_max as i16;

                        self.paste_selected_notes(col_start, Some(row_offset));
                        self.nav.clip_view.piano_roll.clear_all_highlights();
                        self.send_clip_update();
                        dbg::user(&format!("piano roll: pasted notes (shift={row_offset})"));
                    }
                    KeyCode::Char('n') => {
                        self.draw_note(col, cursor_note);
                        self.send_clip_update();
                        dbg::user(&format!("piano roll: toggle note {} at col {}", cursor_note, col + 1));
                    }
                    KeyCode::Char('X') => {
                        self.clear_clip_controls();
                    }
                    KeyCode::Char('A') => {
                        self.toggle_automation_lane();
                    }
                    // Coarse vertical: a whole octave a press, for reaching a
                    // bass note or a lead line without walking every semitone.
                    KeyCode::Char('}') => {
                        self.nav.clip_view.piano_roll.jump_octave_up();
                    }
                    KeyCode::Char('{') => {
                        self.nav.clip_view.piano_roll.jump_octave_down();
                    }
                    // Snap to the nearest pitch that has a note, so editing an
                    // existing note never means scrolling to find it.
                    KeyCode::Char(']') => {
                        self.snap_note_up();
                    }
                    // Go to the playhead: the column the music is on right
                    // now, which is where the note that just sounded wrong
                    // lives.
                    KeyCode::Char('g') => {
                        self.jump_to_playhead();
                    }
                    KeyCode::Char('[') => {
                        self.snap_note_down();
                    }
                    KeyCode::Enter => {
                        let has_highlights = self.nav.clip_view.piano_roll.has_highlights();
                        if has_highlights {
                            // Lock highlights for stretching (Right-Left-Trick on highlighted group)
                            self.nav.clip_view.piano_roll.highlight_locked = true;
                            self.status_message = Some(("stretch mode: h/l=left edge, H/L=right edge, Esc=unlock".into(), std::time::Instant::now()));
                            dbg::user("piano roll: Enter → highlight locked for stretching");
                        } else {
                            let indices = self.note_indices_in_column(col);
                            dbg::user(&format!("piano roll: Enter → column {} selected ({} notes)", self.nav.clip_view.piano_roll.column_display(), indices.len()));
                            self.nav.clip_view.piano_roll.enter(indices);
                        }
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
                let step = self.nav.clip_view.piano_roll.grid
                    .step_ticks(phosphor_core::transport::Transport::PPQ);
                match key.code {
                    KeyCode::Esc => {
                        dbg::user("piano roll: Esc → browsing");
                        self.nav.clip_view.piano_roll.escape();
                    }
                    KeyCode::Char('h') | KeyCode::Left if !shift => {
                        self.adjust_column_edges(-step, false);
                        self.send_clip_update();
                        dbg::user("piano roll: col left edge \u{2190}");
                    }
                    KeyCode::Char('l') | KeyCode::Right if !shift => {
                        self.adjust_column_edges(step, false);
                        self.send_clip_update();
                        dbg::user("piano roll: col left edge \u{2192}");
                    }
                    KeyCode::Char('H') | KeyCode::Char('h') | KeyCode::Left => {
                        self.adjust_column_edges(-step, true);
                        self.send_clip_update();
                        dbg::user("piano roll: col right edge \u{2190}");
                    }
                    KeyCode::Char('L') | KeyCode::Char('l') | KeyCode::Right => {
                        self.adjust_column_edges(step, true);
                        self.send_clip_update();
                        dbg::user("piano roll: col right edge \u{2192}");
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
                let step = self.nav.clip_view.piano_roll.grid
                    .step_ticks(phosphor_core::transport::Transport::PPQ);
                match key.code {
                    KeyCode::Esc => {
                        dbg::user("piano roll: Esc → column mode");
                        self.nav.clip_view.piano_roll.escape();
                    }
                    KeyCode::Char('h') | KeyCode::Left if !shift => {
                        self.adjust_note_edge(col, cursor_note, -step, false);
                        self.send_clip_update();
                        dbg::user("piano roll: note left edge \u{2190}");
                    }
                    KeyCode::Char('l') | KeyCode::Right if !shift => {
                        self.adjust_note_edge(col, cursor_note, step, false);
                        self.send_clip_update();
                        dbg::user("piano roll: note left edge \u{2192}");
                    }
                    KeyCode::Char('H') | KeyCode::Char('h') | KeyCode::Left => {
                        self.adjust_note_edge(col, cursor_note, -step, true);
                        self.send_clip_update();
                        dbg::user("piano roll: note right edge \u{2190}");
                    }
                    KeyCode::Char('L') | KeyCode::Char('l') | KeyCode::Right => {
                        self.adjust_note_edge(col, cursor_note, step, true);
                        self.send_clip_update();
                        dbg::user("piano roll: note right edge \u{2192}");
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
