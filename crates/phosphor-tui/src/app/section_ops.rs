//! App methods: the section clipboard — the loop brace as scissors.
//!
//! State logic lives in `phosphor_app::state::section`; this file is the
//! glue: undo capture, audio resync, the brace-slide keys, the flashes.

use super::*;

impl App {
    /// `y` in the loop editor: lift what is between the markers.
    pub(crate) fn yank_loop_section(&mut self) {
        let (start, end) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
        let section = self.nav.yank_section(start, end);
        if section.is_empty() {
            self.flash("nothing between the markers");
            return;
        }
        let notes = section.note_count();
        let tracks = section.tracks.len();
        self.nav.section_clip = Some(section);
        self.flash(format!(
            "section lifted \u{00b7} {notes} note{} on {tracks} track{} \u{00b7} p drops it at the brace",
            if notes == 1 { "" } else { "s" },
            if tracks == 1 { "" } else { "s" },
        ));
    }

    /// `x` in the loop editor: lift and remove — one undo step across
    /// every track it touches.
    pub(crate) fn cut_loop_section(&mut self) {
        let (start, end) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
        let section = self.nav.yank_section(start, end);
        if section.is_empty() {
            self.flash("nothing between the markers");
            return;
        }
        let before = self.nav.undo_checkpoint(crate::state::undo::UndoScope::Song);
        let counts: Vec<(usize, usize)> =
            self.nav.tracks.iter().enumerate().map(|(i, t)| (i, t.clips.len())).collect();
        let touched = self.nav.remove_section(start, end);
        for &track_idx in &touched {
            let audio_count =
                counts.iter().find(|(i, _)| *i == track_idx).map_or(0, |(_, c)| *c);
            self.resync_track_clips_to_audio(track_idx, audio_count);
        }
        let notes = section.note_count();
        self.nav.section_clip = Some(section);
        self.nav.commit_undo(before, "cut section");
        self.flash(format!(
            "section cut \u{00b7} {notes} note{} lifted \u{00b7} u puts them back",
            if notes == 1 { "" } else { "s" },
        ));
    }

    /// `p` in the loop editor: drop the lifted section at the brace —
    /// clearing the span it lands on, the way a tape drop does — then
    /// leapfrog the brace forward by the section's length, so p p p
    /// stamps three copies back to back. `P` layers instead of clearing.
    pub(crate) fn paste_loop_section(&mut self, replace: bool) {
        let Some(section) = self.nav.section_clip.clone() else {
            self.flash("nothing lifted \u{00b7} y or x between the markers first");
            return;
        };
        let at = self.nav.loop_editor.start;
        let before = self.nav.undo_checkpoint(crate::state::undo::UndoScope::Song);
        let counts: Vec<(usize, usize)> =
            self.nav.tracks.iter().enumerate().map(|(i, t)| (i, t.clips.len())).collect();
        let touched = self.nav.paste_section(&section, at, replace);
        if touched.is_empty() {
            self.flash("the lifted tracks are gone");
            return;
        }
        for &track_idx in &touched {
            let audio_count =
                counts.iter().find(|(i, _)| *i == track_idx).map_or(0, |(_, c)| *c);
            self.resync_track_clips_to_audio(track_idx, audio_count);
        }
        self.nav.commit_undo(before, "paste section");

        // The leapfrog: brace forward by its own cargo, ready to stamp again.
        let len = section.len_ticks;
        let (s, e) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
        self.nav.loop_editor.set_region(s + len, e + len);
        self.sync_loop_to_transport();
        self.flash(format!(
            "section {} \u{00b7} p again stamps the next",
            if replace { "stamped" } else { "layered" },
        ));
    }

    /// Slide the whole brace by one grid step (j back, k forward), or by
    /// its own length (J/K) — the brace is the cursor, this is how it
    /// walks the song.
    pub(crate) fn slide_loop_brace(&mut self, forward: bool, whole_region: bool) {
        let step = if whole_region {
            self.nav.loop_editor.len_ticks()
        } else {
            self.nav.loop_editor.step.ticks()
        };
        let delta = if forward { step } else { -step };
        let (s, e) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
        if s + delta < 0 {
            return;
        }
        let before = self.nav.undo_checkpoint(crate::state::undo::UndoScope::LoopRange);
        self.nav.loop_editor.set_region(s + delta, e + delta);
        self.sync_loop_to_transport();
        self.nav.commit_undo_coalesced(
            before,
            "move loop",
            crate::state::undo::UndoGesture::LoopRange,
        );
    }
}
