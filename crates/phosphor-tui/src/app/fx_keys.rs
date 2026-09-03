//! Keys → the effect chain, and the panels behind its slots.
//!
//! Every edit goes through [`App::set_fx_param`] or [`App::set_fx_bypass`],
//! which change the mirror and tell the audio thread in the same breath.
//! Nothing here writes a parameter any other way, for the reason the
//! sequencer's ops exist: an edit that changes what is on the screen and not
//! what is in the signal path is the hardest kind of bug to see, because the
//! screen agrees with you.
//!
//! # The grammar
//!
//! ```text
//! chain list   j/k slot · enter open · b bypass · [ ] reorder · d remove · a add
//! eq panel     the cursor moves the way the screen looks:
//!              wide   h/l band   j/k control
//!              narrow j/k band   h/l control
//!              enter holds the control · h/l adjusts · H/L strides · esc lets go
//! reverb panel j/k picks a knob · h/l adjusts · H/L strides
//!              enter holds it · esc lets go
//! delay panel  the same column of knobs as the reverb's
//! tape panel   and so is the tape's
//! ```
//!
//! The layout decides which way `h`/`l` point because the EQ's panel is a
//! grid and the arrow a hand reaches for is the one that moves the cursor in
//! the direction it is pointing. The reverb's panel is a *column* of knobs,
//! so there is nothing to the left of a control to move to and `h`/`l` adjust
//! straight away; `enter` still holds, because holding is what stops `j`/`k`
//! walking off the control being turned. Held, `h`/`l` always adjust — that
//! is the fader's contract and it does not move.

use super::*;

use phosphor_app::state::{FxType, FxView};
use phosphor_dsp::fx::eq::{
    iso_step_down, iso_step_up, natural_param, BandType, Slope, PARAM_COUNT,
};
use phosphor_dsp::fx::delay::{
    nearest_division, synced_seconds, uses as delay_uses, HEAD_SETS, SYNC_COUNT,
    natural_param as delay_param, Mode as DelayMode, Routing, TimeMode,
    PARAM_COUNT as DELAY_PARAMS, PARAM_DIVISION, PARAM_FREEZE, PARAM_HEADS,
    PARAM_HIGH_CUT_HZ as DELAY_HIGH_CUT, PARAM_LOW_CUT_HZ as DELAY_LOW_CUT,
    PARAM_MODE as DELAY_MODE, PARAM_ROUTING, PARAM_SYNC, PARAM_TIME_MODE, PARAM_TIME_MS,
};
use phosphor_dsp::fx::reverb::{
    natural_param as reverb_param, Algorithm, PARAM_ALGORITHM, PARAM_COUNT as REVERB_PARAMS,
    PARAM_DAMP_HZ, PARAM_DECAY_S, PARAM_EARLY, PARAM_LOW_CUT_HZ, PARAM_MOD_RATE_HZ,
    PARAM_PREDELAY_MS, PARAM_SIZE,
};

/// How far one press moves a control, and how far a shifted one does.
///
/// The gain's numbers are the EQ's published steps: half a decibel is the
/// finest move worth making by ear, and three is the one a mix decision is
/// made in.
const GAIN_FINE: f32 = 0.5;
const GAIN_COARSE: f32 = 3.0;

