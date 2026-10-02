//! The eight track columns: a button and a fader for each of eight tracks.
//!
//! The columns hold the tracks in banks of eight, in the track list's own
//! order — TRACKS ◀ ▶ moves the bank. A track button selects its track, or
//! mutes, solos or arms it, by the mode buttons beside them; a fader is its
//! track's level, picked up rather than jumped to.
//!
//! Mute, solo and arm are the toggles `m`, `s` and `r` make, on the column's
//! track rather than the cursor's; a fader moves the level through the same
//! fader step the keys use, so both keep their undo and their readout.

use super::*;

use phosphor_app::surface::binding::TrackMode;
use phosphor_app::surface::fader::steps_between;
use phosphor_app::surface::layout::COLUMNS;

const BANK: usize = COLUMNS as usize;

impl App {
    /// The tracks the columns hold, left to right.
    pub(crate) fn deck_bank_tracks(&self) -> impl Iterator<Item = usize> {
        let first = self.deck.track_bank * BANK;
        first..(first + BANK).min(self.nav.tracks.len())
    }

    /// The track under column `n`, if there is one.
    fn column_track(&self, n: u8) -> Option<usize> {
        let idx = self.deck.track_bank * BANK + usize::from(n);
        (idx < self.nav.tracks.len()).then_some(idx)
    }

    /// TRACKS ◀ ▶: the next eight tracks.
    pub(crate) fn deck_track_bank(&mut self, delta: i8) {
        let banks = self.nav.tracks.len().div_ceil(BANK).max(1);
        let at = (self.deck.track_bank as i32 + i32::from(delta)).clamp(0, banks as i32 - 1) as usize;
        self.deck.track_bank = at;
        let last = ((at + 1) * BANK).min(self.nav.tracks.len());
        self.flash(format!("columns: tracks {}-{last}", at * BANK + 1));
    }

    /// A track button.
    pub(crate) fn track_button(&mut self, n: u8) {
        let Some(idx) = self.column_track(n) else {
            return self.flash(format!("no track under column {}", n + 1));
        };
        match self.deck.track_mode {
            TrackMode::Select => {
                self.nav.focus_pane(Pane::Tracks);
                self.nav.element_locked = false;
                self.nav.track_cursor = idx;
                self.nav.track_selected = true;
                self.nav.track_element = crate::state::TrackElement::Label;
                self.nav.show_current_track_controls();
            }
            mode => {
                let saved = self.nav.track_cursor;
                self.nav.track_cursor = idx;
                match mode {
                    TrackMode::Mute => self.nav.toggle_mute(),
                    TrackMode::Solo => self.nav.toggle_solo(),
                    _ => self.nav.toggle_arm(),
                }
                self.nav.track_cursor = saved;
            }
        }
    }

    /// Fader `n` moved to `position`.
    pub(crate) fn move_deck_fader(&mut self, n: u8, position: u8) {
        let Some(idx) = self.column_track(n) else { return };
        let slot = usize::from(n);
        let current = self.nav.tracks[idx].volume_db().map(|db| db.round() as i32);
        // Someone else moved the level, or the column now holds another
        // track: the fader has to catch it again.
        if self.deck.held[slot].is_some_and(|(track, level)| track != idx || level != current) {
            self.deck.pickups[slot].release();
        }
        let Some(target) = self.deck.pickups[slot].moved(position, current) else {
            self.deck.held[slot] = Some((idx, current));
            return;
        };
        let steps = steps_between(current, target);
        if steps != 0 {
            self.step_mix(idx, crate::state::TrackElement::Volume, steps);
        }
        let now = self.nav.tracks[idx].volume_db().map(|db| db.round() as i32);
        self.deck.held[slot] = Some((idx, now));
    }
}
