//! MIDI clip: a sequence of timestamped MIDI events on a timeline.
//!
//! Clips are owned by the audio thread for recording and playback.
//! The UI receives read-only snapshots via a channel.

/// A single MIDI event within a clip, positioned by tick.
#[derive(Debug, Clone, Copy)]
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
        events.sort_by_key(|e| e.tick);
        Self { start_tick, length_ticks, events }
    }

    /// End tick (exclusive).
    pub fn end_tick(&self) -> i64 {
        self.start_tick + self.length_ticks
    }

    /// Get events that fall within a tick range [from, to).
    /// Returns events with tick offsets relative to `from` for sample-accurate placement.
    pub fn events_in_range(&self, from_tick: i64, to_tick: i64) -> Vec<(i64, &ClipEvent)> {
        // Convert to clip-local ticks
        let local_from = from_tick - self.start_tick;
        let local_to = to_tick - self.start_tick;

        self.events
            .iter()
            .filter(|e| e.tick >= local_from && e.tick < local_to)
            .map(|e| (e.tick - local_from, e)) // offset relative to from_tick
            .collect()
    }
}

/// Accumulates MIDI events during recording, then commits to a MidiClip.
pub struct RecordBuffer {
    start_tick: i64,
    events: Vec<ClipEvent>,
    active: bool,
}

impl RecordBuffer {
    pub fn new() -> Self {
        Self { start_tick: 0, events: Vec::with_capacity(1024), active: false }
    }

    /// Begin recording at the given tick position.
    pub fn start(&mut self, tick: i64) {
        self.start_tick = tick;
        self.events.clear();
        self.active = true;
    }

    /// Record a MIDI event at the given absolute tick.
    pub fn record(&mut self, tick: i64, status: u8, data1: u8, data2: u8) {
        if !self.active { return; }
        self.events.push(ClipEvent {
            tick: tick - self.start_tick, // store relative to clip start
            status,
            data1,
            data2,
        });
    }

    pub fn is_active(&self) -> bool { self.active }
    pub fn start_tick(&self) -> i64 { self.start_tick }

    /// Stop recording and return the completed clip.
    /// Returns None if nothing was recorded.
    pub fn commit(&mut self, end_tick: i64) -> Option<MidiClip> {
        self.active = false;
        if self.events.is_empty() {
            return None;
        }
        let length = (end_tick - self.start_tick).max(1);
        let clip = MidiClip::new(self.start_tick, length, self.events.drain(..).collect());
        Some(clip)
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
#[derive(Debug, Clone, Copy)]
pub struct NoteSnapshot {
    pub note: u8,
    pub velocity: u8,
    /// Start position as fraction of clip length (0.0..1.0).
    pub start_frac: f64,
    /// Duration as fraction of clip length.
    pub duration_frac: f64,
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
        assert_eq!(clip.length_ticks, 960);
        assert!(!buf.is_active());
    }

    #[test]
    fn record_buffer_empty_returns_none() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        assert!(buf.commit(960).is_none());
    }

    #[test]
    fn record_buffer_stores_relative_ticks() {
        let mut buf = RecordBuffer::new();
        buf.start(1000); // recording starts at tick 1000
        buf.record(1500, 0x90, 60, 100);
        let clip = buf.commit(2000).unwrap();
        assert_eq!(clip.events[0].tick, 500); // relative to start
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
}