impl App {
    /// One key, in the chain list.
    pub(crate) fn handle_fx_chain_keys(&mut self, key: crossterm::event::KeyEvent) -> bool {
        // While the add-fx menu is up it takes the keys, exactly as it does
        // when opened from the track strip. Without this arm, Enter fell
        // through to "open the slot's panel" on a chain that had no slots,
        // and the chosen effect was never added — the menu over the chain
        // was a menu whose Enter went to the room behind it.
        if self.nav.fx_menu.open {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.nav.fx_menu.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.fx_menu.move_up(),
                KeyCode::Enter => self.fx_menu_choose(),
                KeyCode::Esc | KeyCode::Char('q') => self.nav.fx_menu.open = false,
                _ => {}
            }
            return true;
        }
        // One cursor walks the combined rack: MIDI slots first — they lead
        // in the signal — then the audio inserts. `rack_slot_at` says which
        // half a position names.
        use crate::state::RackSlot;
        let midi_len = self.nav.current_track().map_or(0, |t| t.midi_fx.len());
        let audio_len = self.nav.current_track().map_or(0, |t| t.fx_chain.len());
        let len = midi_len + audio_len;
        let cursor = self.nav.clip_view.fx_cursor.min(len.saturating_sub(1));
        let here = self.nav.rack_slot_at(cursor);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if len > 0 => {
                self.nav.clip_view.fx_cursor = (cursor + 1).min(len - 1);
            }
            KeyCode::Char('k') | KeyCode::Up if len > 0 => {
                self.nav.clip_view.fx_cursor = cursor.saturating_sub(1);
            }
            KeyCode::Enter if len > 0 => match here {
                Some(RackSlot::Midi(slot)) => self.open_midi_fx_panel(slot),
                Some(RackSlot::Audio(slot)) => self.open_fx_panel(slot),
                None => {}
            },
            // `m` and `b` are one switch: every other mutable thing in the
            // application answers to `m`, and a bypassed effect IS a muted
            // effect as far as the player's intent goes.
            KeyCode::Char('b') | KeyCode::Char('m') if len > 0 => match here {
                Some(RackSlot::Midi(slot)) => self.toggle_midi_fx_bypass(slot),
                Some(RackSlot::Audio(slot)) => self.toggle_fx_bypass(slot),
                None => {}
            },
            KeyCode::Char('[') if len > 1 => match here {
                Some(RackSlot::Audio(slot)) if slot > 0 => {
                    self.move_fx(slot, slot - 1);
                    self.nav.clip_view.fx_cursor = midi_len + slot - 1;
                }
                Some(RackSlot::Midi(_)) | Some(RackSlot::Audio(_)) => {
                    self.flash("midi fx lead the chain \u{2014} order is the signal's");
                }
                None => {}
            },
            KeyCode::Char(']') if len > 1 => match here {
                Some(RackSlot::Audio(slot)) if slot + 1 < audio_len => {
                    self.move_fx(slot, slot + 1);
                    self.nav.clip_view.fx_cursor = midi_len + slot + 1;
                }
                Some(RackSlot::Midi(_)) | Some(RackSlot::Audio(_)) => {
                    self.flash("midi fx lead the chain \u{2014} order is the signal's");
                }
                None => {}
            },
            KeyCode::Char('d') if len > 0 => match here {
                Some(RackSlot::Midi(slot)) => self.request_midi_fx_delete(slot),
                Some(RackSlot::Audio(slot)) => self.request_fx_delete(slot),
                None => {}
            },
            KeyCode::Char('a') => {
                self.nav.fx_menu.open = true;
                self.nav.fx_menu.cursor = 0;
            }
            KeyCode::Char('c') if midi_len > 0 => self.commit_midi_fx(),
            _ => return false,
        }
        true
    }

    /// Open a MIDI slot's panel in the wide pane.
    fn open_midi_fx_panel(&mut self, slot: usize) {
        if self.nav.current_track().and_then(|t| t.midi_fx.get(slot)).is_none() {
            return;
        }
        self.nav.clip_view.fx.open_midi(slot);
        self.nav.clip_view.clip_tab = ClipTab::Fx;
        self.nav.clip_view.focus = ClipViewFocus::PianoRoll;
        self.status_message = Some((
            "j/k picks a knob, h/l adjusts \u{00b7} plays live and on playback".into(),
            std::time::Instant::now(),
        ));
    }

    fn toggle_midi_fx_bypass(&mut self, slot: usize) {
        let index = self.nav.track_cursor;
        let Some(track) = self.nav.tracks.get(index) else { return };
        let Some(instance) = track.midi_fx.get(slot) else { return };
        let (bypass, label) = (!instance.bypass, instance.fx_type.label());
        self.set_midi_fx_bypass(index, slot, bypass);
        self.status_message = Some((
            format!("{label}: {}", if bypass { "bypassed" } else { "in the chain" }),
            std::time::Instant::now(),
        ));
    }

    /// The MIDI half of the delete flow: same modal, its own arm.
    fn request_midi_fx_delete(&mut self, slot: usize) {
        let Some(track) = self.nav.current_track() else { return };
        let Some(instance) = track.midi_fx.get(slot) else { return };
        let label = instance.fx_type.label();
        self.nav.clip_view.fx_cursor = slot;
        self.nav.confirm_modal.show(
            ConfirmKind::DeleteFx,
            &format!("remove {label} from the midi rack?"),
        );
    }

    /// Open a slot's panel in the wide pane, where there is room for it.
    fn open_fx_panel(&mut self, slot: usize) {
        let Some(fx_type) = self
            .nav
            .current_track()
            .and_then(|track| track.fx_chain.get(slot))
            .map(|instance| instance.fx_type)
        else {
            return;
        };
        self.nav.clip_view.fx.open(slot);
        self.nav.clip_view.clip_tab = ClipTab::Fx;
        self.nav.clip_view.focus = ClipViewFocus::PianoRoll;
        self.status_message = Some((
            match fx_type {
                FxType::Eq => {
                    "h/l picks a band, j/k a control, enter holds it, esc goes back".into()
                }
                FxType::Reverb | FxType::Delay => {
                    "j/k picks a knob, h/l adjusts, H/L strides, esc goes back".into()
                }
                FxType::Tape => {
                    "j/k picks a knob, h/l adjusts \u{00b7} speed moves the bump and the top"
                        .into()
                }
                FxType::Compressor => {
                    "j/k picks a knob, h/l adjusts \u{00b7} key and klistn are the last two rows"
                        .into()
                }
            },
            std::time::Instant::now(),
        ));
    }

    fn toggle_fx_bypass(&mut self, slot: usize) {
        let index = self.nav.track_cursor;
        let Some(track) = self.nav.tracks.get(index) else { return };
        let Some(instance) = track.fx_chain.get(slot) else { return };
        let (bypass, label) = (!instance.bypass, instance.fx_type.label());
        self.set_fx_bypass(index, slot, bypass);
        self.status_message = Some((
            format!("{label}: {}", if bypass { "bypassed" } else { "in the chain" }),
            std::time::Instant::now(),
        ));
    }

    /// Move a slot along the chain, on both sides.
    ///
    /// Order is the chain's meaning — an EQ before a compressor is a
    /// different sound from one after it — so this is an explicit move and
    /// never anything that could be mistaken for a sort.
    fn move_fx(&mut self, from: usize, to: usize) {
        let index = self.nav.track_cursor;
        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackFx { track_idx: index },
        );
        let Some(track) = self.nav.tracks.get_mut(index) else { return };
        if from >= track.fx_chain.len() || to >= track.fx_chain.len() {
            return;
        }
        let Some(target) = track.fx_target() else { return };
        let slot = track.fx_chain.remove(from);
        let label = slot.fx_type.label();
        track.fx_chain.insert(to, slot);
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::MoveFx { target, from, to });

        self.nav.clip_view.fx_cursor = to;
        // The open panel follows its slot rather than staying on a position
        // that now holds something else.
        if self.nav.clip_view.fx.slot == Some(from) {
            self.nav.clip_view.fx.slot = Some(to);
        }
        self.nav.commit_undo(undo_before, "move effect");
        self.status_message = Some((
            format!("{label} \u{2192} slot {}", to + 1),
            std::time::Instant::now(),
        ));
    }

    /// Ask before taking an effect out. `u` brings it back, settings and
    /// all, but the modal stays: a chain is work, and a `d` that lands one
    /// row off should have to say what it is about to take.
    fn request_fx_delete(&mut self, slot: usize) {
        let Some(track) = self.nav.current_track() else { return };
        let Some(instance) = track.fx_chain.get(slot) else { return };
        let label = instance.fx_type.label();
        let midi_len = track.midi_fx.len();
        self.nav.clip_view.fx_cursor = midi_len + slot;
        self.nav.confirm_modal.show(
            ConfirmKind::DeleteFx,
            &format!("remove {label} from slot {}?", slot + 1),
        );
    }

    /// Take the effect out of the slot under the cursor, on both sides.
    /// The cursor walks the combined rack, so this routes to whichever half
    /// it is standing in.
    pub(crate) fn remove_fx_at_cursor(&mut self) {
        let index = self.nav.track_cursor;
        let cursor = self.nav.clip_view.fx_cursor;
        match self.nav.rack_slot_at(cursor) {
            Some(crate::state::RackSlot::Midi(slot)) => {
                self.remove_midi_fx(index, slot);
                let len = self.nav.rack_len();
                self.nav.clip_view.fx_cursor = cursor.min(len.saturating_sub(1));
                if self.nav.clip_view.fx.midi_slot == Some(slot) {
                    self.nav.clip_view.fx.close();
                    self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                    self.nav.clip_view.focus = ClipViewFocus::FxPanel;
                }
                self.status_message =
                    Some(("midi effect removed (u to undo)".into(), std::time::Instant::now()));
                return;
            }
            Some(crate::state::RackSlot::Audio(slot)) => {
                // Fall through to the audio path below with the mapped index.
                self.remove_audio_fx_at(index, slot);
                return;
            }
            None => return,
        }
    }

    fn remove_audio_fx_at(&mut self, index: usize, slot: usize) {
        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackFx { track_idx: index },
        );
        let Some(track) = self.nav.tracks.get_mut(index) else { return };
        if slot >= track.fx_chain.len() {
            return;
        }
        let Some(target) = track.fx_target() else { return };
        let label = track.fx_chain.remove(slot).fx_type.label();
        let len = track.fx_chain.len();
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::RemoveFx { target, slot });

        let midi_len = self.nav.tracks.get(index).map_or(0, |t| t.midi_fx.len());
        self.nav.clip_view.fx_cursor = (midi_len + slot.min(len.saturating_sub(1)))
            .min(self.nav.rack_len().saturating_sub(1));
        // A panel open on the slot that just went is a panel showing an
        // effect that no longer exists.
        match self.nav.clip_view.fx.slot {
            Some(open) if open == slot => {
                self.nav.clip_view.fx.close();
                self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                self.nav.clip_view.focus = ClipViewFocus::FxPanel;
            }
            Some(open) if open > slot => self.nav.clip_view.fx.slot = Some(open - 1),
            _ => {}
        }
        self.nav.commit_undo(undo_before, "remove effect");
        self.status_message =
            Some((format!("{label} removed (u to undo)"), std::time::Instant::now()));
    }

    // ── The panel ──

    /// One key, in an effect's panel.
    pub(crate) fn handle_fx_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        if self.nav.clip_view.fx.midi_slot.is_some() {
            self.handle_midi_fx_panel_keys(key);
            return;
        }
        match self.open_fx_type() {
            Some(FxType::Reverb) => {
                self.handle_reverb_panel_keys(key);
                return;
            }
            Some(FxType::Delay) => {
                self.handle_delay_panel_keys(key);
                return;
            }
            Some(FxType::Compressor) => {
                self.handle_comp_panel_keys(key);
                return;
            }
            Some(FxType::Tape) => {
                self.handle_tape_panel_keys(key);
                return;
            }
            _ => {}
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if self.nav.clip_view.fx.locked {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                }
                KeyCode::Char('H') => self.adjust_fx_control(-1, true),
                KeyCode::Char('L') => self.adjust_fx_control(1, true),
                KeyCode::Char('h') | KeyCode::Left => self.adjust_fx_control(-1, shift),
                KeyCode::Char('l') | KeyCode::Right => self.adjust_fx_control(1, shift),
                _ => {}
            }
            return;
        }

        // The cursor moves the way the screen looks: bands are columns in the
        // wide layout and rows in the narrow one, and `h` always moves the
        // cursor the way `h` points.
        let wide = self.nav.clip_view.fx.wide;
        let controls = self.fx_control_count();
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                if wide {
                    self.nav.clip_view.fx.move_band(-1);
                } else {
                    self.nav.clip_view.fx.move_control(-1, controls);
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if wide {
                    self.nav.clip_view.fx.move_band(1);
                } else {
                    self.nav.clip_view.fx.move_control(1, controls);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if wide {
                    self.nav.clip_view.fx.move_control(1, controls);
                } else {
                    self.nav.clip_view.fx.move_band(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if wide {
                    self.nav.clip_view.fx.move_control(-1, controls);
                } else {
                    self.nav.clip_view.fx.move_band(-1);
                }
            }
            KeyCode::Char(digit @ '1'..='8') => {
                self.nav.clip_view.fx.band = digit as usize - '1' as usize;
            }
            KeyCode::Enter => {
                if self.fx_control_is_live() {
                    self.nav.clip_view.fx.locked = true;
                    self.status_message = Some((
                        "held: h/l adjusts, H/L strides, esc lets go".into(),
                        std::time::Instant::now(),
                    ));
                } else {
                    self.status_message = Some((
                        "this band type has no such control".into(),
                        std::time::Instant::now(),
                    ));
                }
            }
            KeyCode::Char('n') => self.toggle_fx_band(),
            KeyCode::Char('b') | KeyCode::Char('m') => {
                let slot = self.nav.clip_view.fx.slot.unwrap_or(0);
                self.toggle_fx_bypass(slot);
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.nav.clip_view.fx.close();
                self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                self.nav.clip_view.focus = ClipViewFocus::FxPanel;
            }
            _ => {}
        }
    }

    /// The type of the effect whose panel is open, if one is.
    pub(crate) fn open_fx_type(&self) -> Option<FxType> {
        self.nav.open_fx_type()
    }

    /// How many controls the band under the cursor has. The trim has one.
    fn fx_control_count(&self) -> usize {
        if self.nav.clip_view.fx.band >= FxView::TRIM {
            1
        } else {
            FxView::CONTROLS
        }
    }

    /// The slot's parameters, if a panel is open on one.
    fn fx_params(&self) -> Option<&[f32]> {
        let slot = self.nav.clip_view.fx.slot?;
        let track = self.nav.current_track()?;
        Some(track.fx_chain.get(slot)?.params.as_slice())
    }

    /// Whether the control under the cursor does anything on this band type.
    fn fx_control_is_live(&self) -> bool {
        let view = &self.nav.clip_view.fx;
        if view.band >= FxView::TRIM {
            return true;
        }
        let Some(params) = self.fx_params() else { return false };
        let ty = BandType::from_index(
            params.get(view.band * FxView::CONTROLS).copied().unwrap_or(0.0) as usize,
        );
        match view.control {
            2 => ty.uses_gain(),
            3 => ty.uses_q(),
            4 => ty.uses_slope(),
            _ => true,
        }
    }

    /// The band's on switch, from the panel — the same control the strip's
    /// `on` row shows, on a key rather than through the lock.
    fn toggle_fx_band(&mut self) {
        let view_band = self.nav.clip_view.fx.band;
        if view_band >= FxView::TRIM {
            return;
        }
        let Some(params) = self.fx_params() else { return };
        let index = view_band * FxView::CONTROLS + 5;
        let next = if params.get(index).copied().unwrap_or(0.0) >= 0.5 { 0.0 } else { 1.0 };
        let (track, slot) = (self.nav.track_cursor, self.nav.clip_view.fx.slot.unwrap_or(0));
        self.set_fx_param(track, slot, index, next);
    }

    /// Turn the control under the cursor.
    ///
    /// Every control moves in its own unit and by its own step: frequencies
    /// walk the ISO sixth-octave centres so the readout is always a number an
    /// EQ says out loud, gains move in half a decibel, and the two counted
    /// controls step through their own lists.
    fn adjust_fx_control(&mut self, delta: i32, coarse: bool) {
        if !self.fx_control_is_live() {
            return;
        }
        let view_band = self.nav.clip_view.fx.band;
        let control = self.nav.clip_view.fx.control;
        let Some(params) = self.fx_params() else { return };
        let index = if view_band >= FxView::TRIM {
            PARAM_COUNT - 1
        } else {
            view_band * FxView::CONTROLS + control
        };
        let current = params.get(index).copied().unwrap_or(0.0);
        let ty = BandType::from_index(
            params
                .get(view_band.min(FxView::TRIM - 1) * FxView::CONTROLS)
                .copied()
                .unwrap_or(0.0) as usize,
        );

        let next = if view_band >= FxView::TRIM {
            step_clamped(index, current, delta as f32 * if coarse { GAIN_COARSE } else { GAIN_FINE })
        } else {
            match control {
                // The type, through its own list. Everything else on the band
                // stays where it is: a bell turned into a high-pass and back
                // is the bell it was.
                0 => {
                    let at = (current as i32 + delta).clamp(0, BandType::ALL.len() as i32 - 1);
                    at as f32
                }
                1 => {
                    let hz = f64::from(current);
                    let mut next = hz;
                    // A coarse press is six of them, which is an octave on a
                    // sixth-octave grid.
                    for _ in 0..if coarse { 6 } else { 1 } {
                        next = if delta > 0 { iso_step_up(next) } else { iso_step_down(next) };
                    }
                    next as f32
                }
                2 => step_clamped(
                    index,
                    current,
                    delta as f32 * if coarse { GAIN_COARSE } else { GAIN_FINE },
                ),
                3 => {
                    // Q multiplicatively: the useful travel is 0.1 to 40 and
                    // a fixed step is either useless at the bottom or a jump
                    // at the top.
                    let factor = if coarse { 1.5f32 } else { 1.12 };
                    let next = if delta > 0 { current * factor } else { current / factor };
                    step_to(index, next)
                }
                4 => {
                    let choices = Slope::choices_for(ty);
                    if choices.is_empty() {
                        current
                    } else {
                        let at = choices
                            .iter()
                            .position(|s| f32::from(s.db_per_octave()) == current)
                            .unwrap_or(0);
                        let next = (at as i32 + delta).clamp(0, choices.len() as i32 - 1) as usize;
                        f32::from(choices[next].db_per_octave())
                    }
                }
                _ => {
                    // The on switch, turned rather than toggled: right is on.
                    f32::from(delta > 0)
                }
            }
        };

        let (track, slot) = (self.nav.track_cursor, self.nav.clip_view.fx.slot.unwrap_or(0));
        self.set_fx_param(track, slot, index, next);
    }
}

