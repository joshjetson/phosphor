//! MIDI clip: a sequence of timestamped MIDI events on a timeline.
//!
//! Clips are owned by the audio thread for recording and playback.
//! The UI receives read-only snapshots via a channel.

/// A single MIDI event within a clip, positioned by tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipEvent {
    /// Absolute tick position within the clip (0 = clip start).
    pub tick: i64,
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

/// A recorded MIDI clip.
#[derive(Debug, Clone)]
pub struct MidiClip {
    /// Where this clip starts on the timeline (absolute ticks).
    pub start_tick: i64,
    /// Length in ticks. Events beyond this are ignored on playback.
    pub length_ticks: i64,
    /// Events sorted by tick (relative to start_tick).
    pub events: Vec<ClipEvent>,
}

impl MidiClip {
    pub fn new(start_tick: i64, length_ticks: i64, mut events: Vec<ClipEvent>) -> Self {
        // Offs before ons at the same tick — 0x80 sorts below 0x90, so the
        // status nibble is the tiebreak. Without it, a repeated note whose
        // off and re-strike share a tick can reach the instrument as
        // on, on, off, off, and the second strike is eaten.
        events.sort_by_key(|e| (e.tick, e.status & 0xF0));
        Self { start_tick, length_ticks, events }
    }

    /// End tick (exclusive).
    pub fn end_tick(&self) -> i64 {
        self.start_tick + self.length_ticks
    }

    /// Events that fall within a tick range [from, to), each paired with the
    /// absolute song tick it happens at.
    ///
    /// An iterator rather than a list, because this is called once per clip
    /// per audio callback and collecting into a `Vec` is a trip to the
    /// allocator on the audio thread. Absolute ticks rather than offsets
    /// because that is what
    /// [`crate::pattern::PlaybackWindow::sample_offset`] takes, and clips and
    /// patterns go through the same one.
    pub fn events_between(
        &self,
        from_tick: i64,
        to_tick: i64,
    ) -> impl Iterator<Item = (i64, &ClipEvent)> {
        let start = self.start_tick;
        self.events
            .iter()
            .map(move |e| (start + e.tick, e))
            .filter(move |(tick, _)| *tick >= from_tick && *tick < to_tick)
    }

    /// Get events that fall within a tick range [from, to).
    /// Returns events with tick offsets relative to `from` for sample-accurate placement.
    pub fn events_in_range(&self, from_tick: i64, to_tick: i64) -> Vec<(i64, &ClipEvent)> {
        self.events_between(from_tick, to_tick)
            .map(|(tick, e)| (tick - from_tick, e))
            .collect()
    }
}

/// Accumulates MIDI events during recording, then commits to a MidiClip.
pub struct RecordBuffer {
    start_tick: i64,
    events: Vec<ClipEvent>,
    active: bool,
}

impl Default for RecordBuffer {
    fn default() -> Self { Self::new() }
}

impl RecordBuffer {
    /// The shortest note a take keeps, in ticks — a 64th at 960 PPQ. A stab
    /// whose on and off land in the same audio block would otherwise become
    /// a one-tick note: inaudible on some patches and invisible in the
    /// roll, which reads as a note that vanished.
    pub const MIN_NOTE_TICKS: i64 = 60;

    /// Room for four thousand notes in one pass — far past human playing,
    /// since only note events land here. The buffer never grows on the
    /// audio thread: allocated once when the track is built (`AddTrack` is
    /// already charged for allocation), full means the event is dropped.
    const CAPACITY: usize = 8192;

    pub fn new() -> Self {
        Self { start_tick: 0, events: Vec::with_capacity(Self::CAPACITY), active: false }
    }

    /// Begin recording at the given tick position.
    pub fn start(&mut self, tick: i64) {
        self.start_tick = tick;
        self.events.clear();
        self.active = true;
    }

