//! Keys → [`SeqOp`]. Nothing else.
//!
//! Every key in the step grid names an operation and hands it to
//! [`App::sequencer_op`]. Not one of them reaches into a pattern, which is
//! what makes the same grid drivable by a box with sixteen buttons on it
//! later: the controller maps to the same ops, and there is no second
//! implementation of "toggle the step under the cursor" to disagree with.
//!
//! # The grammar
//!
//! The house pattern, applied to a machine with four bands of controls:
//!
//! ```text
//! j/k      down the screen      the sounds, then the panels under them
//! h/l      along a row          steps, knobs, slots
//! n        write a hit under the cursor
//! enter    grid: open its panel · step/pattern: hold the knob · slots: queue
//! esc      release a knob, then leave the band, then leave the view
//! [ ]      the sound — one keypress, at any depth, never a locked knob
//! ```
//!
//! `j` and `k` walk the rows that are on the screen. On a kit those are the
//! sounds, so the hand that reaches down from the kick gets the snare; on a
//! keyboard they are the eight voices a chord can be layered across. Off the
//! last one it carries on into the panels below, and back up off the step
//! panel it lands on the last row again. One column of things to stand on,
//! top to bottom.
//!
//! A locked knob takes every key it is given, exactly as the fader does:
//! `h`/`l` move it, `H`/`L` move it in strides, `Esc` lets go, and nothing
//! else gets a look. That is what makes `h` mean "adjust" without it also
//! meaning "move the cursor".

use super::*;

use phosphor_app::sequencer::ops::SeqOp;
use phosphor_app::sequencer::SequencerState;
use phosphor_core::pattern::{LANES, SLOTS};

use crate::state::{SeqBand, SeqKnob};

/// The controls the step band shows, in cursor order.
///
/// Two panels, because a drum lane and a keyboard lane are different
/// instruments: a lane pinned to a kit voice has no pitch to set, and a step
/// on it says only *when*.
pub(crate) fn step_knobs(state: &SequencerState) -> &'static [SeqKnob] {
    if state.lane().is_pitched() {
        &[SeqKnob::Pitch, SeqKnob::Chord, SeqKnob::Voicing, SeqKnob::RootBelow, SeqKnob::Gate]
    } else {
        &[SeqKnob::Voice, SeqKnob::Mute, SeqKnob::Solo]
    }
}

/// The controls the pattern band shows, in cursor order. The child comes
/// first: which instrument the sequencer drives is the biggest decision on
/// the band, and a knob nobody scrolls to is a knob nobody finds.
pub(crate) const PATTERN_KNOBS: [SeqKnob; 10] = [
    SeqKnob::Child,
    SeqKnob::Length,
    SeqKnob::Rate,
    SeqKnob::Swing,
    SeqKnob::DefaultGate,
    SeqKnob::BaseVelocity,
    SeqKnob::AccentVelocity,
    SeqKnob::Mode,
    SeqKnob::Tonic,
    SeqKnob::Switch,
];

/// The knobs of whichever band has the cursor, or none for the two bands that
/// have no knobs on them.
pub(crate) fn knobs_of(state: &SequencerState, band: SeqBand) -> &'static [SeqKnob] {
    match band {
        SeqBand::Step => step_knobs(state),
        SeqBand::Pattern => &PATTERN_KNOBS,
        SeqBand::Grid | SeqBand::Slots => &[],
    }
}