// ── The reverb's panel ──

impl App {
    /// One key, in the reverb's panel.
    ///
    /// A column of twelve knobs rather than a grid, so `j`/`k` pick and
    /// `h`/`l` turn. `enter` still holds, and holding is not decoration: it
    /// is what stops `j`/`k` walking off the control a hand is in the middle
    /// of turning.
    fn handle_reverb_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('H') => self.adjust_reverb_control(-1, true),
            KeyCode::Char('L') => self.adjust_reverb_control(1, true),
            KeyCode::Char('h') | KeyCode::Left => self.adjust_reverb_control(-1, shift),
            KeyCode::Char('l') | KeyCode::Right => self.adjust_reverb_control(1, shift),
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav.clip_view.fx.move_cursor(1, REVERB_PARAMS);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav.clip_view.fx.move_cursor(-1, REVERB_PARAMS);
            }
            KeyCode::Enter => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.locked = true;
                    self.status_message = Some((
                        "held: h/l adjusts, H/L strides, esc lets go".into(),
                        std::time::Instant::now(),
                    ));
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.close();
                    self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                    self.nav.clip_view.focus = ClipViewFocus::FxPanel;
                }
            }
            KeyCode::Char('b') | KeyCode::Char('m') => {
                let slot = self.nav.clip_view.fx.slot.unwrap_or(0);
                self.toggle_fx_bypass(slot);
            }
            _ => {}
        }
    }

    /// The MIDI-effect panel: a knob list, the reverb panel's manners.
    fn handle_midi_fx_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let count = self
            .nav
            .clip_view
            .fx
            .midi_slot
            .and_then(|slot| self.nav.current_track()?.midi_fx.get(slot).map(|i| i.fx_type))
            .map_or(0, |t| t.params().len());
        match key.code {
            KeyCode::Char('H') => self.adjust_midi_fx_control(-1, true),
            KeyCode::Char('L') => self.adjust_midi_fx_control(1, true),
            KeyCode::Char('h') | KeyCode::Left => self.adjust_midi_fx_control(-1, shift),
            KeyCode::Char('l') | KeyCode::Right => self.adjust_midi_fx_control(1, shift),
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav.clip_view.fx.move_cursor(1, count);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav.clip_view.fx.move_cursor(-1, count);
            }
            KeyCode::Enter => {
                self.nav.clip_view.fx.locked = !self.nav.clip_view.fx.locked;
                self.status_message = if self.nav.clip_view.fx.locked {
                    Some(("held: h/l adjusts, H/L strides, esc lets go".into(), std::time::Instant::now()))
                } else {
                    None
                };
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.close();
                    self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                    self.nav.clip_view.focus = ClipViewFocus::FxPanel;
                }
            }
            KeyCode::Char('b') | KeyCode::Char('m') => {
                let slot = self.nav.clip_view.fx.midi_slot.unwrap_or(0);
                self.toggle_midi_fx_bypass(slot);
            }
            KeyCode::Char(c @ '1'..='4') => {
                let slot = self.nav.clip_view.fx.midi_slot.unwrap_or(0);
                self.apply_arp_preset(slot, c as usize - '1' as usize);
            }
            KeyCode::Char('e') => {
                let slot = self.nav.clip_view.fx.midi_slot.unwrap_or(0);
                let is_chord = self
                    .nav
                    .current_track()
                    .and_then(|t| t.midi_fx.get(slot))
                    .is_some_and(|i| i.fx_type == crate::state::MidiFxType::Chord);
                if is_chord {
                    self.open_prog_editor(slot);
                }
            }
            _ => {}
        }
    }

    /// One of the arp's factory feels, as one undo step.
    /// `pub(crate)` with a test alias below, so the battery can press the
    /// number keys' handler directly.
    pub(crate) fn apply_arp_preset(&mut self, slot: usize, index: usize) {
        use crate::state::{MidiFxType, ARP_PRESETS};
        let track_idx = self.nav.track_cursor;
        let is_arp = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.midi_fx.get(slot))
            .is_some_and(|i| i.fx_type == MidiFxType::Arp);
        if !is_arp {
            return;
        }
        let Some(&(name, settings)) = ARP_PRESETS.get(index) else { return };
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMidiFx { track_idx },
        );
        for (param, value) in settings {
            self.write_midi_fx_param(track_idx, slot, param, value);
        }
        self.nav.commit_undo(before, "arp preset");
        self.flash(format!("arp: {name}"));
    }

    /// Turn the MIDI-effect control under the cursor. Selectors — style,
    /// rate, octaves, latch — step by one and stop at the ends; the
    /// percentages stride under shift.
    fn adjust_midi_fx_control(&mut self, direction: i32, stride: bool) {
        let track_idx = self.nav.track_cursor;
        let Some(slot) = self.nav.clip_view.fx.midi_slot else { return };
        let row = self.nav.clip_view.fx.band;
        let Some(instance) = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.midi_fx.get(slot))
        else {
            return;
        };
        let fx_type = instance.fx_type;
        let Some(info) = fx_type.params().get(row) else { return };
        let current = instance.params.get(row).copied().unwrap_or(info.default);
        // Ranges up to ten wide are selectors; the rest are values.
        let step = if info.max - info.min <= 10.0 {
            1.0
        } else if stride {
            10.0
        } else {
            1.0
        };
        let value = (current + step * direction as f32).clamp(info.min, info.max);
        self.set_midi_fx_param(track_idx, slot, row, value);
        let shown = fx_type.value_text(row, value);
        self.flash(format!("{}: {shown}", info.name));
    }

    /// Turn the reverb control under the cursor.
    ///
    /// Every control moves in its own unit and by its own law: times move
    /// geometrically because the ear does, frequencies walk the ISO
    /// sixth-octave centres so the readout is a number a manual would print,
    /// `size` moves in the 5% steps the geometry crossfade is quantised to,
    /// and percentages move in whole points.
    fn adjust_reverb_control(&mut self, delta: i32, coarse: bool) {
        let control = self.nav.clip_view.fx.band.min(REVERB_PARAMS - 1);
        let Some(params) = self.fx_params() else { return };
        let algorithm = crate::ui::fx::reverb_algorithm(params);
        if !algorithm.uses(control) {
            self.status_message = Some((
                format!("{} has no {} control", algorithm.label(), reverb_param(control).map_or("", |p| p.name)),
                std::time::Instant::now(),
            ));
            return;
        }
        let current = params.get(control).copied().unwrap_or(0.0);
        let step = f64::from(delta);

        let next = match control {
            PARAM_ALGORITHM => {
                let at = (current.round() as i32 + delta)
                    .clamp(0, Algorithm::ALL.len() as i32 - 1);
                at as f32
            }
            PARAM_PREDELAY_MS => current + delta as f32 * if coarse { 10.0 } else { 1.0 },
            // Times geometrically: a tenth of a second matters at 0.5 s and
            // is invisible at 12.
            PARAM_DECAY_S | PARAM_MOD_RATE_HZ => {
                let factor = if coarse { 1.5f64 } else { 1.08 };
                (f64::from(current) * factor.powf(step)) as f32
            }
            // The morph's own quantum, so one press is exactly one crossfade
            // rather than a fraction of one that does nothing.
            PARAM_SIZE => current + delta as f32 * if coarse { 0.25 } else { 0.05 },
            PARAM_DAMP_HZ | PARAM_LOW_CUT_HZ => {
                let mut hz = f64::from(current);
                // A coarse press is six of them, which is an octave on a
                // sixth-octave grid.
                for _ in 0..if coarse { 6 } else { 1 } {
                    hz = if delta > 0 { iso_step_up(hz) } else { iso_step_down(hz) };
                }
                hz as f32
            }
            _ => current + delta as f32 * if coarse { 10.0 } else { 1.0 },
        };
        let next = match reverb_param(control) {
            Some(info) => next.clamp(info.min, info.max),
            None => next,
        };

        let (track, slot) = (self.nav.track_cursor, self.nav.clip_view.fx.slot.unwrap_or(0));
        self.set_fx_param(track, slot, control, next);
        if control == PARAM_ALGORITHM {
            self.follow_algorithm_with_early(track, slot, algorithm, next);
        }
    }

    /// Move the early-reflection level to what the incoming algorithm wants —
    /// but only if the player has not set it themselves.
    ///
    /// A bare eight-line hall emits nothing at all for its first 125 ms,
    /// because that is its shortest delay line; a plate's whole identity is
    /// having no early reflections at all. One control cannot default to both,
    /// and a control whose *value* changed behind the algorithm selector
    /// would be a control that lies about what it is set to. So this moves it
    /// visibly, on screen, in the same keystroke, and only from the outgoing
    /// algorithm's own suggestion — the moment a player touches `early`, the
    /// algorithm knob stops touching it back.
    fn follow_algorithm_with_early(
        &mut self,
        track: usize,
        slot: usize,
        was: Algorithm,
        now: f32,
    ) {
        let wanted = Algorithm::from_index(now.round().max(0.0) as usize);
        if wanted == was {
            return;
        }
        let Some(params) = self.fx_params() else { return };
        let early = params.get(PARAM_EARLY).copied().unwrap_or(0.0);
        if (early - was.suggested_early()).abs() > 0.5 {
            return;
        }
        self.set_fx_param(track, slot, PARAM_EARLY, wanted.suggested_early());
    }
}

