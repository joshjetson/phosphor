//! Keys mode's operations: the switch, the three ways to make a zone, the
//! brace that resizes one, and the three ways a root arrives.
//!
//! Its own file rather than more of [`super::sampler_ops`] for the trim
//! strip's reason: keys mode is a mode. It has its own keys, its own truth
//! ([`phosphor_app::sampler::zones`]) and its own way of naming what is
//! being edited. What it does *not* have is its own way of reaching the
//! engine — every edit here goes state first, then the undo step, then the
//! same [`App::sync_sampler_span`] every other pad edit leaves by, because
//! an edit that changes the screen and not the signal is the hardest kind
//! of bug to see.
//!
//! # What a mode switch has to resend
//!
//! Both modes' keys, every time. The engine holds whatever it was last
//! told, so a pad that only sounds in the mode being left has to be told it
//! is empty now — the lesson the M3 undo union taught, applied to a switch
//! instead of to a step back.
//!
//! # Where a root comes from
//!
//! Three doors, one destination
//! ([`SamplerState::set_edit_root`](phosphor_app::sampler::SamplerState::set_edit_root)):
//! a capture knows the pitch because it recorded the performance, a file
//! name is read on the way in, and `R` arms the keyboard so the next key
//! played is the answer. The third one takes the MIDI tap the way the
//! progression editor's learn does — see [`App::handle_tap_event`] — because
//! a key played to answer a question must not also walk the cursor off the
//! zone that asked it.

use super::*;

use phosphor_app::sampler::{MapMode, SamplerState, ZoneEdge, NUM_PADS};

use crate::state::undo::{UndoGesture, UndoScope};

impl App {
    // ── Which mode ──

    /// What the bed under the cursor means right now.
    pub(crate) fn sampler_mode(&self) -> MapMode {
        self.cursor_sampler().map_or(MapMode::Pads, |s| s.mode)
    }

    /// The zone under the cursor, when keys mode is on and one covers it.
    pub(crate) fn cursor_sampler_zone(&self) -> Option<usize> {
        match self.sampler_mode() {
            MapMode::Keys => self.cursor_sampler().and_then(SamplerState::cursor_zone),
            MapMode::Pads => None,
        }
    }

    /// `K`: pads or keys.
    ///
    /// Capital and deliberate, because a mode is a decision — lowercase `k`
    /// walks the knob cursor, and a mode switch under a key that is pressed
    /// all day would be a kit that changes shape by accident.
    pub(crate) fn toggle_sampler_map_mode(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let mode = sampler.mode.other();
        sampler.set_mode(mode);
        // The same set either side of the switch — it is the union of both
        // modes — so one read of it covers what starts sounding and what
        // has to stop.
        let pads = sampler.sounding_pads();
        let zones = sampler.zones.len();
        self.nav.commit_undo(before, "map mode");
        for pad in pads {
            self.sync_sampler_pad(track_idx, pad);
        }
        // The two modes offer different controls, so a cursor left where it
        // was would be standing on a different knob than it was a moment
        // ago — and a *held* one would be turning it.
        let view = &mut self.nav.clip_view.sampler;
        view.knob = 0;
        view.locked = false;
        view.trim = None;
        view.root_learn = false;
        self.stop_sampler_preview();
        self.clamp_sampler_cursors();
        self.flash(match (mode, zones) {
            (MapMode::Pads, _) => "pads \u{00b7} every key is its own sound again".to_string(),
            (MapMode::Keys, 0) => format!(
                "keys \u{00b7} no zones yet \u{00b7} {}",
                SamplerState::no_zone_message(),
            ),
            (MapMode::Keys, 1) => "keys \u{00b7} 1 zone".to_string(),
            (MapMode::Keys, n) => format!("keys \u{00b7} {n} zones"),
        });
    }

    // ── Making a zone ──

    /// `w`: the whole bed.
    pub(crate) fn zone_whole(&mut self) {
        self.set_zone_span(0, NUM_PADS - 1, "the whole bed");
    }

    /// `o`: the octave the cursor is standing in, C to B.
    pub(crate) fn zone_octave(&mut self) {
        let Some(cursor) = self.cursor_sampler().map(|s| s.cursor) else { return };
        let note = SamplerState::note_of_pad(cursor);
        let base = note - note % 12;
        // Either end of the bed is half an octave: the bottom three keys
        // are an A-B, and the top is a lone C. Clamping rather than
        // refusing, because the player asked for the octave they are in.
        let lo = SamplerState::pad_of_note(base).unwrap_or(0);
        let hi = SamplerState::pad_of_note(base + 11).unwrap_or(NUM_PADS - 1);
        self.set_zone_span(lo, hi, "one octave");
    }