    /// Record a MIDI event at the given absolute tick.
    ///
    /// Notes only. A controller sweep or a pitch bend has no lane to live
    /// on yet: captured, it would play back until the first edit rebuilt
    /// the clip from its notes and silently erased it — data loss with no
    /// message and no undo entry that names it. Refused here, the rule is
    /// simply "what the roll shows is what the clip holds", until
    /// automation lanes exist.
    ///
    /// A note-on at velocity zero is a note-off — every controller that
    /// runs notes together sends them that way — and it is normalised to
    /// `0x80` so that everything downstream can put offs before ons by
    /// status byte alone.
    pub fn record(&mut self, tick: i64, status: u8, data1: u8, data2: u8) {
        if !self.active { return; }
        let kind = status & 0xF0;
        if kind != 0x90 && kind != 0x80 { return; }
        let (status, data2) = if kind == 0x90 && data2 == 0 { (0x80, 0) } else { (status, data2) };
        if self.events.len() >= self.events.capacity() {
            // Growing here is a heap allocation on the audio thread. The cap
            // is far past human playing; hitting it means something is
            // spraying, and dropping is the honest failure.
            return;
        }
        self.events.push(ClipEvent {
            tick: tick - self.start_tick, // store relative to clip start
            status,
            data1,
            data2,
        });
    }

    pub fn is_active(&self) -> bool { self.active }
    pub fn start_tick(&self) -> i64 { self.start_tick }

    /// How many anticipated notes a wrap can carry over — all ten fingers,
    /// with room to spare. An array, not a `Vec`: this runs on the audio
    /// thread at the loop wrap, where allocating is forbidden.
    pub const MAX_ANTICIPATED: usize = 16;

    /// Take out the note-ons still held within `window` ticks of `end_rel`
    /// — the downbeats a player anticipated. A human leads a downbeat by
    /// ten to thirty milliseconds as a matter of course, and without this
    /// the note lands at the far right edge of the *previous* pass instead
    /// of on beat one of the next. Returns the (note, velocity) pairs and
    /// how many are real, so the caller can re-strike them at the top of
    /// the new pass.
    pub fn take_anticipated(
        &mut self,
        end_rel: i64,
        window: i64,
    ) -> ([(u8, u8); Self::MAX_ANTICIPATED], usize) {
        let mut out = [(0u8, 0u8); Self::MAX_ANTICIPATED];
        let mut count = 0;
        let mut i = 0;
        while i < self.events.len() {
            let e = self.events[i];
            let is_held_on = e.status & 0xF0 == 0x90
                && e.tick >= end_rel - window
                && !self.events[i + 1..]
                    .iter()
                    .any(|o| o.status & 0xF0 == 0x80 && o.data1 == e.data1);
            if is_held_on && count < Self::MAX_ANTICIPATED {
                out[count] = (e.data1, e.data2);
                count += 1;
                self.events.remove(i);
            } else {
                i += 1;
            }
        }
        (out, count)
    }