/// A control moved by `delta`, kept inside the EQ's own published travel.
fn step_clamped(index: usize, current: f32, delta: f32) -> f32 {
    step_to(index, current + delta)
}

/// A control set to `value`, kept inside the EQ's own published travel.
///
/// The limits are the effect's answer rather than a copy of them here, so a
/// control whose range moves cannot leave a stale clamp behind in the UI.
fn step_to(index: usize, value: f32) -> f32 {
    match natural_param(index) {
        Some(info) => value.clamp(info.min, info.max),
        None => value,
    }
}

impl App {
    /// One press of the pan or of a send, and what it now reads.
    ///
    /// Both go through the strip's own arithmetic and then through
    /// [`App::sync_routing`], which is the only thing that tells the audio
    /// thread — a pan that moved on the screen and not in the mix is the
    /// defect this shape exists to make impossible.
    pub(crate) fn step_routing(&mut self, steps: i32) {
        use phosphor_core::fx::SendSlot;
        let index = self.nav.track_cursor;
        let element = self.nav.track_element;
        let undo_before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMix { track_idx: index },
        );
        let Some(track) = self.nav.tracks.get_mut(index) else { return };

        let text = match element {
            crate::state::TrackElement::Pan => {
                let pan = track.adjust_pan(steps);
                format!("pan: {}", pan_label(pan))
            }
            crate::state::TrackElement::SendA | crate::state::TrackElement::SendB => {
                let slot = if element == crate::state::TrackElement::SendA {
                    SendSlot::A
                } else {
                    SendSlot::B
                };
                // A send opens from silence: the first press up takes it to
                // the bottom of the useful travel rather than to −inf + 1 dB,
                // which is a press that does nothing audible.
                let current = track.send_db(slot);
                let next = match (current, steps > 0) {
                    (None, true) => SEND_FLOOR_DB,
                    (None, false) => phosphor_core::fx::SILENT_DB,
                    (Some(db), _) => db + steps as f32,
                };
                let db = track.set_send_db(slot, next);
                let name = if element == crate::state::TrackElement::SendA { "A" } else { "B" };
                if db <= 0.0 {
                    format!("send {name}: off")
                } else {
                    format!("send {name}: {:+.0} dB", track.send_db(slot).unwrap_or(0.0))
                }
            }
            _ => return,
        };

