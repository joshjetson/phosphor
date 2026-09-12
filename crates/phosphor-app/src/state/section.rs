//! The section clipboard: the loop brace as a pair of scissors.
//!
//! `y` in the loop editor lifts what is between the markers — every
//! instrument track at once, notes and recorded controllers alike — and
//! `p` drops it at the brace, wherever the brace has been moved to since.
//! `x` lifts and removes. The grammar is the app's own yank/paste,
//! and it maps one-to-one onto a future hardware COPY button: the brace
//! is both the selection and the destination cursor, so the whole gesture
//! is "mark it, move, stamp it" — and stamping twice lays two copies,
//! because the paste leapfrogs the brace forward by its own length.
//!
//! Notes belong to the section when their **onset** is inside it — the
//! same rule the recorder lives by. A note that starts inside and rings
//! past the edge travels whole; a note that started before the brace is
//! not this section's to take.

use phosphor_core::clip::{ClipEvent, NoteSnapshot};

use super::{Clip, NavState};

/// One track's share of a lifted section. Ticks are relative to the
/// section's start.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionTrack {
    pub track_idx: usize,
    pub notes: Vec<NoteSnapshot>,
    pub controls: Vec<ClipEvent>,
}

/// A lifted section: its length, every track's contents, and the set of
/// tracks that were in scope when it was lifted. The scope matters to the
/// replace paste: a track that was silent inside the brace pastes its
/// silence — the section is a slab of time, not a sparse note bag.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionClipboard {
    pub len_ticks: i64,
    pub tracks: Vec<SectionTrack>,
    pub scope: Vec<usize>,
}

impl SectionClipboard {
    /// Total notes held, for the flash line.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.tracks.iter().map(|t| t.notes.len()).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.iter().all(|t| t.notes.is_empty() && t.controls.is_empty())
    }
}

