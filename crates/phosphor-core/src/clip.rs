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

/// Which controller stream an event belongs to, for thinning: a control
/// change is one stream per controller number, while pitch bend and channel
/// pressure carry their value in the data bytes and are one stream each.
fn control_stream(e: &ClipEvent) -> (u8, u8) {
    match e.status & 0xF0 {
        0xB0 => (0xB0, e.data1),
        other => (other, 0),
    }
}

/// Where an event sorts among its neighbours on the same tick: note-offs
/// first, then controllers — a mod value or a bend is *state*, set before
/// the strike that should hear it — then note-ons. Without the off-first
/// rule, a repeated note whose off and re-strike share a tick reaches the
/// instrument as on, on, off, off, and the second strike is eaten.
pub fn same_tick_order(status: u8) -> u8 {
    match status & 0xF0 {
        0x80 => 0,
        0x90 => 2,
        _ => 1,
    }
}

impl MidiClip {
    pub fn new(start_tick: i64, length_ticks: i64, mut events: Vec<ClipEvent>) -> Self {
        events.sort_by_key(|e| (e.tick, same_tick_order(e.status)));
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

    /// Room for one pass of hard playing *and* a continuous controller
    /// sweep — a held mod-wheel ride lands one event per audio block. The
    /// buffer never grows on the audio thread: allocated once when the
    /// track is built (`AddTrack` is already charged for allocation), full
    /// means the event is dropped.
    const CAPACITY: usize = 16384;

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
    /// Notes, and the three performance controllers an instrument in the
    /// rack can hear — control change, pitch bend, channel pressure. The
    /// controllers travel with the clip from here on: through the snapshot,
    /// through every edit, into the session file. Anything else has no
    /// destination and is refused at the door.
    ///
    /// A note-on at velocity zero is a note-off — every controller that
    /// runs notes together sends them that way — and it is normalised to
    /// `0x80` so that everything downstream can put offs before ons by
    /// status byte alone.
    pub fn record(&mut self, tick: i64, status: u8, data1: u8, data2: u8) {
        if !self.active { return; }
        let kind = status & 0xF0;
        if !matches!(kind, 0x90 | 0x80 | 0xB0 | 0xE0 | 0xD0) { return; }
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
        // at the clip's end. Controllers ride alongside, untouched but for
        // the shift onto the bar-snapped timeline.
        let mut notes: Vec<(u8, u8, i64, i64)> = Vec::new(); // note, vel, on, off
        let mut pending: Vec<(u8, u8, i64)> = Vec::new();
        let mut controls: Vec<ClipEvent> = Vec::new();
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
                _ => controls.push(ClipEvent { tick, ..e }),
            }
        }
        for (note, vel, on) in pending {
            notes.push((note, vel, on, length));
        }

        // The sustain pedal becomes note length. A pianist's take reads
        // staccato and sounds sustained if the pedal is left as data the
        // roll cannot show; resolving it at commit makes the notes say what
        // the performance meant. Every release that lands while the pedal
        // is down slides to the next pedal-up — or to the clip's end — and
        // the pedal events themselves are consumed.
        let mut pedal: Vec<(i64, bool)> = controls
            .iter()
            .filter(|e| e.status & 0xF0 == 0xB0 && e.data1 == 64)
            .map(|e| (e.tick, e.data2 >= 64))
            .collect();
        if !pedal.is_empty() {
            pedal.sort_by_key(|&(tick, _)| tick);
            let down_at = |t: i64| -> bool {
                pedal.iter().rev().find(|&&(pt, _)| pt <= t).map(|&(_, d)| d).unwrap_or(false)
            };
            let next_up_after = |t: i64| -> i64 {
                pedal
                    .iter()
                    .find(|&&(pt, d)| pt > t && !d)
                    .map(|&(pt, _)| pt)
                    .unwrap_or(length)
            };
            for (_, _, _, off) in &mut notes {
                if *off < length && down_at(*off) {
                    *off = next_up_after(*off);
                }
            }
            controls.retain(|e| !(e.status & 0xF0 == 0xB0 && e.data1 == 64));
        }

        for (_, _, on, off) in &mut notes {
            *off = (*on + Self::MIN_NOTE_TICKS).max(*off).min(length).max(*on + 1);
        }

        // Same pitch, same tick: one note, the harder hit.
        notes.sort_by_key(|&(note, vel, on, _)| (on, note, std::cmp::Reverse(vel)));
        notes.dedup_by_key(|&mut (note, _, on, _)| (note, on));

        // Same controller, same tick: the last word wins — a wheel that
        // moved twice inside one block is at its final value. Reversed
        // before the stable sort so that within a tick the newest event
        // leads, which is the one dedup keeps.
        controls.reverse();
        controls.sort_by_key(|e| (control_stream(e), e.tick));
        controls.dedup_by(|a, b| control_stream(a) == control_stream(b) && a.tick == b.tick);