        self.nav.commit_undo_coalesced(
            undo_before,
            "routing",
            crate::state::undo::UndoGesture::Routing { track_idx: index },
        );
        self.sync_routing(index);
        self.status_message = Some((text, std::time::Instant::now()));
    }
}

/// Where a send opens to when it is turned up from silence.
const SEND_FLOOR_DB: f32 = -40.0;

/// The pan, as a mixer says it: `C`, `L37`, `R50`.
pub(crate) fn pan_label(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round() as i32;
    if amount == 0 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("L{amount}")
    } else {
        format!("R{amount}")
    }
}

// ── The delay's panel ──

/// How far one press moves the free-running time, and how far a shifted one
/// does — as a *ratio*, so the step is a musically equal one at both ends of a
/// range that spans five thousand to one.
const TIME_FINE: f32 = 1.015;
const TIME_COARSE: f32 = 1.15;

impl App {
    /// One key, in the delay's panel.
    ///
    /// The same grammar as the reverb's, because it is the same shape of
    /// panel: a column of knobs, `j`/`k` to pick and `h`/`l` to turn, `enter`
    /// to hold so that `j`/`k` stop walking off the control a hand is in the
    /// middle of turning.
    fn handle_delay_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('H') => self.adjust_delay_control(-1, true),
            KeyCode::Char('L') => self.adjust_delay_control(1, true),
            KeyCode::Char('h') | KeyCode::Left => self.adjust_delay_control(-1, shift),
            KeyCode::Char('l') | KeyCode::Right => self.adjust_delay_control(1, shift),
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav.clip_view.fx.move_cursor(1, DELAY_PARAMS);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav.clip_view.fx.move_cursor(-1, DELAY_PARAMS);
            }
            KeyCode::Enter => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.locked = true;
                    self.status_message = Some((
                        "held: h/l adjusts, H/L strides, esc lets go".into(),
                        std::time::Instant::now(),
                    ));
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.close();
                    self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                    self.nav.clip_view.focus = ClipViewFocus::FxPanel;
                }
            }
            KeyCode::Char('b') | KeyCode::Char('m') => {
                let slot = self.nav.clip_view.fx.slot.unwrap_or(0);
                self.toggle_fx_bypass(slot);
            }
            _ => {}
        }
    }

    /// Turn the delay control under the cursor.
    ///
    /// Every control moves in its own unit and by its own law: the counted
    /// ones step through their own lists, the free time moves by a *ratio* so
    /// that a press is worth the same musically at 2 ms as at 2 s, frequencies
    /// walk the ISO sixth-octave centres, and percentages move in whole
    /// points.
    fn adjust_delay_control(&mut self, delta: i32, coarse: bool) {
        let control = self.nav.clip_view.fx.band.min(DELAY_PARAMS - 1);
        let Some(params) = self.fx_params() else { return };
        if !delay_uses(params, control) {
            let why = crate::ui::fx::delay_why_not(params, control);
            self.status_message = Some((why, std::time::Instant::now()));
            return;
        }
        let params = params.to_vec();
        let current = params.get(control).copied().unwrap_or(0.0);

        let next = match control {
            DELAY_MODE => step_list(current, delta, DelayMode::ALL.len()),
            PARAM_ROUTING => step_list(current, delta, Routing::ALL.len()),
            PARAM_TIME_MODE => step_list(current, delta, TimeMode::ALL.len()),
            PARAM_HEADS => step_list(current, delta, HEAD_SETS.len()),
            PARAM_DIVISION => step_list(current, delta, SYNC_COUNT),
            // The two switches turn rather than toggle: right is on, which is
            // the same thing `h`/`l` does to every other control on the panel.
            PARAM_SYNC | PARAM_FREEZE => f32::from(delta > 0),
            PARAM_TIME_MS => {
                let factor = if coarse { TIME_COARSE } else { TIME_FINE };
                if delta > 0 { current * factor } else { current / factor }
            }
            DELAY_LOW_CUT | DELAY_HIGH_CUT => {
                let mut hz = f64::from(current);
                // A coarse press is six of them, which is an octave on a
                // sixth-octave grid.
                for _ in 0..if coarse { 6 } else { 1 } {
                    hz = if delta > 0 { iso_step_up(hz) } else { iso_step_down(hz) };
                }
                hz as f32
            }
            _ => current + delta as f32 * if coarse { 10.0 } else { 1.0 },
        };
        let next = match delay_param(control) {
            Some(info) => next.clamp(info.min, info.max),
            None => next,
        };

        let (track, slot) = (self.nav.track_cursor, self.nav.clip_view.fx.slot.unwrap_or(0));
        self.set_fx_param(track, slot, control, next);
        if control == PARAM_SYNC {
            self.carry_the_delay_time_over(track, slot, &params, next >= 0.5);
        }
    }

    /// **Switching the clock carries the current time over.**
    ///
    /// Most plugins keep two hidden values and jump between them, which is a
    /// mouse affordance — you can see both at once. In a terminal you cannot,
    /// so the switch writes whichever half was not being used to match the one
    /// that was. It also enables the workflow the switch is actually for:
    /// dial the delay in by ear, then lock it to the grid.
    fn carry_the_delay_time_over(
        &mut self,
        track: usize,
        slot: usize,
        before: &[f32],
        now_synced: bool,
    ) {
        let bpm = f64::from(self.nav.tempo_bpm);
        if now_synced {
            // Free-running to synced: land on the division nearest the time
            // that was dialled in.
            let seconds = f64::from(before.get(PARAM_TIME_MS).copied().unwrap_or(0.0)) / 1000.0;
            let division = nearest_division(seconds, bpm);
            self.set_fx_param(track, slot, PARAM_DIVISION, division as f32);
            let (landed, _) = synced_seconds(division, bpm);
            self.status_message = Some((
                format!(
                    "synced: {:.0} ms \u{2192} {} ({:.0} ms)",
                    seconds * 1000.0,
                    phosphor_dsp::fx::delay::SYNC_LABELS[division],
                    landed * 1000.0
                ),
                std::time::Instant::now(),
            ));
        } else {
            // Synced to free-running: keep the milliseconds it was at.
            let division =
                before.get(PARAM_DIVISION).copied().unwrap_or(0.0).round().max(0.0) as usize;
            let (seconds, _) = synced_seconds(division, bpm);
            let ms = (seconds * 1000.0) as f32;
            self.set_fx_param(track, slot, PARAM_TIME_MS, ms);
            self.status_message = Some((
                format!("free: {ms:.0} ms, carried over from {}", phosphor_dsp::fx::delay::SYNC_LABELS[division]),
                std::time::Instant::now(),
            ));
        }
    }
}