    /// Stop recording and return the completed clip.
    /// Returns None if nothing was recorded.
    ///
    /// The commit is where a raw performance becomes a clip that keeps its
    /// promises:
    ///
    /// * **Bars.** The clip is widened to whole bars — start floored, end
    ///   ceiled. A loop take is bar-aligned already, so this only moves a
    ///   linear take recorded from mid-bar; without it the piano roll's
    ///   grid and the quantize are both stretched against a fractional
    ///   length and land every note somewhere musically wrong.
    /// * **No held note is left open.** A note still down at the wrap or at
    ///   stop gets its off written at the clip's end. The snapshot always
    ///   drew it that way; the audio clip used to keep the raw dangling on
    ///   and drone it on every playback until the next panic.
    /// * **No note shorter than a 64th**, and no doubled note: two ons of
    ///   one pitch on one tick keep the harder hit.
    pub fn commit(&mut self, end_tick: i64) -> Option<MidiClip> {
        self.active = false;
        if self.events.is_empty() {
            return None;
        }

        let bar = crate::transport::Transport::PPQ * 4;
        let snapped_start = (self.start_tick / bar) * bar;
        let shift = self.start_tick - snapped_start;
        let raw_end = shift + (end_tick - self.start_tick).max(1);
        let length = ((raw_end + bar - 1) / bar) * bar;

        // Pair the raw stream into notes: FIFO per pitch, held notes closed
        // at the clip's end.
        let mut notes: Vec<(u8, u8, i64, i64)> = Vec::new(); // note, vel, on, off
        let mut pending: Vec<(u8, u8, i64)> = Vec::new();
        for e in self.events.drain(..) {
            let tick = e.tick + shift;
            match e.status & 0xF0 {
                0x90 => pending.push((e.data1, e.data2, tick)),
                0x80 => {
                    if let Some(pos) = pending.iter().position(|(n, _, _)| *n == e.data1) {
                        let (note, vel, on) = pending.remove(pos);
                        notes.push((note, vel, on, tick));
                    }
                }
                _ => {}
            }
        }
        for (note, vel, on) in pending {
            notes.push((note, vel, on, length));
        }

        for (_, _, on, off) in &mut notes {
            *off = (*on + Self::MIN_NOTE_TICKS).max(*off).min(length).max(*on + 1);
        }

        // Same pitch, same tick: one note, the harder hit.
        notes.sort_by_key(|&(note, vel, on, _)| (on, note, std::cmp::Reverse(vel)));
        notes.dedup_by_key(|&mut (note, _, on, _)| (note, on));

        let mut events = Vec::with_capacity(notes.len() * 2);
        for (note, vel, on, off) in notes {
            events.push(ClipEvent { tick: on, status: 0x90, data1: note, data2: vel });
            events.push(ClipEvent { tick: off, status: 0x80, data1: note, data2: 0 });
        }
        Some(MidiClip::new(snapped_start, length, events))
    }

    /// Discard without committing.
    pub fn discard(&mut self) {
        self.active = false;
        self.events.clear();
    }
}

/// A read-only snapshot of clip data, sent from audio thread to UI.
#[derive(Debug, Clone)]
pub struct ClipSnapshot {
    pub track_id: usize,
    pub clip_index: usize,
    pub start_tick: i64,
    pub length_ticks: i64,
    pub event_count: usize,
    /// Simplified note data for piano roll display.
    pub notes: Vec<NoteSnapshot>,
}

/// A note for display in the piano roll.
///
/// `PartialEq` compares the fractions exactly, which is right for the one
/// place it is used: deciding whether an undo checkpoint saw any change at
/// all. Two clips that differ only in float noise were produced by different
/// edits, and an edit is a change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteSnapshot {
    pub note: u8,
    pub velocity: u8,
    /// Start position as fraction of clip length (0.0..1.0).
    pub start_frac: f64,
    /// Duration as fraction of clip length.
    pub duration_frac: f64,
}

impl NoteSnapshot {
    /// Convert edited NoteSnapshots back to ClipEvents for the audio thread.
    /// Each note produces a note-on and note-off event.
    pub fn to_clip_events(notes: &[NoteSnapshot], length_ticks: i64) -> Vec<ClipEvent> {
        let mut events = Vec::with_capacity(notes.len() * 2);
        for n in notes {
            let on_tick = (n.start_frac * length_ticks as f64) as i64;
            let off_tick = ((n.start_frac + n.duration_frac) * length_ticks as f64) as i64;
            events.push(ClipEvent {
                tick: on_tick,
                status: 0x90,
                data1: n.note,
                data2: n.velocity,
            });
            events.push(ClipEvent {
                tick: off_tick.min(length_ticks),
                status: 0x80,
                data1: n.note,
                data2: 0,
            });
        }
        // Offs before ons at the same tick — see [`MidiClip::new`].
        events.sort_by_key(|e| (e.tick, e.status & 0xF0));
        events
    }
}