        let mut events = Vec::with_capacity(notes.len() * 2 + controls.len());
        for (note, vel, on, off) in notes {
            events.push(ClipEvent { tick: on, status: 0x90, data1: note, data2: vel });
            events.push(ClipEvent { tick: off, status: 0x80, data1: note, data2: 0 });
        }
        events.extend(controls);
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
    /// The performance controllers — control change, pitch bend, channel
    /// pressure — in ticks from the clip's start. They travel with the clip
    /// through every edit, which is what keeps a recorded wheel sweep alive
    /// past the first note deleted after it.
    pub controls: Vec<ClipEvent>,
}

/// A note in a clip, in ticks from the clip's start — the same ruler the
/// controls use, and the same ruler the audio thread plays from. The screen
/// converts to fractions at the moment of drawing and nowhere else.
///
/// `PartialEq` is exact, which is right for the one place it is used:
/// deciding whether an undo checkpoint saw any change at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteSnapshot {
    pub note: u8,
    pub velocity: u8,
    /// Ticks from the clip's start.
    pub start_tick: i64,
    /// Length in ticks. Never below 1.
    pub duration_ticks: i64,
    /// A muted note stays in the clip — visible, editable, selectable — but
    /// produces no events for the audio thread. The audition-an-edit flag.
    pub muted: bool,
}

impl NoteSnapshot {
    /// Where the note starts as a fraction of the clip — the screen's ruler.
    #[must_use]
    pub fn start_frac(&self, length_ticks: i64) -> f64 {
        self.start_tick as f64 / length_ticks.max(1) as f64
    }

    /// The note's length as a fraction of the clip — the screen's ruler.
    #[must_use]
    pub fn duration_frac(&self, length_ticks: i64) -> f64 {
        self.duration_ticks as f64 / length_ticks.max(1) as f64
    }

    /// The tick just past the note's end.
    #[must_use]
    pub fn end_tick(&self) -> i64 {
        self.start_tick + self.duration_ticks
    }