/// A counted control moved one place along its own list, stopping at both
/// ends.
fn step_list(current: f32, delta: i32, len: usize) -> f32 {
    (current.round() as i32 + delta).clamp(0, len as i32 - 1) as f32
}

// ── The tape's panel ──

use phosphor_dsp::fx::tape::{
    auto_makeup_db as tape_auto_makeup_db, natural_param as tape_param, uses as tape_uses, Speed,
    PARAM_AUTO_MAKEUP as TAPE_AUTO_MAKEUP, PARAM_AZIMUTH_DEG as TAPE_AZIMUTH,
    PARAM_BUMP_DB as TAPE_BUMP_DB, PARAM_COUNT as TAPE_PARAMS, PARAM_SPEED as TAPE_SPEED,
    PARAM_TRIM_DB as TAPE_TRIM,
};

/// How far one press moves the head bump and the azimuth, and how far a
/// shifted one does. Both are small ranges where the whole travel matters, so
/// they step in tenths rather than in percentage points.
const BUMP_FINE: f32 = 0.1;
const BUMP_COARSE: f32 = 0.5;
const AZIMUTH_FINE: f32 = 0.05;
const AZIMUTH_COARSE: f32 = 0.25;

impl App {
    /// One key, in the tape's panel.
    ///
    /// The same grammar as the reverb's, the delay's and the compressor's,
    /// because it is the same shape of panel: a column of knobs, `j`/`k` to
    /// pick and `h`/`l` to turn, `enter` to hold so that `j`/`k` stop walking
    /// off the control a hand is in the middle of turning.
    fn handle_tape_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('H') => self.adjust_tape_control(-1, true),
            KeyCode::Char('L') => self.adjust_tape_control(1, true),
            KeyCode::Char('h') | KeyCode::Left => self.adjust_tape_control(-1, shift),
            KeyCode::Char('l') | KeyCode::Right => self.adjust_tape_control(1, shift),
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav.clip_view.fx.move_cursor(1, TAPE_PARAMS);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav.clip_view.fx.move_cursor(-1, TAPE_PARAMS);
            }
            KeyCode::Enter => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.locked = true;
                    self.status_message = Some((
                        "held: h/l adjusts, H/L strides, esc lets go".into(),
                        std::time::Instant::now(),
                    ));
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.close();
                    self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                    self.nav.clip_view.focus = ClipViewFocus::FxPanel;
                }
            }
            KeyCode::Char('b') | KeyCode::Char('m') => {
                let slot = self.nav.clip_view.fx.slot.unwrap_or(0);
                self.toggle_fx_bypass(slot);
            }
            _ => {}
        }
    }

    /// Turn the tape control under the cursor.
    ///
    /// Every control moves in its own unit and by its own law: the speed
    /// steps through its three positions, the head bump and the azimuth move
    /// in tenths because their whole travel is three decibels and one degree,
    /// the trim moves in the EQ's own half-decibel steps, and the percentages
    /// move in whole points.
    fn adjust_tape_control(&mut self, delta: i32, coarse: bool) {
        let control = self.nav.clip_view.fx.band.min(TAPE_PARAMS - 1);
        let (track, slot) = (self.nav.track_cursor, self.nav.clip_view.fx.slot.unwrap_or(0));
        let Some(params) = self.fx_params().map(<[f32]>::to_vec) else { return };
        let current = params.get(control).copied().unwrap_or(0.0);

        // **The automatic hands the trim back rather than refusing the key**,
        // and it hands it back *where it had it*, so taking control never
        // moves the level. The compressor's makeup does exactly this, and it
        // is exactly this control.
        if control == TAPE_TRIM && !tape_uses(&params, control) {
            let seeded = tape_auto_makeup_db(&params) as f32;
            self.set_fx_param(track, slot, TAPE_AUTO_MAKEUP, 0.0);
            self.set_fx_param(track, slot, TAPE_TRIM, seeded);
            self.status_message = Some((
                format!("output is yours: {seeded:+.1} dB, where the automatic had it"),
                std::time::Instant::now(),
            ));
            return;
        }

        let next = match control {
            TAPE_SPEED => step_list(current, delta, Speed::ALL.len()),
            // The switch turns rather than toggles: right is on, which is the
            // same thing `h`/`l` does to every other control on the panel.
            TAPE_AUTO_MAKEUP => f32::from(delta > 0),
            TAPE_BUMP_DB => {
                current + delta as f32 * if coarse { BUMP_COARSE } else { BUMP_FINE }
            }
            TAPE_AZIMUTH => {
                current + delta as f32 * if coarse { AZIMUTH_COARSE } else { AZIMUTH_FINE }
            }
            TAPE_TRIM => current + delta as f32 * if coarse { GAIN_COARSE } else { GAIN_FINE },
            _ => current + delta as f32 * if coarse { 10.0 } else { 1.0 },
        };
        let next = match tape_param(control) {
            Some(info) => next.clamp(info.min, info.max),
            None => next,
        };
        self.set_fx_param(track, slot, control, next);

        // Switching the makeup back to automatic says what it decided, for
        // the same reason the manual seeding does: a gain that changes
        // without a number is a gain a player cannot check.
        if control == TAPE_AUTO_MAKEUP && next >= 0.5 {
            let mut after = params;
            after[TAPE_AUTO_MAKEUP] = 1.0;
            self.status_message = Some((
                format!("output is automatic again: {:+.1} dB", tape_auto_makeup_db(&after)),
                std::time::Instant::now(),
            ));
        }
    }
}