impl NavState {
    /// Lift the contents of `[start, end)` from every instrument track.
    #[must_use]
    pub fn yank_section(&self, start: i64, end: i64) -> SectionClipboard {
        let mut tracks = Vec::new();
        for (track_idx, track) in self.tracks.iter().enumerate() {
            if track.mixer_id.is_none() {
                continue;
            }
            let mut notes = Vec::new();
            let mut controls = Vec::new();
            for clip in &track.clips {
                for n in &clip.notes {
                    let abs = clip.start_tick + n.start_tick;
                    if abs >= start && abs < end {
                        let mut n = *n;
                        n.start_tick = abs - start;
                        notes.push(n);
                    }
                }
                for e in &clip.controls {
                    let abs = clip.start_tick + e.tick;
                    if abs >= start && abs < end {
                        let mut e = *e;
                        e.tick = abs - start;
                        controls.push(e);
                    }
                }
            }
            if !notes.is_empty() || !controls.is_empty() {
                tracks.push(SectionTrack { track_idx, notes, controls });
            }
        }
        let scope = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.mixer_id.is_some())
            .map(|(i, _)| i)
            .collect();
        SectionClipboard { len_ticks: (end - start).max(1), tracks, scope }
    }

    /// Remove the contents of `[start, end)` from every instrument track,
    /// by the same onset rule the yank uses. Clips stay where they are —
    /// an emptied clip is still the container the player laid down.
    /// Returns the indices of the tracks that changed.
    pub fn remove_section(&mut self, start: i64, end: i64) -> Vec<usize> {
        let indices: Vec<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.mixer_id.is_some())
            .map(|(i, _)| i)
            .collect();
        indices
            .into_iter()
            .filter(|&i| self.remove_section_on(i, start, end))
            .collect()
    }

    /// Drop a lifted section at `at`. Returns the tracks that changed.
    ///
    /// `replace` is the field's consensus default — the OP-1's drop, the
    /// Octatrack's paste, Reaper's razor all clear the span they land on,
    /// and only the span: what was under the stamp goes, what is beside
    /// it stays. A replace clears every track the lift had in scope, so a
    /// track that was silent inside the brace stamps its silence — the
    /// section is a slab of time. The merge variant (`P`) layers instead,
    /// the MPC MERGE / Renoise mix-paste. Either way, spans that overlap
    /// existing clips are absorbed into one union clip, keeping the
    /// clips-never-overlap invariant the whole timeline leans on.
    pub fn paste_section(&mut self, section: &SectionClipboard, at: i64, replace: bool) -> Vec<usize> {
        let mut touched = Vec::new();
        if replace {
            for &track_idx in &section.scope {
                if self.tracks.get(track_idx).is_some_and(|t| t.mixer_id.is_some()) {
                    let cleared = self
                        .remove_section_on(track_idx, at, at + section.len_ticks);
                    if cleared && !touched.contains(&track_idx) {
                        touched.push(track_idx);
                    }
                }
            }
        }
        for part in &section.tracks {
            let Some(track) = self.tracks.get_mut(part.track_idx) else { continue };
            if track.mixer_id.is_none() {
                continue;
            }
            // The pasted span must cover every note it carries, tail
            // included — a note that rings past the section edge needs a
            // clip long enough to hold it.
            let content_end = part
                .notes
                .iter()
                .map(NoteSnapshot::end_tick)
                .chain(part.controls.iter().map(|e| e.tick + 1))
                .max()
                .unwrap_or(section.len_ticks)
                .max(section.len_ticks);
            let span = (at, at + content_end);

            // Union with everything the span touches.
            let mut union_start = span.0;
            let mut union_end = span.1;
            let mut absorbed_notes: Vec<NoteSnapshot> = Vec::new();
            let mut absorbed_hidden: Vec<(i64, i64, u8, u8)> = Vec::new();
            let mut absorbed_controls: Vec<ClipEvent> = Vec::new();
            let overlaps = |c: &Clip, s: i64, e: i64| c.start_tick < e && c.start_tick + c.length_ticks > s;
            for c in track.clips.iter() {
                if overlaps(c, span.0, span.1) {
                    union_start = union_start.min(c.start_tick);
                    union_end = union_end.max(c.start_tick + c.length_ticks);
                }
            }
            track.clips.retain(|c| {
                if !overlaps(c, span.0, span.1) {
                    return true;
                }
                for n in &c.notes {
                    let mut n = *n;
                    n.start_tick += c.start_tick - union_start;
                    absorbed_notes.push(n);
                }
                for &(t, d, note, vel) in &c.hidden_notes {
                    absorbed_hidden.push((t + c.start_tick - union_start, d, note, vel));
                }
                for e in &c.controls {
                    let mut e = *e;
                    e.tick += c.start_tick - union_start;
                    absorbed_controls.push(e);
                }
                false
            });

            // The pasted material, in union coordinates.
            for n in &part.notes {
                let mut n = *n;
                n.start_tick += at - union_start;
                absorbed_notes.push(n);
            }
            for e in &part.controls {
                let mut e = *e;
                e.tick += at - union_start;
                absorbed_controls.push(e);
            }

            let length = union_end - union_start;
            let ppq = phosphor_core::transport::Transport::PPQ;
            let beats = ((length as f64) / ppq as f64).ceil() as u16;
            track.clips.push(Clip {
                number: track.clips.len() + 1,
                width: beats.max(1),
                has_content: true,
                start_tick: union_start,
                length_ticks: length,
                notes: absorbed_notes,
                hidden_notes: absorbed_hidden,
                controls: absorbed_controls,
            });
            track.clips.sort_by_key(|c| c.start_tick);
            if !touched.contains(&part.track_idx) {
                touched.push(part.track_idx);
            }
        }
        touched
    }

    /// [`Self::remove_section`]'s single-track half. Returns whether
    /// anything went.
    fn remove_section_on(&mut self, track_idx: usize, start: i64, end: i64) -> bool {
        let Some(track) = self.tracks.get_mut(track_idx) else { return false };
        let mut changed = false;
        for clip in &mut track.clips {
            let before = clip.notes.len() + clip.controls.len();
            clip.notes.retain(|n| {
                let abs = clip.start_tick + n.start_tick;
                abs < start || abs >= end
            });
            clip.controls.retain(|e| {
                let abs = clip.start_tick + e.tick;
                abs < start || abs >= end
            });
            if clip.notes.len() + clip.controls.len() != before {
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_core::transport::Transport;

    const BAR: i64 = Transport::PPQ * 4;

    fn nav_with_two_tracks() -> NavState {
        let mut nav = NavState::new(Vec::new());
        for k in 0..2usize {
            let mut track = crate::state::TrackState::new(
                "t",
                0,
                false,
                phosphor_core::project::TrackKind::Instrument,
                vec![],
            );
            track.mixer_id = Some(k);
            nav.tracks.push(track);
        }
        nav
    }

    fn clip(start: i64, len: i64, notes: Vec<NoteSnapshot>) -> Clip {
        Clip {
            number: 1,
            width: 4,
            has_content: true,
            start_tick: start,
            length_ticks: len,
            notes,
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        }
    }

    fn note(pitch: u8, start_tick: i64) -> NoteSnapshot {
        NoteSnapshot { note: pitch, velocity: 100, start_tick, duration_ticks: 240, muted: false }
    }

    /// The onset rule: a note starting inside travels (tail and all), a
    /// note starting before the brace stays behind.
    #[test]
    fn yank_takes_onsets_not_tails() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR * 2, vec![
            // Starts before the section, rings into it: stays.
            NoteSnapshot { note: 48, velocity: 100, start_tick: 0, duration_ticks: BAR, muted: false },
            // Starts inside: travels, whole.
            NoteSnapshot { note: 60, velocity: 100, start_tick: BAR / 2, duration_ticks: BAR, muted: false },
        ]));
        let s = nav.yank_section(BAR / 4, BAR);
        assert_eq!(s.note_count(), 1);
        assert_eq!(s.tracks[0].notes[0].note, 60);
        assert_eq!(s.tracks[0].notes[0].start_tick, BAR / 2 - BAR / 4, "not re-based");
        assert_eq!(s.tracks[0].notes[0].duration_ticks, BAR, "the tail was clipped");
    }

    /// Yank reaches across every instrument track at once.
    #[test]
    fn yank_spans_the_tracks() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 0)]));
        nav.tracks[1].clips.push(clip(0, BAR, vec![note(36, 480)]));
        let s = nav.yank_section(0, BAR);
        assert_eq!(s.tracks.len(), 2);
        assert_eq!(s.note_count(), 2);
    }

    /// Remove takes exactly what yank would have taken, and the clips —
    /// containers, not contents — stay.
    #[test]
    fn remove_mirrors_yank() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR * 2, vec![note(60, 0), note(64, BAR), note(67, BAR + 480)]));
        let touched = nav.remove_section(BAR, BAR * 2);
        assert_eq!(touched, vec![0]);
        let c = &nav.tracks[0].clips[0];
        assert_eq!(c.notes.len(), 1);
        assert_eq!(c.notes[0].note, 60);
        assert_eq!(nav.tracks[0].clips.len(), 1, "the container went with the contents");
    }

    /// Paste into empty space builds one clip holding the section.
    #[test]
    fn paste_into_silence_builds_a_clip() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 240)]));
        let s = nav.yank_section(0, BAR);
        let touched = nav.paste_section(&s, BAR * 4, false);
        assert_eq!(touched, vec![0]);
        let pasted = nav.tracks[0].clips.iter().find(|c| c.start_tick == BAR * 4).expect("no clip landed");
        assert_eq!(pasted.notes.len(), 1);
        assert_eq!(pasted.notes[0].start_tick, 240, "the note moved inside the section");
        assert_eq!(pasted.length_ticks, BAR);
    }

    /// Paste onto existing material merges into one clip — the timeline's
    /// no-overlap invariant survives any paste.
    #[test]
    fn paste_onto_material_merges_and_never_overlaps() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 0)]));
        nav.tracks[0].clips.push(clip(BAR * 2, BAR, vec![note(72, 0)]));
        let s = nav.yank_section(0, BAR);
        // Land it half over the second clip.
        let _ = nav.paste_section(&s, BAR * 2 - BAR / 2, false);
        let clips = &nav.tracks[0].clips;
        for (i, a) in clips.iter().enumerate() {
            for b in clips.iter().skip(i + 1) {
                assert!(
                    a.start_tick + a.length_ticks <= b.start_tick
                        || b.start_tick + b.length_ticks <= a.start_tick,
                    "paste left overlapping clips: {clips:?}"
                );
            }
        }
        // Both the old note and the pasted one live in the union.
        let union = clips.iter().find(|c| c.start_tick == BAR * 2 - BAR / 2 || c.start_tick == BAR * 2).map_or_else(
            || clips.last().unwrap(),
            |c| c,
        );
        let pitches: Vec<u8> = union.notes.iter().map(|n| n.note).collect();
        assert!(pitches.contains(&72), "the old note was lost in the merge");
        assert!(pitches.contains(&60), "the pasted note is missing");
    }

    /// A sub-bar section pastes at its exact length — a quarter of a bar
    /// lifted is a quarter of a bar dropped.
    #[test]
    fn a_quarter_bar_section_travels_at_size() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 0), note(62, 240)]));
        let s = nav.yank_section(0, BAR / 4);
        assert_eq!(s.len_ticks, BAR / 4);
        assert_eq!(s.note_count(), 2);
        let _ = nav.paste_section(&s, BAR * 2, false);
        let pasted = nav.tracks[0].clips.iter().find(|c| c.start_tick == BAR * 2).unwrap();
        // The clip is long enough for the note tails, and no longer.
        assert!(pasted.length_ticks >= 240 + 240);
        assert!(pasted.length_ticks <= BAR / 2, "a quarter-bar section pasted fat: {}", pasted.length_ticks);
    }

    /// Pasting a note whose tail outruns the section end grows the clip
    /// to hold it rather than truncating the note.
    #[test]
    fn a_ringing_tail_gets_room() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR * 2, vec![NoteSnapshot {
            note: 60, velocity: 100, start_tick: 100, duration_ticks: BAR, muted: false,
        }]));
        let s = nav.yank_section(0, BAR / 4);
        let _ = nav.paste_section(&s, BAR * 4, false);
        let pasted = nav.tracks[0].clips.iter().find(|c| c.start_tick == BAR * 4).unwrap();
        assert!(
            pasted.length_ticks >= 100 + BAR,
            "the clip cannot hold the tail: {}",
            pasted.length_ticks
        );
    }

    /// The consensus default: p clears the span it lands on — and only
    /// the span — before laying the section. What was beside the stamp
    /// stays untouched.
    #[test]
    fn replace_paste_clears_only_the_stamped_span() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 240)]));
        // Standing material at the target: one note inside the span, one
        // just past it.
        nav.tracks[0].clips.push(clip(BAR * 4, BAR * 2, vec![note(72, 100), note(74, BAR + 100)]));
        let s = nav.yank_section(0, BAR);
        let _ = nav.paste_section(&s, BAR * 4, true);
        let all: Vec<(u8, i64)> = nav.tracks[0]
            .clips
            .iter()
            .flat_map(|c| c.notes.iter().map(move |n| (n.note, c.start_tick + n.start_tick)))
            .collect();
        assert!(!all.iter().any(|&(n, _)| n == 72), "the stamp did not clear under itself: {all:?}");
        assert!(all.iter().any(|&(n, _)| n == 74), "the stamp cleared past its own end: {all:?}");
        assert!(all.iter().any(|&(n, t)| n == 60 && t == BAR * 4 + 240), "the section did not land");
    }

    /// A track that was silent inside the brace stamps its silence: the
    /// replace clears every track the lift had in scope, not just the
    /// ones that carried notes.
    #[test]
    fn a_silent_track_pastes_its_silence() {
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 0)]));
        // Track 1 is silent in the lifted bar, but has material at the target.
        nav.tracks[1].clips.push(clip(BAR * 4, BAR, vec![note(36, 0)]));
        let s = nav.yank_section(0, BAR);
        let _ = nav.paste_section(&s, BAR * 4, true);
        assert!(
            nav.tracks[1].clips.iter().all(|c| c.notes.is_empty()),
            "the silent half of the section did not overwrite: the slab has a hole"
        );

        // The merge variant leaves it alone.
        let mut nav = nav_with_two_tracks();
        nav.tracks[0].clips.push(clip(0, BAR, vec![note(60, 0)]));
        nav.tracks[1].clips.push(clip(BAR * 4, BAR, vec![note(36, 0)]));
        let s = nav.yank_section(0, BAR);
        let _ = nav.paste_section(&s, BAR * 4, false);
        assert!(
            nav.tracks[1].clips.iter().any(|c| !c.notes.is_empty()),
            "the merge paste erased a track it carried nothing for"
        );
    }
}