    /// Convert edited NoteSnapshots back to ClipEvents for the audio thread.
    /// Each note produces a note-on and note-off event.
    pub fn to_clip_events(notes: &[NoteSnapshot], length_ticks: i64) -> Vec<ClipEvent> {
        let mut events = Vec::with_capacity(notes.len() * 2);
        for n in notes {
            if n.muted {
                continue;
            }
            let on_tick = n.start_tick;
            let off_tick = n.end_tick();
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
        // Offs before ons at the same tick — see [`same_tick_order`].
        events.sort_by_key(|e| (e.tick, same_tick_order(e.status)));
        events
    }
}

impl ClipSnapshot {
    pub fn from_clip(track_id: usize, clip_index: usize, clip: &MidiClip) -> Self {
        let mut notes = Vec::new();

        // Track note-on times to pair with note-offs
        let mut pending: Vec<(u8, u8, i64)> = Vec::new(); // (note, velocity, start_tick)
        let mut controls = Vec::new();

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
                            start_tick: start,
                            duration_ticks: dur,
                            muted: false,
                        });
                    }
                }
                _ => controls.push(*event),
            }
        }

        // Close any pending notes at clip end
        for (note, vel, start) in pending {
            let dur = (clip.length_ticks - start).max(1);
            notes.push(NoteSnapshot {
                note,
                velocity: vel,
                start_tick: start,
                duration_ticks: dur,
                muted: false,
            });
        }

        Self {
            track_id,
            clip_index,
            start_tick: clip.start_tick,
            length_ticks: clip.length_ticks,
            event_count: clip.events.len(),
            notes,
            controls,
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

    /// The performance controllers are captured with the notes; anything
    /// with no destination in the rack is refused at the door.
    #[test]
    fn record_captures_controllers_and_refuses_the_rest() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(50, 0xB0, 1, 90);   // mod wheel
        buf.record(60, 0xE0, 0, 64);   // pitch bend
        buf.record(70, 0xD0, 80, 0);   // aftertouch
        buf.record(80, 0xA0, 60, 90);  // poly pressure — nothing plays it
        buf.record(90, 0xC0, 5, 0);    // program change — nothing plays it
        let clip = buf.commit(BAR).unwrap();
        let kinds: Vec<u8> = clip.events.iter().map(|e| e.status & 0xF0).collect();
        assert!(kinds.contains(&0xB0), "the mod wheel was lost");
        assert!(kinds.contains(&0xE0), "the pitch bend was lost");
        assert!(kinds.contains(&0xD0), "the aftertouch was lost");
        assert!(!kinds.contains(&0xA0) && !kinds.contains(&0xC0), "an event with no destination was kept");

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

    /// A wheel that moved twice inside one block keeps its final value;
    /// separate controller streams never thin each other.
    #[test]
    fn commit_thins_a_controller_to_its_last_word() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0xB0, 1, 40);
        buf.record(100, 0xB0, 1, 90);  // same tick, later word
        buf.record(100, 0xB0, 7, 120); // a different controller, same tick
        let clip = buf.commit(BAR).unwrap();
        let mod_wheel: Vec<_> = clip
            .events
            .iter()
            .filter(|e| e.status & 0xF0 == 0xB0 && e.data1 == 1)
            .collect();
        assert_eq!(mod_wheel.len(), 1, "the doubled wheel value survived");
        assert_eq!(mod_wheel[0].data2, 90, "the earlier word won");
        assert!(
            clip.events.iter().any(|e| e.status & 0xF0 == 0xB0 && e.data1 == 7),
            "a different controller was thinned away"
        );
    }

    /// The snapshot hands the controllers to the UI alongside the notes.
    #[test]
    fn snapshot_carries_the_controllers() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(100, 0x90, 60, 100);
        buf.record(300, 0x80, 60, 0);
        buf.record(200, 0xB0, 1, 75);
        let clip = buf.commit(BAR).unwrap();
        let snap = ClipSnapshot::from_clip(0, 0, &clip);
        assert_eq!(snap.notes.len(), 1);
        assert_eq!(snap.controls.len(), 1, "the controller missed the snapshot");
        assert_eq!(snap.controls[0].tick, 200);
        assert_eq!(snap.controls[0].data2, 75);
    }

    /// The sustain pedal becomes note length at commit: a release under the
    /// pedal slides to the pedal-up, the pedal events are consumed, and a
    /// note released after the pedal lifted keeps its own timing.
    #[test]
    fn sustain_pedal_resolves_into_note_lengths() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(0, 0xB0, 64, 127);      // pedal down
        buf.record(100, 0x90, 60, 100);    // note on
        buf.record(300, 0x80, 60, 0);      // released under the pedal
        buf.record(2000, 0xB0, 64, 0);     // pedal up
        buf.record(2500, 0x90, 62, 90);    // second note, pedal already up
        buf.record(2700, 0x80, 62, 0);     // its own release stands
        let clip = buf.commit(BAR).unwrap();

        let off_60 = clip.events.iter().find(|e| e.status == 0x80 && e.data1 == 60).unwrap();
        assert_eq!(off_60.tick, 2000, "the pedalled release did not slide to the pedal-up");
        let off_62 = clip.events.iter().find(|e| e.status == 0x80 && e.data1 == 62).unwrap();
        assert_eq!(off_62.tick, 2700, "an unpedalled release was moved");
        assert!(
            !clip.events.iter().any(|e| e.status & 0xF0 == 0xB0 && e.data1 == 64),
            "the consumed pedal events were kept as data"
        );
    }

    /// A note held under the pedal into the end of the take rings to the
    /// clip's end, exactly like a note held by hand.
    #[test]
    fn sustain_holds_to_the_end_when_never_lifted() {
        let mut buf = RecordBuffer::new();
        buf.start(0);
        buf.record(0, 0xB0, 64, 127);
        buf.record(100, 0x90, 60, 100);
        buf.record(200, 0x80, 60, 0);
        // pedal never lifts
        let clip = buf.commit(BAR).unwrap();
        let off = clip.events.iter().find(|e| e.status == 0x80 && e.data1 == 60).unwrap();
        assert_eq!(off.tick, BAR, "the never-lifted pedal did not hold to the end");
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
        assert_eq!(snap.notes[0].start_tick, 0);
        assert_eq!(snap.notes[0].duration_ticks, 240);
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
        assert_eq!(snap.notes[0].duration_ticks, 960, "the pending note did not close at clip end");
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
        // Second note: tick 480 exactly — ticks in, ticks out, no rounding.
        assert_eq!(note_ons[1].data1, 64);
        assert_eq!(note_ons[1].tick, 480);
    }

    #[test]
    fn edited_snapshot_produces_different_events() {
        let mut notes = vec![
            NoteSnapshot { note: 60, velocity: 100, start_tick: 0, duration_ticks: 240, muted: false },
        ];

        let original = NoteSnapshot::to_clip_events(&notes, 960);
        assert_eq!(original[0].tick, 0); // note on at tick 0

        // Edit: move start to tick 480
        notes[0].start_tick = 480;
        let edited = NoteSnapshot::to_clip_events(&notes, 960);
        assert_eq!(edited[0].tick, 480); // note on at tick 480 now

        // Edit: make it shorter
        notes[0].duration_ticks = 96;
        let shorter = NoteSnapshot::to_clip_events(&notes, 960);
        let off_tick = shorter.iter().find(|e| e.status == 0x80).unwrap().tick;
        assert_eq!(off_tick, 576); // 480 + 96 = 576
    }
}