// ── The compressor's panel ──

use phosphor_dsp::fx::compressor::{
    auto_makeup_for, auto_release_of, character_params, ratio_to_percent,
    natural_param as comp_param, AutoRelease, CHARACTER_COUNT, PARAM_ATTACK_MS,
    PARAM_AUTO_MAKEUP, PARAM_AUTO_RELEASE, PARAM_CHARACTER, PARAM_KNEE_DB, PARAM_MAKEUP_DB, PARAM_MIX as COMP_MIX, PARAM_RATIO, PARAM_RELEASE_MS,
    PARAM_SC_HPF_HZ, PARAM_SENSE, PARAM_THRESHOLD_DB, RATIO_STOPS, SC_HPF_MAX_HZ,
    SC_HPF_MIN_HZ,
};

use crate::ui::fx::{COMP_ROWS, COMP_ROW_KEY, COMP_ROW_KEY_LISTEN};

impl App {
    /// One key, in the compressor's panel.
    ///
    /// The same grammar as the reverb's and the delay's, because it is the
    /// same shape of panel: a column of knobs, `j`/`k` to pick and `h`/`l` to
    /// turn, `enter` to hold so that `j`/`k` stop walking off the control a
    /// hand is in the middle of turning.
    fn handle_comp_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('H') => self.adjust_comp_control(-1, true),
            KeyCode::Char('L') => self.adjust_comp_control(1, true),
            KeyCode::Char('h') | KeyCode::Left => self.adjust_comp_control(-1, shift),
            KeyCode::Char('l') | KeyCode::Right => self.adjust_comp_control(1, shift),
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav.clip_view.fx.move_cursor(1, COMP_ROWS);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav.clip_view.fx.move_cursor(-1, COMP_ROWS);
            }
            KeyCode::Enter => {
                // On the key-listen row, `enter` is the switch rather than the
                // hold: holding a two-position control so that `h`/`l` can
                // turn it is a keystroke spent on nothing.
                if self.nav.clip_view.fx.band.min(COMP_ROWS - 1) == COMP_ROW_KEY_LISTEN {
                    self.toggle_key_listen();
                } else if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.locked = true;
                    self.status_message = Some((
                        "held: h/l adjusts, H/L strides, esc lets go".into(),
                        std::time::Instant::now(),
                    ));
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if self.nav.clip_view.fx.locked {
                    self.nav.clip_view.fx.locked = false;
                    self.status_message = None;
                } else {
                    self.nav.clip_view.fx.close();
                    self.nav.clip_view.clip_tab = ClipTab::InstConfig;
                    self.nav.clip_view.focus = ClipViewFocus::FxPanel;
                    // Leaving the panel puts the monitor back. See
                    // `App::enforce_key_listen` for the frame-by-frame version
                    // of the same rule.
                    self.set_key_listen(None);
                }
            }
            KeyCode::Char('b') | KeyCode::Char('m') => {
                let slot = self.nav.clip_view.fx.slot.unwrap_or(0);
                self.toggle_fx_bypass(slot);
            }
            _ => {}
        }
    }

    /// Turn the compressor control under the cursor.
    ///
    /// Every control moves in its own unit and by its own law: decibels in
    /// decibels, times geometrically because the ear hears them that way, the
    /// detector filter along the ISO sixth-octave centres, and the ratio in
    /// *slope* — one point of dB-per-dB at a time, or from one printed ratio
    /// to the next when the press is a stride.
    ///
    /// **Turning a greyed control takes it back.** The makeup and the release
    /// grey out when an automatic owns them; reaching for either one switches
    /// the automatic off and seeds the knob with the value it was already
    /// producing, so the control never jumps and nothing on this panel is ever
    /// simply dead.
    fn adjust_comp_control(&mut self, delta: i32, coarse: bool) {
        let control = self.nav.clip_view.fx.band.min(COMP_ROWS - 1);
        let (track, slot) = (self.nav.track_cursor, self.nav.clip_view.fx.slot.unwrap_or(0));

        if control == COMP_ROW_KEY {
            self.step_comp_key(delta);
            return;
        }
        if control == COMP_ROW_KEY_LISTEN {
            self.set_key_listen_here(delta > 0);
            return;
        }

        let Some(params) = self.fx_params().map(<[f32]>::to_vec) else { return };
        let at = |index: usize| params.get(index).copied().unwrap_or(0.0);

        // The two automatics hand their control back rather than refusing it.
        if control == PARAM_MAKEUP_DB && at(PARAM_AUTO_MAKEUP) >= 0.5 {
            let seeded = auto_makeup_for(
                f64::from(at(PARAM_THRESHOLD_DB)),
                f64::from(at(PARAM_RATIO)) / 100.0,
            ) as f32;
            self.set_fx_param(track, slot, PARAM_AUTO_MAKEUP, 0.0);
            self.set_fx_param(track, slot, PARAM_MAKEUP_DB, seeded);
            self.status_message = Some((
                format!("makeup is yours: {seeded:+.1} dB, where the automatic had it"),
                std::time::Instant::now(),
            ));
            return;
        }
        if control == PARAM_RELEASE_MS && auto_release_of(&params) != AutoRelease::Off {
            self.set_fx_param(track, slot, PARAM_AUTO_RELEASE, 0.0);
            self.status_message = Some((
                format!("release is yours: {:.0} ms", at(PARAM_RELEASE_MS)),
                std::time::Instant::now(),
            ));
            return;
        }

        let current = at(control);
        let next = match control {
            PARAM_CHARACTER => step_list(current, delta, CHARACTER_COUNT),
            PARAM_THRESHOLD_DB | PARAM_KNEE_DB => {
                current + delta as f32 * if coarse { 6.0 } else { 1.0 }
            }
            // Linear in slope, which is linear in effect. A stride jumps to
            // the next ratio a manual would print.
            PARAM_RATIO => {
                if coarse {
                    next_ratio_stop(current, delta)
                } else {
                    current + delta as f32
                }
            }
            // Times geometrically: a tenth of a millisecond matters at 1 ms
            // and is invisible at 100.
            PARAM_ATTACK_MS | PARAM_RELEASE_MS => {
                let factor = if coarse { 2.0f32 } else { 1.15 };
                if delta > 0 { current * factor } else { current / factor }
            }
            PARAM_AUTO_RELEASE => step_list(current, delta, AutoRelease::ALL.len()),
            PARAM_MAKEUP_DB => current + delta as f32 * if coarse { GAIN_COARSE } else { GAIN_FINE },
            COMP_MIX => current + delta as f32 * if coarse { 10.0 } else { 1.0 },
            // The two switches turn rather than toggle: right is on, which is
            // what `h`/`l` does to every other control on the panel.
            PARAM_AUTO_MAKEUP | PARAM_SENSE => f32::from(delta > 0),
            PARAM_SC_HPF_HZ => step_sc_hpf(current, delta, coarse),
            _ => current,
        };
        let next = match comp_param(control) {
            Some(info) => next.clamp(info.min, info.max),
            None => next,
        };
        self.set_fx_param(track, slot, control, next);

        // Recalling a character writes all twelve, because a macro that only
        // moved its own selector would be a selector that lies about what is
        // in force. It is done here rather than inside the effect so that one
        // `set_parameter` is always one control — which is what keeps a
        // session load from depending on the order its controls are written.
        if control == PARAM_CHARACTER && next != current {
            self.recall_comp_character(track, slot, next);
        }
    }

    /// Write a character's whole parameter set, on both sides.
    fn recall_comp_character(&mut self, track: usize, slot: usize, index: f32) {
        let wanted = character_params(index.round().max(0.0) as usize);
        for (control, &value) in wanted.iter().enumerate() {
            if control != PARAM_CHARACTER {
                self.set_fx_param(track, slot, control, value);
            }
        }
        let name = phosphor_app::fx::CHARACTERS
            [(index.round().max(0.0) as usize).min(CHARACTER_COUNT - 1)];
        self.status_message = Some((
            format!("{}: {}", name.name, name.note),
            std::time::Instant::now(),
        ));
    }

    /// The tracks the key selector can point at, by mixer id, in strip order.
    ///
    /// Every instrument or audio track except this one — a track cannot key
    /// off itself, and the mixer refuses it anyway. The list is rebuilt every
    /// press rather than cached, because a track added or deleted while the
    /// panel is open must not leave the selector pointing into a stale list.
    fn comp_key_choices(&self) -> Vec<usize> {
        let here = self.nav.current_track().and_then(|t| t.mixer_id);
        self.nav
            .tracks
            .iter()
            .filter(|t| matches!(t.kind, TrackKind::Instrument | TrackKind::Audio))
            .filter_map(|t| t.mixer_id)
            .filter(|id| Some(*id) != here)
            .collect()
    }

    /// Step the key selector: `internal`, then every other track by name.
    fn step_comp_key(&mut self, delta: i32) {
        let choices = self.comp_key_choices();
        let Some(track) = self.nav.tracks.get(self.nav.track_cursor) else { return };
        if !matches!(track.kind, TrackKind::Instrument | TrackKind::Audio) {
            self.status_message =
                Some(("a bus has no key \u{2014} put the compressor on a track".into(),
                      std::time::Instant::now()));
            return;
        }
        let at = track
            .key_source
            .and_then(|id| choices.iter().position(|c| *c == id))
            .map_or(0, |p| p + 1);
        let next = (at as i32 + delta).clamp(0, choices.len() as i32) as usize;
        let source = (next > 0).then(|| choices[next - 1]);
        self.set_key_source(self.nav.track_cursor, source);
    }

    /// Point a track's sidechain at another track, on both sides.
    pub(crate) fn set_key_source(&mut self, track_index: usize, source: Option<usize>) {
        let name = source.and_then(|id| {
            self.nav
                .tracks
                .iter()
                .find(|t| t.mixer_id == Some(id))
                .map(|t| t.name.clone())
        });
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(track_id) = track.mixer_id else { return };
        track.key_source = source;
        // Remembered only so a key whose track is later deleted can name it.
        if source.is_some() {
            track.key_source_name = name.clone();
        }
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetKeySource { track_id, source });
        self.status_message = Some((
            match &name {
                Some(name) => format!("key \u{25B8} {name}"),
                None => "key \u{25B8} internal".to_string(),
            },
            std::time::Instant::now(),
        ));
    }

    // ── Key listen ──

    /// Put the key on the monitor path in place of this track's output, or
    /// take it off again.
    ///
    /// One at a time, and the type is what says so: `key_listen` is an
    /// `Option`, on this side and on the audio thread's, so arming a second
    /// one disarms the first without anybody having to remember to.
    pub(crate) fn set_key_listen(&mut self, track_id: Option<usize>) {
        if self.nav.key_listen == track_id {
            return;
        }
        self.nav.key_listen = track_id;
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetKeyListen { track: track_id });
    }

    /// Arm or disarm the key listen on the strip under the cursor.
    fn set_key_listen_here(&mut self, on: bool) {
        let Some(track) = self.nav.current_track() else { return };
        if !matches!(track.kind, TrackKind::Instrument | TrackKind::Audio) {
            self.status_message = Some((
                "a bus has no key to listen to".into(),
                std::time::Instant::now(),
            ));
            return;
        }
        let Some(id) = track.mixer_id else { return };
        let name = track.name.clone();
        self.set_key_listen(on.then_some(id));
        self.status_message = Some((
            if on {
                format!("key listen: {name} is playing its key \u{2014} esc puts it back")
            } else {
                "key listen off".to_string()
            },
            std::time::Instant::now(),
        ));
    }

    fn toggle_key_listen(&mut self) {
        let armed = self
            .nav
            .current_track()
            .and_then(|t| t.mixer_id)
            .is_some_and(|id| self.nav.key_listen == Some(id));
        self.set_key_listen_here(!armed);
    }

    /// **Key listen never outlives the panel it was armed from.**
    ///
    /// Called every frame, so that every way out of the panel is covered by
    /// one rule rather than by a clear on each of them: closing it, opening a
    /// different slot, deleting the compressor, switching tracks, loading a
    /// session. If the compressor whose panel armed it is not still open on
    /// the track it names, the monitor goes back.
    ///
    /// The transport's stop is handled on the other side, by the mixer itself,
    /// so that a front end which has stopped answering cannot leave a mix with
    /// a hole in it either.
    pub(crate) fn enforce_key_listen(&mut self) {
        let Some(id) = self.nav.key_listen else { return };
        let still_open = self.nav.clip_view.fx.slot.is_some_and(|slot| {
            self.nav.current_track().is_some_and(|track| {
                track.mixer_id == Some(id)
                    && track
                        .fx_chain
                        .get(slot)
                        .is_some_and(|s| s.fx_type == FxType::Compressor)
            })
        });
        if !still_open {
            self.set_key_listen(None);
        }
    }
}

