//! App methods: the loop editor — the brace as cursor and as scissors.
//!
//! State logic lives in `phosphor_app::state::{loop_editor, section}`; this
//! file is the glue: the editor's keys, undo capture, audio resync, the
//! flashes.

use super::*;
use phosphor_app::timeline::TICKS_PER_BAR;

impl App {
    /// A key while the loop editor has the keyboard.
    ///
    /// Digits build a count for the next key — `8L` stretches the end eight
    /// grid steps, `4p` stamps four copies — so a brace of any size is a few
    /// keys away, and so is any number of copies of it.
    pub(crate) fn handle_loop_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        use crate::debug_log as dbg;

        if let KeyCode::Char(c) = key.code {
            if let Some(digit) = c.to_digit(10) {
                self.nav.loop_editor.push_digit(digit);
                if let Some(n) = self.nav.loop_editor.count {
                    self.flash(format!("{n}\u{00d7} \u{00b7} the next key happens {n} times"));
                }
                return;
            }
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let times = self.nav.loop_editor.take_count();
        let repeat = |edit: fn(&mut crate::state::LoopEditor)| {
            move |l: &mut crate::state::LoopEditor| (0..times).for_each(|_| edit(l))
        };
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
            KeyCode::Char('h') | KeyCode::Left if shift => self.edit_loop_range(repeat(|l| l.move_end_left())),
            KeyCode::Char('l') | KeyCode::Right if shift => self.edit_loop_range(repeat(|l| l.move_end_right())),
            KeyCode::Char('h') | KeyCode::Left => self.edit_loop_range(repeat(|l| l.move_start_left())),
            KeyCode::Char('l') | KeyCode::Right => self.edit_loop_range(repeat(|l| l.move_start_right())),
            KeyCode::Char('H') => self.edit_loop_range(repeat(|l| l.move_end_left())),
            KeyCode::Char('L') => self.edit_loop_range(repeat(|l| l.move_end_right())),
            // The brace is the cursor: j/k walk it along the song by one grid
            // step, J/K leap it by its own length.
            KeyCode::Char('j') | KeyCode::Down => self.slide_loop_brace(true, false, times),
            KeyCode::Char('k') | KeyCode::Up => self.slide_loop_brace(false, false, times),
            KeyCode::Char('J') => self.slide_loop_brace(true, true, times),
            KeyCode::Char('K') => self.slide_loop_brace(false, true, times),
            KeyCode::Char('y') => self.yank_loop_section(),
            KeyCode::Char('x') | KeyCode::Char('d') => self.cut_loop_section(),
            KeyCode::Char('p') => self.paste_loop_section(true, times),
            KeyCode::Char('P') => self.paste_loop_section(false, times),
            KeyCode::Char('c') => self.brace_to_clip(),
            KeyCode::Char('a') => self.brace_to_song(),
            KeyCode::Char('z') => {
                let (start, end) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
                self.nav.timeline.fit(start, end);
                self.flash(format!("lanes show {}", bars_words(self.nav.timeline.bars)));
            }
            KeyCode::Char('g') => {
                self.nav.loop_editor.cycle_step();
                self.flash(format!(
                    "loop grid: {} \u{00b7} markers move by it",
                    self.nav.loop_editor.step.label()
                ));
            }
            _ => {
                dbg::user(&format!("loop editor: ignored key {:?}", key.code));
                return;
            }
        }
        dbg::system(&format!("loop range: {}", self.nav.loop_editor.display()));
    }

    /// `z`/`Z` in the tracks pane: the lanes one zoom level closer or
    /// further out, about the clip under the cursor.
    pub(crate) fn zoom_lanes(&mut self, closer: bool) {
        let anchor = self.nav.zoom_anchor();
        if closer {
            self.nav.timeline.zoom_in(anchor);
        } else {
            self.nav.timeline.zoom_out(anchor);
        }
        self.flash(format!("lanes show {}", bars_words(self.nav.timeline.bars)));
    }

    /// `c`: the brace around a clip on the track under the cursor — the one
    /// the brace starts in, else the next one after it, else the last one
    /// before. A clip of any length is one key from being lifted.
    fn brace_to_clip(&mut self) {
        let at = self.nav.loop_editor.start;
        let span = self.nav.current_track().and_then(|track| {
            let spans = track.clips.iter().map(|c| (c.start_tick, c.start_tick + c.length_ticks));
            let spans: Vec<(i64, i64)> = spans.collect();
            spans
                .iter()
                .find(|(s, e)| *s <= at && at < *e)
                .or_else(|| spans.iter().filter(|(s, _)| *s > at).min_by_key(|(s, _)| *s))
                .or_else(|| spans.iter().max_by_key(|(s, _)| *s))
                .copied()
        });
        let Some((start, end)) = span else {
            self.flash("no clip on this track \u{00b7} j/k in the tracks pane picks one");
            return;
        };
        self.edit_loop_range(|l| l.set_region(start, end));
        self.flash(format!("brace on the clip \u{00b7} {}", self.nav.loop_editor.length_words()));
    }

    /// `a`: the brace around the whole song, from the top to the end of the
    /// last clip on any track, out to its bar line.
    fn brace_to_song(&mut self) {
        let end = self
            .nav
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.start_tick + c.length_ticks)
            .max();
        let Some(end) = end.filter(|&e| e > 0) else {
            self.flash("no clips yet \u{00b7} nothing to brace");
            return;
        };
        let end = (end + TICKS_PER_BAR - 1) / TICKS_PER_BAR * TICKS_PER_BAR;
        self.edit_loop_range(|l| l.set_region(0, end));
        self.flash(format!("brace on the whole song \u{00b7} {}", self.nav.loop_editor.length_words()));
    }

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
            "{} lifted \u{00b7} {notes} note{} on {tracks} track{} \u{00b7} p drops it at the brace",
            self.nav.loop_editor.length_words(),
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
        let counts = self.clip_counts();
        let touched = self.nav.remove_section(start, end);
        self.resync_touched(&touched, &counts);
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
    /// leapfrog the brace forward by the section's length, so p p p (or
    /// `3p`) stamps three copies back to back, as one undo step. `P` layers
    /// instead of clearing.
    pub(crate) fn paste_loop_section(&mut self, replace: bool, times: u32) {
        let Some(section) = self.nav.section_clip.clone() else {
            self.flash("nothing lifted \u{00b7} y or x between the markers first");
            return;
        };
        let before = self.nav.undo_checkpoint(crate::state::undo::UndoScope::Song);
        let counts = self.clip_counts();
        let mut touched: Vec<usize> = Vec::new();
        for _ in 0..times {
            let at = self.nav.loop_editor.start;
            let landed = self.nav.paste_section(&section, at, replace);
            if landed.is_empty() {
                break;
            }
            for track_idx in landed {
                if !touched.contains(&track_idx) {
                    touched.push(track_idx);
                }
            }
            // The leapfrog: brace forward by its own cargo, ready to stamp again.
            let len = section.len_ticks;
            let (s, e) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
            self.nav.loop_editor.set_region(s + len, e + len);
        }
        if touched.is_empty() {
            self.flash("the lifted tracks are gone");
            return;
        }
        self.resync_touched(&touched, &counts);
        self.nav.commit_undo(before, "paste section");
        self.sync_loop_to_transport();
        let copies = if times == 1 { String::new() } else { format!("{times} copies ") };
        self.flash(format!(
            "section {copies}{} \u{00b7} p again stamps the next",
            if replace { "stamped" } else { "layered" },
        ));
    }

    /// Slide the whole brace by `times` grid steps (j back, k forward), or
    /// by its own length (J/K) — the brace is the cursor, this is how it
    /// walks the song.
    pub(crate) fn slide_loop_brace(&mut self, forward: bool, whole_region: bool, times: u32) {
        let step = if whole_region {
            self.nav.loop_editor.len_ticks()
        } else {
            self.nav.loop_editor.step.ticks()
        };
        let delta = if forward { step } else { -step } * i64::from(times);
        let (s, e) = (self.nav.loop_editor.start, self.nav.loop_editor.end);
        // Backwards past the top stops at the top rather than refusing.
        let delta = delta.max(-s);
        if delta == 0 {
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

    /// How many clips each track holds now — what the audio side has, for
    /// [`App::resync_track_clips_to_audio`] after an edit.
    fn clip_counts(&self) -> Vec<usize> {
        self.nav.tracks.iter().map(|t| t.clips.len()).collect()
    }

    fn resync_touched(&mut self, touched: &[usize], counts: &[usize]) {
        for &track_idx in touched {
            let audio_count = counts.get(track_idx).copied().unwrap_or(0);
            self.resync_track_clips_to_audio(track_idx, audio_count);
        }
    }
}

/// "1 bar", "16 bars".
fn bars_words(bars: i64) -> String {
    format!("{bars} bar{}", if bars == 1 { "" } else { "s" })
}
