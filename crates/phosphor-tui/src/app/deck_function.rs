//! The function knob and the buttons around the edit row.
//!
//! One knob, six jobs: TEMPO, SWING, GRID, MASTER, LOOP and LAST pick what it
//! turns, and it stays on that until another is picked. Each turn goes
//! through what the keys already reach — `+`/`-` for the tempo, the
//! sequencer's own swing op, the piano roll's grid list, the master's fader
//! step, the loop brace's `H`/`L` — so every one of them undoes the way the
//! keyboard's does.
//!
//! Beside it: TAP, BROWSE, DELETE and DUPLICATE, which mean the obvious
//! thing on each screen, and the deck's line on screen.

use super::*;

use std::time::{Duration, Instant};

use phosphor_app::sequencer::ops::SeqOp;
use phosphor_app::surface::binding::{Chord, FnTarget};
use phosphor_app::surface::screen::{screen, Screen};

/// Taps further apart than this start a new count.
const TAP_RESET: Duration = Duration::from_secs(2);
/// Taps closer than this are a bounce, not a beat (300 bpm).
const TAP_BOUNCE: Duration = Duration::from_millis(200);

fn target_label(target: FnTarget) -> &'static str {
    match target {
        FnTarget::Tempo => "tempo",
        FnTarget::Swing => "swing",
        FnTarget::Grid => "grid",
        FnTarget::Master => "master",
        FnTarget::Loop => "loop end",
        FnTarget::Last => "last knob",
    }
}

impl App {
    /// A function button: the knob turns this now.
    pub(crate) fn point_function(&mut self, target: FnTarget) {
        self.deck.function = target;
        self.flash(format!("function knob: {}", target_label(target)));
    }

    pub(crate) fn turn_function(&mut self, detents: i8) {
        let up = detents >= 0;
        if self.deck.function == FnTarget::Last {
            return self.turn_last(detents);
        }
        for _ in 0..detents.unsigned_abs() {
            match self.deck.function {
                FnTarget::Tempo => self.press_chord(Chord::ch(if up { '+' } else { '-' })),
                FnTarget::Swing => {
                    if self.nav.current_track().and_then(|t| t.sequencer.as_ref()).is_none() {
                        return self.flash("swing belongs to a step grid \u{00b7} select a sequencer track");
                    }
                    self.sequencer_op(SeqOp::NudgeSwing(if up { 1 } else { -1 }));
                }
                FnTarget::Grid => {
                    let roll = &mut self.nav.clip_view.piano_roll;
                    roll.grid = if up { roll.grid.next() } else { roll.grid.prev() };
                    let words = format!("grid: {}", self.nav.clip_view.piano_roll.grid.label());
                    self.flash(words);
                }
                FnTarget::Master => {
                    let Some(master) = self.nav.tracks.iter().position(|t| t.kind == TrackKind::Master) else {
                        return;
                    };
                    self.step_mix(master, crate::state::TrackElement::Volume, if up { 1 } else { -1 });
                }
                FnTarget::Loop => {
                    self.edit_loop_range(|l| if up { l.move_end_right() } else { l.move_end_left() });
                    let words = format!("loop: {}", self.nav.loop_editor.display());
                    self.flash(words);
                }
                FnTarget::Last => {}
            }
        }
    }

    /// TAP: two or more taps set the tempo to their average beat.
    pub(crate) fn tap_tempo(&mut self, now: Instant) {
        if let Some(&last) = self.deck.taps.last() {
            let gap = now.duration_since(last);
            if gap < TAP_BOUNCE {
                return;
            }
            if gap > TAP_RESET {
                self.deck.taps.clear();
            }
        }
        self.deck.taps.push(now);
        // The last four beats: enough to average out a shaky hand, few
        // enough to follow a change of mind.
        let excess = self.deck.taps.len().saturating_sub(5);
        self.deck.taps.drain(..excess);
        let taps = &self.deck.taps;
        if taps.len() < 2 {
            return self.flash("tap \u{00b7} keep tapping");
        }
        let span = taps[taps.len() - 1].duration_since(taps[0]).as_secs_f64();
        let bpm = (60.0 * (taps.len() - 1) as f64 / span).round();
        let current = self.engine.transport.tempo_bpm();
        self.nudge_tempo(bpm - current);
    }

    /// BROWSE: the screen's own list of things to load.
    pub(crate) fn deck_browse(&mut self) {
        match screen(&self.nav) {
            Screen::Instrument | Screen::Steps => self.press_menu('w'),
            Screen::Pads | Screen::Zones => self.press_chord(Chord::ch('a')),
            Screen::Tracks | Screen::Track => self.press_menu('o'),
            other => self.flash(format!("{}: nothing to browse here", other.label())),
        }
    }

    /// DELETE: the track on the track list, the thing under the cursor
    /// everywhere else — each asks first where the keyboard's `d` does.
    pub(crate) fn deck_delete(&mut self) {
        match screen(&self.nav) {
            Screen::Tracks | Screen::Track => self.press_menu('d'),
            _ => self.press_chord(Chord::ch('d')),
        }
    }

    pub(crate) fn deck_duplicate(&mut self) {
        match screen(&self.nav) {
            Screen::Track => self.press_chord(Chord::ch('D')),
            Screen::TrackCell => self.press_chord(Chord::ch('d')),
            other => self.flash(format!("{}: nothing to duplicate here", other.label())),
        }
    }

    /// PART: the next part of the locked track — the keyboard's Tab.
    pub(crate) fn next_part(&mut self) {
        self.press_chord(Chord::key(phosphor_app::surface::binding::Key::Tab));
    }

    /// The deck's line: what is locked, what the eight knobs are, and what
    /// the function knob and the track buttons are doing.
    pub(crate) fn deck_line(&self) -> String {
        let now = screen(&self.nav);
        let bank = self.bank();
        let mut line = format!("DECK \u{00b7} {}", now.label());
        if let Some(page) = bank.get(self.deck.page) {
            if bank.len() > 1 {
                line.push_str(&format!(" \u{00b7} page {}/{}", self.deck.page + 1, bank.len()));
            }
            line.push_str(" \u{00b7}");
            for (i, k) in page.iter().enumerate() {
                line.push_str(&format!(" {}:{}", i + 1, k.label));
            }
        }
        let first = self.deck.track_bank * 8 + 1;
        line.push_str(&format!(
            " \u{00b7} fn {} \u{00b7} tracks {first}+ {}",
            target_label(self.deck.function),
            format!("{:?}", self.deck.track_mode).to_lowercase(),
        ));
        line
    }
}