/// One press of a knob, as operations.
///
/// `coarse` is the shifted press: an octave rather than a semitone, five
/// percent rather than one. A control with no coarse setting ignores it,
/// which is better than inventing a stride nobody asked for.
///
/// Toggles read their own state so that a knob turned right always means on:
/// `h` and `l` on a switch are "off" and "on", not "flip" twice.
fn knob_ops(state: &SequencerState, knob: SeqKnob, delta: i8, coarse: bool) -> Vec<SeqOp> {
    let pattern = state.pattern();
    let step = state.step();
    let up = delta > 0;
    match knob {
        SeqKnob::Pitch => {
            vec![if coarse { SeqOp::NudgeOctave(delta) } else { SeqOp::NudgePitch(delta) }]
        }
        SeqKnob::Chord => vec![SeqOp::CycleChord(delta)],
        SeqKnob::Voicing => vec![SeqOp::CycleVoicing(delta)],
        SeqKnob::RootBelow => {
            if step.root_below() == up { Vec::new() } else { vec![SeqOp::ToggleRootBelow] }
        }
        SeqKnob::Gate => {
            // The gate walks in fives and off the top into the tie, so a
            // coarse press is five of them rather than a different control.
            vec![SeqOp::NudgeGate(if coarse { delta.saturating_mul(5) } else { delta })]
        }
        SeqKnob::Voice => {
            let note = state.lane().note;
            let stride = i32::from(delta) * if coarse { 12 } else { 1 };
            let next = (i32::from(note) + stride).clamp(0, 127) as u8;
            vec![SeqOp::SetLaneNote(next)]
        }
        SeqKnob::Mute => {
            if state.lane().muted == up { Vec::new() } else { vec![SeqOp::ToggleLaneMute] }
        }
        SeqKnob::Solo => {
            if state.lane().soloed == up { Vec::new() } else { vec![SeqOp::ToggleLaneSolo] }
        }
        SeqKnob::Length => vec![SeqOp::CycleLength(delta)],
        SeqKnob::Rate => vec![SeqOp::CycleRate(delta)],
        SeqKnob::Swing => vec![SeqOp::NudgeSwing(if coarse { delta.saturating_mul(5) } else { delta })],
        SeqKnob::DefaultGate => vec![SeqOp::NudgeDefaultGate(delta)],
        SeqKnob::BaseVelocity => {
            vec![SeqOp::NudgeBaseVelocity(if coarse { delta.saturating_mul(10) } else { delta })]
        }
        SeqKnob::AccentVelocity => {
            vec![SeqOp::NudgeAccentVelocity(if coarse { delta.saturating_mul(10) } else { delta })]
        }
        SeqKnob::Mode => vec![SeqOp::CycleMode(delta)],
        SeqKnob::Tonic => {
            let next = (i32::from(pattern.tonic) + i32::from(delta)).rem_euclid(12) as u8;
            vec![SeqOp::SetTonic(next)]
        }
        SeqKnob::Switch => vec![SeqOp::CycleSwitchQuant(delta)],
        // Needs the track, not the sequencer state — handled by the caller.
        SeqKnob::Child => Vec::new(),
    }
}

/// The instruments a sequencer may drive: everything except itself.
pub(crate) fn child_choices() -> impl Iterator<Item = InstrumentType> {
    InstrumentType::ALL.iter().copied().filter(|t| !t.is_sequencer())
}

impl App {
    /// One key, in the step grid.
    pub(crate) fn handle_sequencer_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crate::debug_log as dbg;