impl ClipSnapshot {
    pub fn from_clip(track_id: usize, clip_index: usize, clip: &MidiClip) -> Self {
        let len = clip.length_ticks as f64;
        let mut notes = Vec::new();

        // Track note-on times to pair with note-offs
        let mut pending: Vec<(u8, u8, i64)> = Vec::new(); // (note, velocity, start_tick)

        for event in &clip.events {
            let status = event.status & 0xF0;
            match status {
                0x90 if event.data2 > 0 => {
                    pending.push((event.data1, event.data2, event.tick));
                }
                0x90 | 0x80 => {
                    // Note off — find matching pending note
                    if let Some(pos) = pending.iter().position(|(n, _, _)| *n == event.data1) {
                        let (note, vel, start) = pending.remove(pos);
                        let dur = (event.tick - start).max(1);
                        notes.push(NoteSnapshot {
                            note,
                            velocity: vel,
                            start_frac: start as f64 / len,
                            duration_frac: dur as f64 / len,
                        });
                    }
                }
                _ => {}
            }
        }

        // Close any pending notes at clip end
        for (note, vel, start) in pending {
            let dur = (clip.length_ticks - start).max(1);
            notes.push(NoteSnapshot {
                note,
                velocity: vel,
                start_frac: start as f64 / len,
                duration_frac: dur as f64 / len,
            });
        }

        Self {
            track_id,
            clip_index,
            start_tick: clip.start_tick,
            length_ticks: clip.length_ticks,
            event_count: clip.events.len(),
            notes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: i64 = crate::transport::Transport::PPQ * 4;

    #[test]
    fn record_buffer_captures_events() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 100); // note on
        buf.record(200, 0x80, 60, 0);   // note off
        assert!(buf.is_active());

        let clip = buf.commit(960).unwrap();
        assert_eq!(clip.events.len(), 2);
        assert_eq!(clip.start_tick, 0);
        // The clip is widened to whole bars, so the grid and quantize have
        // something musically true to measure against.
        assert_eq!(clip.length_ticks, BAR);
        assert!(!buf.is_active());
    }

