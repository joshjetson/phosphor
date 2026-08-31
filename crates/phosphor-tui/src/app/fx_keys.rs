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
        let len = self.nav.current_track().map_or(0, |t| t.fx_chain.len());
        let cursor = self.nav.clip_view.fx_cursor.min(len.saturating_sub(1));
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if len > 0 => {
                self.nav.clip_view.fx_cursor = (cursor + 1).min(len - 1);
            }
            KeyCode::Char('k') | KeyCode::Up if len > 0 => {
                self.nav.clip_view.fx_cursor = cursor.saturating_sub(1);
            }
            KeyCode::Enter if len > 0 => self.open_fx_panel(cursor),
            KeyCode::Char('b') if len > 0 => self.toggle_fx_bypass(cursor),
            KeyCode::Char('[') if len > 1 => self.move_fx(cursor, cursor.wrapping_sub(1)),
            KeyCode::Char(']') if len > 1 && cursor + 1 < len => self.move_fx(cursor, cursor + 1),
            KeyCode::Char('d') if len > 0 => self.request_fx_delete(cursor),
            KeyCode::Char('a') => {
                self.nav.fx_menu.open = true;
                self.nav.fx_menu.cursor = 0;
            }
            _ => return false,
        }
        true
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
                FxType::Reverb => {
                    "j/k picks a knob, h/l adjusts, H/L strides, esc goes back".into()
                }
                other => format!("{} has no panel yet", other.label()),
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
        self.status_message = Some((
            format!("{label} \u{2192} slot {}", to + 1),
            std::time::Instant::now(),
        ));
    }

    /// Ask before taking an effect out: the chain is work, and there is no
    /// undo for it yet.
    fn request_fx_delete(&mut self, slot: usize) {
        let Some(track) = self.nav.current_track() else { return };
        let Some(instance) = track.fx_chain.get(slot) else { return };
        let label = instance.fx_type.label();
        self.nav.clip_view.fx_cursor = slot;
        self.nav.confirm_modal.show(
            ConfirmKind::DeleteFx,
            &format!("remove {label} from slot {}?", slot + 1),
        );
    }

    /// Take the effect out of the slot under the cursor, on both sides.
    pub(crate) fn remove_fx_at_cursor(&mut self) {
        let index = self.nav.track_cursor;
        let slot = self.nav.clip_view.fx_cursor;
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

        self.nav.clip_view.fx_cursor = slot.min(len.saturating_sub(1));
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
        self.status_message =
            Some((format!("{label} removed"), std::time::Instant::now()));
    }

    // ── The panel ──

    /// One key, in an effect's panel.
    pub(crate) fn handle_fx_panel_keys(&mut self, key: crossterm::event::KeyEvent) {
        if self.open_fx_type() == Some(FxType::Reverb) {
            self.handle_reverb_panel_keys(key);
            return;
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
            KeyCode::Char('b') => {
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
        let slot = self.nav.clip_view.fx.slot?;
        Some(self.nav.current_track()?.fx_chain.get(slot)?.fx_type)
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
            KeyCode::Char('b') => {
                let slot = self.nav.clip_view.fx.slot.unwrap_or(0);
                self.toggle_fx_bypass(slot);
            }
            _ => {}
        }
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