/// The next ratio a manual would print, in the direction of travel.
fn next_ratio_stop(percent: f32, delta: i32) -> f32 {
    let here = f64::from(percent);
    if delta > 0 {
        RATIO_STOPS
            .iter()
            .map(|r| f64::from(ratio_to_percent(*r)))
            .find(|stop| *stop > here + 0.01)
            .unwrap_or(100.0) as f32
    } else {
        RATIO_STOPS
            .iter()
            .rev()
            .map(|r| f64::from(ratio_to_percent(*r)))
            .find(|stop| *stop < here - 0.01)
            .unwrap_or(0.0) as f32
    }
}

/// The detector high-pass, along the ISO sixth-octave centres, with `off` one
/// press below the bottom of the travel.
fn step_sc_hpf(current: f32, delta: i32, coarse: bool) -> f32 {
    if current < SC_HPF_MIN_HZ {
        return if delta > 0 { SC_HPF_MIN_HZ } else { 0.0 };
    }
    let mut hz = f64::from(current);
    for _ in 0..if coarse { 6 } else { 1 } {
        hz = if delta > 0 { iso_step_up(hz) } else { iso_step_down(hz) };
    }
    if hz < f64::from(SC_HPF_MIN_HZ) {
        // One press below the bottom is off, which is where the control
        // ships and where an external key usually wants it.
        return 0.0;
    }
    (hz as f32).min(SC_HPF_MAX_HZ)
}