    /// Throw the brace across `lo..=hi`: a new zone on bare keys, or the
    /// zone under the cursor resized when there is one.
    fn set_zone_span(&mut self, lo: usize, hi: usize, what: &str) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        if !self.in_keys_mode() {
            return;
        }
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let made = sampler.cursor_zone().is_none();
        let span = sampler.zone_span(lo, hi);
        let title = sampler.edit_title();
        let empty = sampler.edited().is_some_and(|state| state.layers.is_empty());
        self.nav.commit_undo(before, "zone span");
        let crowded = self.zone_overflow_note(track_idx);
        // The panel points at the span, which is the thing that was just
        // thrown — and the thing `enter` then holds.
        self.nav.clip_view.sampler.knob = 0;
        self.sync_sampler_span(track_idx, span);
        self.clamp_sampler_cursors();
        self.flash(format!(
            "{title} \u{00b7} {what} \u{00b7} {}{crowded}",
            match (made, empty) {
                (_, true) => "a loads a sound into it",
                (true, false) => "seeded from the pad under the cursor",
                (false, false) => "enter holds the brace",
            },
        ));
    }

    /// `s`: split the zone under the cursor at the cursor key.
    pub(crate) fn zone_split(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        if !self.in_keys_mode() {
            return;
        }
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let span = match sampler.zone_split() {
            Ok(span) => span,
            Err(message) => {
                self.flash(message);
                return;
            }
        };
        let title = sampler.edit_title();
        self.nav.commit_undo(before, "split zone");
        self.nav.clip_view.sampler.knob = 0;
        self.sync_sampler_span(track_idx, span);
        self.clamp_sampler_cursors();
        self.flash(format!(
            "{title} \u{00b7} split \u{00b7} rooted at its own first key \u{00b7} u undoes it",
        ));
    }

    /// `D`: ask before taking a zone off the bed.
    ///
    /// Capital, because lowercase `d` is the layer list's and a zone is the
    /// bigger thing — the same shift the rest of the box uses for "the
    /// larger version of this key". Asked about for the layer's reason: a
    /// zone can hold a take that took a performance to make.
    pub(crate) fn request_zone_delete(&mut self) {
        let Some(sampler) = self.cursor_sampler() else { return };
        if sampler.cursor_zone().is_none() {
            self.flash(SamplerState::no_zone_message());
            return;
        }
        let message = format!("take {} off the bed?", sampler.edit_title());
        self.nav.confirm_modal.show(ConfirmKind::DeleteSamplerZone, &message);
    }

    /// The `y` of that modal.
    ///
    /// The undo step is not only for the player: the slice it captured
    /// holds the last reference on this side to the zone's audio, so the
    /// buffers stay alive until history lets go of them and the audio
    /// thread's own drop is never the last one.
    pub(crate) fn delete_zone(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let Some(index) = self.cursor_sampler_zone() else { return };
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let title = sampler.edit_title();
        let Some(span) = sampler.zone_remove(index) else { return };
        self.nav.commit_undo(before, "remove zone");
        self.sync_sampler_span(track_idx, span);
        self.stop_sampler_preview();
        self.clamp_sampler_cursors();
        self.flash(format!("{title} removed \u{00b7} u brings it back"));
    }

    // ── The brace ──

    /// `h`/`l` and `H`/`L` on a held span: move one edge by one key.
    ///
    /// The loop brace's grammar, which is why `H`/`L` are the other edge
    /// here rather than a stride: a region has two ends, and a player
    /// should not have to remember which mode they are in to move the one
    /// they are looking at.
    pub(crate) fn nudge_zone_edge(&mut self, edge: ZoneEdge, delta: i32) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let Some(index) = self.cursor_sampler_zone() else {
            self.flash(SamplerState::no_zone_message());
            return;
        };
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        // The step is anchored on the edge that is *not* moving, so a whole
        // run of presses carries one key and folds into one step.
        let (lo, hi) = (sampler.zones[index].lo, sampler.zones[index].hi);
        let anchor = match edge {
            ZoneEdge::Low => hi,
            ZoneEdge::High => lo,
        };
        let moved = sampler.move_zone_edge(index, edge, delta);
        let title = sampler.edit_title();
        let keys = sampler.cursor_zone().map_or(0, |i| sampler.zones[i].keys());
        let Some(span) = moved else {
            // Why it did not move: the bed has ends, and so do the zones
            // either side. "It is against something" is not an answer a
            // player can do anything with.
            let stopped = match edge {
                ZoneEdge::Low if lo == 0 => "at the bottom of the bed",
                ZoneEdge::High if hi == NUM_PADS - 1 => "at the top of the bed",
                _ if lo == hi => "against its other edge",
                _ => "against the zone beside it",
            };
            self.flash(format!("{title} \u{00b7} the {} edge is {stopped}", edge.label()));
            return;
        };
        // One step per run, not one per press: holding `l` is a player
        // deciding where the edge goes, and `u` should take back the
        // decision rather than the last thirtieth of it.
        self.nav.commit_undo_coalesced(
            before,
            "zone span",
            UndoGesture::SamplerZone { track_idx, anchor },
        );
        let crowded = self.zone_overflow_note(track_idx);
        self.sync_sampler_span(track_idx, span);
        self.clamp_sampler_cursors();
        self.flash(format!("{title} \u{00b7} {keys} keys{crowded}"));
    }

    // ── Roots ──

    /// `R`: arm the keyboard, or put it back.
    pub(crate) fn toggle_root_learn(&mut self) {
        if self.cursor_sampler_zone().is_none() {
            self.flash(SamplerState::no_zone_message());
            return;
        }
        let armed = !self.nav.clip_view.sampler.root_learn;
        self.nav.clip_view.sampler.root_learn = armed;
        let title = self.cursor_sampler().map(SamplerState::edit_title).unwrap_or_default();
        self.flash(if armed {
            format!("{title} \u{00b7} play the key this sound was recorded at \u{00b7} esc cancels")
        } else {
            format!("{title} \u{00b7} root learn off")
        });
    }

    /// Whether the next key played is an answer rather than a note.
    pub(crate) fn sampler_root_learn_armed(&self) -> bool {
        self.nav.clip_view.sampler.root_learn && self.cursor_sampler_zone().is_some()
    }

    /// The key that answered: it becomes the zone's root, and the arming
    /// is spent.
    pub(crate) fn learn_zone_root(&mut self, note: u8) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        self.nav.clip_view.sampler.root_learn = false;
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let moved = sampler.set_edit_root(note);
        let title = sampler.edit_title();
        // A root learned back onto itself is not a step: the commit sees
        // two identical slices and drops it, which is what keeps `u` from
        // landing on a state the player never saw.
        self.nav.commit_undo(before, "zone root");
        if moved {
            self.sync_sampler_edit(track_idx);
        }
        self.flash(format!(
            "{title} \u{00b7} root {} \u{00b7} learned from the keys",
            phosphor_app::format::note_name(note),
        ));
    }

    /// Disarm root-learn the player has walked away from.
    ///
    /// Asked once per keystroke beside the audition's own question, and for
    /// the same reason: there are more ways out of the pad map than there
    /// are keys in it — Tab to another view, the mode switched back to
    /// pads, the zone deleted, the cursor walked onto a bare key. An
    /// arming left standing would take the next note played anywhere in the
    /// box and retune a zone with it.
    pub(crate) fn reconcile_sampler_learn(&mut self) {
        if !self.nav.clip_view.sampler.root_learn {
            return;
        }
        if !self.sampler_keys_have_focus() || self.cursor_sampler_zone().is_none() {
            self.nav.clip_view.sampler.root_learn = false;
        }
    }

    // ── Shared ──

    /// Whether the keys mean zones here, saying so when they do not.
    fn in_keys_mode(&mut self) -> bool {
        if self.sampler_mode() == MapMode::Keys {
            return true;
        }
        self.flash("K puts the bed into zones \u{00b7} these keys are its own");
        false
    }

    /// What the flash adds when zones overlap deeply enough for the
    /// eight-layer bed to turn sounds away.
    ///
    /// Said at the edit that caused it rather than drawn somewhere for
    /// ever: a layer that does not sound is a surprise, and the moment to
    /// be surprised is the moment you made it happen.
    fn zone_overflow_note(&self, track_idx: usize) -> String {
        let over = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_ref())
            .map_or(0, |s| s.zone_overflow());
        if over == 0 {
            return String::new();
        }
        format!(
            " \u{00b7} {over} layer{} past eight do not sound where these zones overlap",
            if over == 1 { "" } else { "s" },
        )
    }
}