    #[test]
    fn record_buffer_empty_returns_none() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        assert!(buf.commit(960).is_none());
    }

    /// A linear take started mid-bar commits a clip floored to the bar
    /// line, with every note keeping its absolute position.
    #[test]
    fn record_buffer_snaps_a_mid_bar_take_to_the_bar() {
        let mut buf = RecordBuffer::new();
        buf.start(1000); // recording starts mid-bar
        buf.record(1500, 0x90, 60, 100);
        buf.record(1700, 0x80, 60, 0);
        let clip = buf.commit(2000).unwrap();
        assert_eq!(clip.start_tick, 0, "the clip start was not floored to the bar");
        assert_eq!(clip.length_ticks, BAR, "the clip end was not ceiled to the bar");
        // Absolute position preserved: bar start + event tick = 1500.
        assert_eq!(clip.start_tick + clip.events[0].tick, 1500);
    }

    /// A note still held when the take commits gets its off written at the
    /// clip's end — the raw dangling on used to drone on every playback.
    #[test]
    fn commit_closes_a_note_still_held() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 100); // never released
        let clip = buf.commit(BAR).unwrap();
        let offs: Vec<_> = clip.events.iter().filter(|e| e.status == 0x80).collect();
        assert_eq!(offs.len(), 1, "the held note was left open");
        assert_eq!(offs[0].tick, BAR, "the off did not land at the clip's end");
    }

    /// A stab whose on and off land in the same block keeps a playable,
    /// visible length — never a one-tick sliver.
    #[test]
    fn commit_floors_the_shortest_note() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 100);
        buf.record(100, 0x80, 60, 0); // same block, same tick
        let clip = buf.commit(BAR).unwrap();
        let on = clip.events.iter().find(|e| e.status == 0x90).unwrap();
        let off = clip.events.iter().find(|e| e.status == 0x80).unwrap();
        assert!(
            off.tick - on.tick >= RecordBuffer::MIN_NOTE_TICKS,
            "a same-block stab became a {}-tick sliver",
            off.tick - on.tick
        );
    }

    /// Two ons of one pitch on one tick are one note — the harder hit.
    #[test]
    fn commit_keeps_one_of_a_doubled_note() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 40);
        buf.record(100, 0x90, 60, 110);
        buf.record(300, 0x80, 60, 0);
        buf.record(300, 0x80, 60, 0);
        let clip = buf.commit(BAR).unwrap();
        let ons: Vec<_> = clip.events.iter().filter(|e| e.status == 0x90).collect();
        assert_eq!(ons.len(), 1, "the doubled note survived");
        assert_eq!(ons[0].data2, 110, "the softer hit won");
    }

    /// Controller data is refused at the door: captured, it would play back
    /// until the first edit rebuilt the clip from its notes and silently
    /// erased it. Notes only, until automation lanes exist.
    #[test]
    fn record_refuses_what_the_roll_cannot_hold() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(50, 0xB0, 1, 90);  // mod wheel
        buf.record(60, 0xE0, 0, 64);  // pitch bend
        buf.record(70, 0xD0, 80, 0);  // aftertouch
        assert!(buf.commit(BAR).is_none(), "controller data was recorded");

        // A note-on at velocity zero is a note-off, normalised so offs can
        // sort before ons by status byte alone.
        buf.start(0);
        buf.record(100, 0x90, 60, 100);
        buf.record(200, 0x90, 60, 0); // running-status note-off
        let clip = buf.commit(BAR).unwrap();
        assert_eq!(
            clip.events.iter().filter(|e| e.status == 0x80).count(),
            1,
            "the velocity-zero off was not honoured"
        );
        let off = clip.events.iter().find(|e| e.status == 0x80).unwrap();
        assert_eq!(off.tick, 200, "the off drifted from where it was played");
    }

    /// A downbeat played a hair before the wrap comes out of the old pass
    /// and re-strikes at the top of the new one.
    #[test]
    fn anticipated_downbeats_move_to_the_next_pass() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 100);   // an ordinary note...
        buf.record(300, 0x80, 60, 0);     // ...released long before the wrap
        buf.record(BAR - 50, 0x90, 72, 90); // the anticipated downbeat, still held
        let (carried, count) = buf.take_anticipated(BAR, crate::transport::Transport::PPQ / 8);
        assert_eq!(count, 1, "the anticipated note was not carried");
        assert_eq!(carried[0], (72, 90));

        let clip = buf.commit(BAR).unwrap();
        assert!(
            clip.events.iter().all(|e| e.data1 != 72),
            "the carried note also stayed in the old pass"
        );
        assert_eq!(
            clip.events.iter().filter(|e| e.data1 == 60).count(),
            2,
            "the ordinary note was disturbed"
        );
    }

    #[test]
    fn clip_events_in_range() {
        let clip = MidiClip::new(0, 960, vec![
            ClipEvent { tick: 0,   status: 0x90, data1: 60, data2: 100 },
            ClipEvent { tick: 240, status: 0x80, data1: 60, data2: 0 },
            ClipEvent { tick: 480, status: 0x90, data1: 64, data2: 100 },
            ClipEvent { tick: 720, status: 0x80, data1: 64, data2: 0 },
        ]);

        // First quarter
        let events = clip.events_in_range(0, 240);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.data1, 60); // note 60

        // Second quarter
        let events = clip.events_in_range(240, 480);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.status, 0x80); // note off

        // Full clip
        let events = clip.events_in_range(0, 960);
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn clip_events_outside_range_excluded() {
        let clip = MidiClip::new(1000, 960, vec![
            ClipEvent { tick: 100, status: 0x90, data1: 60, data2: 100 },
        ]);

        // Before clip
        let events = clip.events_in_range(0, 500);
        assert_eq!(events.len(), 0);

        // During clip (tick 1100 = local tick 100)
        let events = clip.events_in_range(1000, 1200);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn clip_snapshot_pairs_notes() {
        let clip = MidiClip::new(0, 960, vec![
            ClipEvent { tick: 0,   status: 0x90, data1: 60, data2: 100 },
            ClipEvent { tick: 240, status: 0x80, data1: 60, data2: 0 },
            ClipEvent { tick: 480, status: 0x90, data1: 64, data2: 80 },
            ClipEvent { tick: 720, status: 0x80, data1: 64, data2: 0 },
        ]);

        let snap = ClipSnapshot::from_clip(0, 0, &clip);
        assert_eq!(snap.notes.len(), 2);
        assert_eq!(snap.notes[0].note, 60);
        assert!((snap.notes[0].start_frac - 0.0).abs() < 0.01);
        assert!((snap.notes[0].duration_frac - 0.25).abs() < 0.01);
        assert_eq!(snap.notes[1].note, 64);
    }

    #[test]
    fn clip_snapshot_closes_pending_notes() {
        let clip = MidiClip::new(0, 960, vec![
            ClipEvent { tick: 0, status: 0x90, data1: 60, data2: 100 },
            // No note-off — should close at clip end
        ]);

        let snap = ClipSnapshot::from_clip(0, 0, &clip);
        assert_eq!(snap.notes.len(), 1);
        assert!((snap.notes[0].duration_frac - 1.0).abs() < 0.01);
    }

    #[test]
    fn discard_clears_buffer() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 100);
        buf.discard();
        assert!(!buf.is_active());
        assert!(buf.commit(960).is_none());
    }

    #[test]
    fn note_snapshot_to_clip_events_round_trip() {
        // Record a clip
        let clip = MidiClip::new(0, 960, vec![
            ClipEvent { tick: 0,   status: 0x90, data1: 60, data2: 100 },
            ClipEvent { tick: 240, status: 0x80, data1: 60, data2: 0 },
            ClipEvent { tick: 480, status: 0x90, data1: 64, data2: 80 },
            ClipEvent { tick: 720, status: 0x80, data1: 64, data2: 0 },
        ]);

        // Convert to snapshots (like the UI receives)
        let snap = ClipSnapshot::from_clip(0, 0, &clip);
        assert_eq!(snap.notes.len(), 2);

        // Convert back to events (like the UI sends after editing)
        let events = NoteSnapshot::to_clip_events(&snap.notes, 960);
        assert_eq!(events.len(), 4); // 2 notes × 2 events each

        // Verify the events are correct
        let note_ons: Vec<_> = events.iter().filter(|e| e.status == 0x90).collect();
        let note_offs: Vec<_> = events.iter().filter(|e| e.status == 0x80).collect();
        assert_eq!(note_ons.len(), 2);
        assert_eq!(note_offs.len(), 2);

        // First note: tick 0, note 60
        assert_eq!(note_ons[0].data1, 60);
        assert_eq!(note_ons[0].tick, 0);
        // Second note: tick ~480, note 64
        assert_eq!(note_ons[1].data1, 64);
        assert!((note_ons[1].tick - 480).abs() <= 1);
    }

    #[test]
    fn edited_snapshot_produces_different_events() {
        let mut notes = vec![
            NoteSnapshot { note: 60, velocity: 100, start_frac: 0.0, duration_frac: 0.25 },
        ];

        let original = NoteSnapshot::to_clip_events(&notes, 960);
        assert_eq!(original[0].tick, 0); // note on at tick 0

        // Edit: move start to 0.5
        notes[0].start_frac = 0.5;
        let edited = NoteSnapshot::to_clip_events(&notes, 960);
        assert_eq!(edited[0].tick, 480); // note on at tick 480 now

        // Edit: make it shorter
        notes[0].duration_frac = 0.1;
        let shorter = NoteSnapshot::to_clip_events(&notes, 960);
        let off_tick = shorter.iter().find(|e| e.status == 0x80).unwrap().tick;
        assert_eq!(off_tick, 576); // 480 + 96 = 576
    }
}