        // A tab open on a track with no sequencer on it can still be left.
        if self.nav.current_track().and_then(|t| t.sequencer.as_deref()).is_none() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.nav.escape();
            }
            return;
        }

        dbg::user(&format!(
            "sequencer: {:?} band={:?} knob={} locked={}",
            key.code,
            self.nav.clip_view.sequencer.band,
            self.nav.clip_view.sequencer.knob,
            self.nav.clip_view.sequencer.locked,
        ));

        if self.nav.clip_view.sequencer.locked {
            self.sequencer_locked_key(key);
            return;
        }

        // A number being typed is abandoned the moment anything else is
        // pressed, so a `1` left over from a change of mind cannot turn the
        // next `2` into step twelve.
        if !matches!(key.code, KeyCode::Char('0'..='9')) {
            self.nav.clip_view.sequencer.digits.clear();
        }
        if self.sequencer_band_key(key) {
            return;
        }
        self.sequencer_view_key(key);
    }

    /// A knob is held: `h`/`l` move it, `H`/`L` move it in strides, and every
    /// other key is swallowed so that nothing leaks out of a locked control.
    fn sequencer_locked_key(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.nav.clip_view.sequencer.locked = false;
                self.status_message = None;
            }
            KeyCode::Char('H') => self.sequencer_adjust(-1, true),
            KeyCode::Char('L') => self.sequencer_adjust(1, true),
            KeyCode::Char('h') | KeyCode::Left => self.sequencer_adjust(-1, shift),
            KeyCode::Char('l') | KeyCode::Right => self.sequencer_adjust(1, shift),
            _ => {}
        }
    }

    /// Turn the knob under the cursor.
    fn sequencer_adjust(&mut self, delta: i8, coarse: bool) {
        let view_band = self.nav.clip_view.sequencer.band;
        let cursor = self.nav.clip_view.sequencer.knob;
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return;
        };
        let knobs = knobs_of(state, view_band);
        let Some(&knob) = knobs.get(cursor) else { return };
        // The child knob walks the instrument list, which lives on the track
        // rather than in the sequencer state the pure knob table can see.
        if knob == SeqKnob::Child {
            let current = self.nav.current_track().and_then(|t| t.instrument_type);
            let choices: Vec<_> = child_choices().collect();
            let at = current
                .and_then(|c| choices.iter().position(|&x| x == c))
                .unwrap_or(0);
            let next = (at as i32 + i32::from(delta)).rem_euclid(choices.len() as i32) as usize;
            self.sequencer_op(SeqOp::SetChild(choices[next]));
            return;
        }
        let ops = knob_ops(state, knob, delta, coarse);
        for op in ops {
            self.sequencer_op(op);
        }
    }

    /// One row down the screen.
    fn sequencer_down(&mut self) {
        if self.nav.clip_view.sequencer.band != SeqBand::Grid {
            self.nav.clip_view.sequencer.move_band(1);
            return;
        }
        let lane = self
            .nav
            .current_track()
            .and_then(|t| t.sequencer.as_deref())
            .map_or(0, SequencerState::lane_cursor);
        if lane + 1 < LANES {
            self.sequencer_op(SeqOp::MoveLane(1));
        } else {
            self.nav.clip_view.sequencer.move_band(1);
        }
    }

    /// One row up the screen.
    fn sequencer_up(&mut self) {
        let band = self.nav.clip_view.sequencer.band;
        if band == SeqBand::Grid {
            self.sequencer_op(SeqOp::MoveLane(-1));
            return;
        }
        self.nav.clip_view.sequencer.move_band(-1);
        // Coming back into the grid lands on the row it was left from, which
        // is the bottom one — the same square the cursor walked off.
        if band == SeqBand::Step {
            self.sequencer_op(SeqOp::SelectLane(LANES as u8 - 1));
        }
    }

    /// The keys whose meaning depends on which band has the cursor. Answers
    /// whether the key was one of them.
    fn sequencer_band_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let band = self.nav.clip_view.sequencer.band;
        let arrow_left = matches!(key.code, KeyCode::Char('h') | KeyCode::Left);
        let arrow_right = matches!(key.code, KeyCode::Char('l') | KeyCode::Right);
        let delta: i8 = if arrow_left { -1 } else { 1 };

        // j/k walks *down the screen*. In the grid of a kit that means the
        // lanes, because the lanes are the rows: a hand reaching down from
        // the kick expects the snare, not a panel. Walking off the last lane
        // carries on into the panels below, and walking back up off the step
        // panel lands on the last lane again, so the whole view is one
        // continuous column of things to stand on.
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.sequencer_down();
                return true;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.sequencer_up();
                return true;
            }
            _ => {}
        }

        if !arrow_left && !arrow_right {
            return self.sequencer_enter_key(key);
        }

        match band {
            SeqBand::Grid => self.sequencer_op(SeqOp::MoveStep(delta)),
            SeqBand::Step | SeqBand::Pattern => {
                let count = self
                    .nav
                    .current_track()
                    .and_then(|t| t.sequencer.as_deref())
                    .map_or(0, |state| knobs_of(state, band).len());
                self.nav.clip_view.sequencer.move_knob(i32::from(delta), count);
            }
            // The slot cursor *is* the selected slot: moving it swaps what
            // the grid above is showing, which is the only way to compare two
            // patterns without a second cursor to keep track of.
            SeqBand::Slots => {
                let selected = self
                    .nav
                    .current_track()
                    .and_then(|t| t.sequencer.as_deref())
                    .map_or(0, SequencerState::selected_slot);
                let next = (i32::from(selected) + i32::from(delta)).clamp(0, SLOTS as i32 - 1);
                self.sequencer_op(SeqOp::SelectSlot(next as u8));
            }
        }
        true
    }

    /// Enter, Esc and the digits: one meaning per band.
    fn sequencer_enter_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let band = self.nav.clip_view.sequencer.band;
        match key.code {
            KeyCode::Enter => {
                match band {
                    // Enter opens what is under the cursor, which is what it
                    // does everywhere else in this application: on a track,
                    // on a fader, on a knob, in a menu. It used to write a
                    // hit — a second `n` — and in doing so it took away the
                    // only key a player would think to press to find out
                    // what a step is set to. `n` writes; Enter looks inside.
                    SeqBand::Grid => {
                        self.nav.clip_view.sequencer.focus_band(SeqBand::Step);
                        self.status_message = Some((
                            "step panel: h/l picks a control, enter holds it, esc goes back"
                                .into(),
                            std::time::Instant::now(),
                        ));
                    }
                    SeqBand::Step | SeqBand::Pattern => {
                        let count = self
                            .nav
                            .current_track()
                            .and_then(|t| t.sequencer.as_deref())
                            .map_or(0, |state| knobs_of(state, band).len());
                        if count > 0 {
                            self.nav.clip_view.sequencer.locked = true;
                            self.status_message = Some((
                                "knob held: h/l adjust, H/L strides, esc releases".into(),
                                std::time::Instant::now(),
                            ));
                        }
                    }
                    SeqBand::Slots => self.queue_selected_slot(),
                }
                true
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if band == SeqBand::Grid {
                    self.nav.escape();
                } else {
                    self.nav.clip_view.sequencer.focus_band(SeqBand::Grid);
                }
                true
            }
            KeyCode::Char(ch @ '0'..='9') => {
                self.sequencer_digit(ch);
                true
            }
            _ => false,
        }
    }

    /// A digit types towards a step number in the grid and a slot number in
    /// the pattern strip — the number that is on the screen in front of it in
    /// each case.
    fn sequencer_digit(&mut self, ch: char) {
        let band = self.nav.clip_view.sequencer.band;
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return;
        };
        let max = match band {
            SeqBand::Slots => SLOTS,
            _ => state.pattern().step_count(),
        };
        let Some(number) = self.nav.clip_view.sequencer.type_digit(ch, max) else { return };
        match band {
            SeqBand::Slots => self.sequencer_op(SeqOp::SelectSlot(number as u8 - 1)),
            _ => self.sequencer_op(SeqOp::SelectStep(number as u8 - 1)),
        }
    }

    /// Queue the slot being looked at, or take back the queue when it is
    /// already the one queued — one key, both directions.
    fn queue_selected_slot(&mut self) {
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return;
        };
        let selected = state.selected_slot();
        if state.queued_slot() == Some(selected) {
            self.sequencer_op(SeqOp::ClearQueue);
            self.status_message =
                Some(("queue cleared".into(), std::time::Instant::now()));
            return;
        }
        if state.is_chained() {
            self.status_message = Some((
                "a chain is running — C clears it".into(),
                std::time::Instant::now(),
            ));
            return;
        }
        if state.live_slot() == selected {
            return;
        }
        self.sequencer_op(SeqOp::QueueSlot(selected));
    }

    /// The keys that mean the same thing in every band.
    fn sequencer_view_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('[') => self.sequencer_op(SeqOp::MoveLane(-1)),
            KeyCode::Char(']') => self.sequencer_op(SeqOp::MoveLane(1)),
            KeyCode::Char('n') => self.sequencer_op(SeqOp::ToggleStep),
            KeyCode::Char('a') => self.sequencer_op(SeqOp::ToggleAccent),
            KeyCode::Char('x') => self.sequencer_op(SeqOp::ClearStep),
            KeyCode::Char('m') => self.sequencer_op(SeqOp::ToggleLaneMute),
            KeyCode::Char('s') => self.sequencer_op(SeqOp::ToggleLaneSolo),
            KeyCode::Char('t') => self.toggle_pattern_playback(),
            KeyCode::Char('r') => self.toggle_step_record(),
            KeyCode::Char('b') => self.bounce_pattern(),
            KeyCode::Char('_') => self.sequencer_tie(),
            KeyCode::Char('.') => self.sequencer_op(SeqOp::RecordRest),
            KeyCode::Char('c') => self.chain_selected_slot(),
            KeyCode::Char('C') => {
                self.sequencer_op(SeqOp::ClearChain);
                self.status_message = Some(("chain cleared".into(), std::time::Instant::now()));
            }
            KeyCode::Char('y') => self.yank_pattern(),
            KeyCode::Char('p') => self.paste_pattern(),
            KeyCode::Char('X') => self.clear_pattern(),
            _ => {}
        }
    }

    /// Whether the pattern on this track is generating notes.
    fn toggle_pattern_playback(&mut self) {
        // With the transport stopped, `t` means GO — full stop. Patterns run
        // from birth, so a toggle here would MUTE a fresh pattern: the first
        // real play-through pressed t on a brand-new beat and silenced it
        // while the coaching line was still saying "t — play". A drum
        // machine's start button never needs the machine explained to it.
        // While the transport rolls, `t` is the pattern's mute — stopping the
        // pattern leaves the transport alone, other tracks may be playing.
        if !self.engine.transport.is_playing() {
            self.sequencer_op(SeqOp::SetPlaying(true));
            self.sync_loop_to_transport();
            self.engine.transport.play();
            self.status_message = Some((
                "pattern running · transport started".to_string(),
                std::time::Instant::now(),
            ));
            return;
        }
        self.sequencer_op(SeqOp::TogglePlaying);
        let running = self
            .nav
            .current_track()
            .and_then(|t| t.sequencer.as_deref())
            .is_some_and(SequencerState::is_playing);
        self.status_message = Some((
            if running { "pattern running".to_string() } else { "pattern muted — t unmutes".to_string() },
            std::time::Instant::now(),
        ));
    }

    /// Arm step record, so that playing writes into the pattern.
    fn toggle_step_record(&mut self) {
        let armed = self
            .nav
            .current_track()
            .and_then(|t| t.sequencer.as_deref())
            .is_some_and(SequencerState::is_step_recording);
        self.sequencer_op(SeqOp::ArmStepRecord(!armed));
        self.held_notes.clear();
        self.recorded_notes.clear();
        self.status_message = Some((
            if armed {
                "step record off".to_string()
            } else {
                "step record: play to write, . rests, _ ties".to_string()
            },
            std::time::Instant::now(),
        ));
    }

    /// The tie key: while recording it ties the step just written, and
    /// otherwise it holds the step under the cursor into the next one. One
    /// key, because it is one idea.
    fn sequencer_tie(&mut self) {
        let recording = self
            .nav
            .current_track()
            .and_then(|t| t.sequencer.as_deref())
            .is_some_and(SequencerState::is_step_recording);
        self.sequencer_op(if recording { SeqOp::RecordTie } else { SeqOp::ToggleTie });
    }

    /// Add the slot being looked at to the chain — or, when it is already the
    /// last entry, ask for it one more time. Pressing `c` four times on A is
    /// `A×4`, which is how a chain gets written.
    fn chain_selected_slot(&mut self) {
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return;
        };
        let slot = state.selected_slot();
        let chain = state.chain();
        let op = match chain.last() {
            Some(entry) if entry.slot == slot && entry.repeats < 64 => SeqOp::SetChainRepeats {
                index: chain.len() as u8 - 1,
                repeats: entry.repeats + 1,
            },
            _ => SeqOp::PushChainEntry { slot, repeats: 1 },
        };
        self.sequencer_op(op);
    }

    fn yank_pattern(&mut self) {
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return;
        };
        let slot = state.selected_slot();
        self.nav.clip_view.sequencer.copy_from = Some(slot);
        self.status_message = Some((
            format!("pattern {} yanked", (b'A' + slot) as char),
            std::time::Instant::now(),
        ));
    }

    fn paste_pattern(&mut self) {
        let Some(from) = self.nav.clip_view.sequencer.copy_from else {
            self.status_message = Some(("no pattern yanked".into(), std::time::Instant::now()));
            return;
        };
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return;
        };
        let to = state.selected_slot();
        self.sequencer_op(SeqOp::CopyPattern { from, to });
        self.status_message = Some((
            format!("pattern {} → {}", (b'A' + from) as char, (b'A' + to) as char),
            std::time::Instant::now(),
        ));
    }

    fn clear_pattern(&mut self) {
        let letter = self
            .nav
            .current_track()
            .and_then(|t| t.sequencer.as_deref())
            .map_or('A', |s| (b'A' + s.selected_slot()) as char);
        self.sequencer_op(SeqOp::ClearPattern);
        self.status_message =
            Some((format!("pattern {letter} cleared"), std::time::Instant::now()));
    }
}
