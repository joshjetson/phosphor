//! Phrase layers: recorded performances stacked on a pad and played back
//! through one child instrument, instead of being rendered to audio.
//!
//! A phrase is the memory-cheap sibling of a sampled layer. The capture
//! machinery that would have written a take to PCM keeps the plan instead,
//! so four bars cost a few hundred events rather than a few megabytes, and
//! the sound comes from an instrument rather than a buffer.
//!
//! # One child, not one per pad
//!
//! Every phrase on every pad sounds through a single child instrument, which
//! is rendered exactly once per block however many phrases are running. That
//! is what makes the feature affordable — eighty-eight pads with an
//! instrument each is eighty-eight voice pools — and it is what the rest of
//! this module is shaped by:
//!
//! * a phrase's own control is its velocity scale, not a fader, because
//!   there is no per-phrase place in a shared render to put a fader;
//! * the pad's level, pan and envelope do not reach a phrase, for the same
//!   reason;
//! * the child hears nothing but what the runners send it, which is what
//!   makes "release every note this runner holds" a complete answer to a
//!   hung note.
//!
//! # The hung-note rule
//!
//! Every way a runner can end — the phrase running out, a gate release, a
//! poly cut, a choke, a steal, all-notes-off, all-sound-off, a reset — gives
//! back every note that runner put down on the child, in the same block it
//! ends. A runner does not give its slot back until those note-offs have
//! actually been handed over, so even a block whose event list filled up
//! cannot leave a key down on the child: the runner keeps its slot and tries
//! again at the top of the next block.

use std::sync::Arc;

use phosphor_plugin::sample::{PadPhrase, PhraseEvent};
use phosphor_plugin::{MidiEvent, Plugin};

/// Phrase slots per pad.
///
/// Four against the eight a pad gets for sampled layers, because a phrase
/// costs more at the moment it is triggered: a sampled layer takes a voice
/// from a pool that is sized for it, while a phrase takes one of the few
/// runners *and* pours notes into an instrument shared with every other pad.
/// Four phrases on one key is already a quartet; the cap is where the memory
/// and the note traffic are, so it is enforced here rather than trusted.
pub const MAX_PHRASES: usize = 4;

/// Phrases that can be playing at once, across the whole sampler.
///
/// A runner is one playing phrase, so this is "how many performances can
/// overlap": four phrases of one pad fired together, or eight pads each
/// running one, or one pad's phrase overlapping itself seven deep. Past it
/// the oldest gesture is stopped to make room, the same bargain the voice
/// pool strikes.
pub const PHRASE_RUNNERS: usize = 8;

/// Events a block will consider, handed over or not.
///
/// Two jobs in one number, and both of them are the deadline's. It is the
/// size of the list the child is given, so nothing here ever asks the
/// allocator; and it is the bound on the *work* a block can be made to do,
/// because a runner asks for one of these before it so much as looks at an
/// event. Without the second job a phrase carrying ten thousand events at
/// the same frame — a corrupt take, a stuck arpeggiator — would walk all ten
/// thousand inside a single sample, and a bound that only counts what gets
/// written is not a bound at all.
///
/// Eight runners would have to deliver thirty-two events each inside one
/// 512-sample block to reach it. Past it nothing is thrown away: the runner
/// keeps its place and carries on at the top of the next block, so the worst
/// case is a straggler arriving late rather than a note going missing.
const MAX_CHILD_EVENTS: usize = 256;

/// A silent child is stopped rather than rendered — but only after it has
/// been silent this long with nothing playing.
///
/// Generous on purpose. The cost of being early is a truncated release tail,
/// which is audible; the cost of being late is a second of an idle
/// instrument's `process`, which is not.
const CHILD_IDLE_SECONDS: f64 = 1.0;

/// Peak below which a block of the child's output counts as silence.
const CHILD_FLOOR: f32 = 1e-5;

/// A quiet event, for filling the block's event array. Status 0 is not a
/// MIDI status byte, so a stale slot cannot be mistaken for a note.
const NO_EVENT: MidiEvent =
    MidiEvent { sample_offset: 0, status: 0, data1: 0, data2: 0 };

/// A phrase as the engine holds it: the same fields [`PadPhrase`] arrives
/// with, sanitized into something a runner can trust completely.
pub(crate) struct PhraseSlot {
    events: Arc<[PhraseEvent]>,
    /// Performance length in frames, never shorter than the last event's
    /// own frame — a phrase is at least as long as the notes in it.
    frames: u64,
    gain: f32,
    transpose_with_key: bool,
    mute: bool,
    vel_lo: u8,
    vel_hi: u8,
}

impl PhraseSlot {
    /// Sanitize a delivered phrase into one a runner can be pointed at, or
    /// `None` when there is nothing to play.
    ///
    /// Runs on the audio thread, so everything here is O(1): the length
    /// floor reads the *last* event rather than scanning for the largest
    /// frame, because the capture machinery delivers events in time order
    /// and a scan over a four-bar phrase is unbounded work behind a
    /// deadline. An out-of-order list is not a fault either way — see
    /// [`PhraseRunner::advance`] for what it does instead.
    pub(crate) fn from_phrase(phrase: &PadPhrase) -> Option<Self> {
        if phrase.events.is_empty() {
            return None;
        }
        let last = phrase.events.last().map_or(0, |e| e.frame.saturating_add(1));
        let (vel_lo, vel_hi) = super::pad::repair_vel(phrase.vel_lo, phrase.vel_hi);
        Some(Self {
            events: Arc::clone(&phrase.events),
            frames: phrase.frames.max(last),
            gain: if phrase.gain.is_nan() { 0.0 } else { phrase.gain.clamp(0.0, 4.0) },
            transpose_with_key: phrase.transpose_with_key,
            mute: phrase.mute,
            vel_lo,
            vel_hi,
        })
    }

    /// Whether this phrase answers a hit at `vel`. The layer rule, shared:
    /// a muted phrase answers nothing, a velocity-switched one answers only
    /// its own range.
    pub(crate) fn answers(&self, vel: u8) -> bool {
        !self.mute && super::pad::in_vel_window(self.vel_lo, self.vel_hi, vel)
    }

    #[cfg(test)]
    pub(crate) fn length(&self) -> u64 {
        self.frames
    }

    #[cfg(test)]
    pub(crate) fn velocity_scale(&self) -> f32 {
        self.gain
    }
}

/// The block's note traffic for the child, in fixed storage.
///
/// Built as the block is walked, so it comes out in timeline order and needs
/// no sort of its own — unlike the sampler's own incoming events, which
/// arrive from a host that only promises to have sorted them.
pub(super) struct ChildEvents {
    buf: [MidiEvent; MAX_CHILD_EVENTS],
    /// Events written.
    len: usize,
    /// Events *considered*, written or not. See [`MAX_CHILD_EVENTS`] — this
    /// is the one the budget is kept against, because an event a runner
    /// decides not to send still cost the time it took to decide.
    spent: usize,
}

impl ChildEvents {
    pub(super) fn new() -> Self {
        Self { buf: [NO_EVENT; MAX_CHILD_EVENTS], len: 0, spent: 0 }
    }

    /// Claim one event's worth of the block's budget.
    ///
    /// `false` means the block is full, and every caller answers it the same
    /// way: leave the event where it is and come back next block. Nothing is
    /// dropped on this path, which is what makes a full block a late note
    /// rather than a missing one — and, for a note-off, the difference
    /// between a busy bar and a key held down forever.
    #[must_use]
    fn claim(&mut self) -> bool {
        if self.spent >= MAX_CHILD_EVENTS {
            return false;
        }
        self.spent += 1;
        true
    }

    /// Write an event that has already been claimed. The claim bounds the
    /// length, so the guard here is a belt on top of braces.
    fn write(&mut self, offset: u32, status: u8, data1: u8, data2: u8) {
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = MidiEvent { sample_offset: offset, status, data1, data2 };
            self.len += 1;
        }
    }

    /// Pass a controller straight through to the child — the only message
    /// the sampler sends on its own behalf rather than a runner's.
    pub(super) fn push_cc(&mut self, offset: u32, controller: u8) {
        if self.claim() {
            self.write(offset, 0xB0, controller, 0);
        }
    }

    pub(super) fn events(&self) -> &[MidiEvent] {
        &self.buf[..self.len]
    }
}

/// Everything the engine settles before a runner starts.
///
/// A struct rather than six arguments because the two note numbers are the
/// halves of one decision — the played key against the pad's root — and a
/// pair of `u8`s next to each other in an argument list is a transposition
/// bug waiting for a refactor.
pub(super) struct RunnerStart {
    pub pad: usize,
    pub hit: u64,
    /// The key that was pressed.
    pub note: u8,
    /// The pad's reference key.
    pub root: u8,
    /// The velocity curve the engine has already applied to this hit, so a
    /// phrase answers the keyboard the way a sampled layer does.
    pub vel_gain: f32,
    /// A gate pad's phrases stop when the key comes up; a one-shot's play
    /// to the end. Latched here, like the voice latches its trigger mode,
    /// so changing the pad mid-phrase cannot change what the phrase is.
    pub gate: bool,
}

/// One playing phrase.
pub(super) struct PhraseRunner {
    /// The performance. `None` means the slot is free.
    ///
    /// Holding its own `Arc` is what lets the pad's phrases be replaced
    /// while this one plays: the runner finishes on the events it started
    /// with, exactly as a voice finishes on the buffer it started with.
    events: Option<Arc<[PhraseEvent]>>,
    pad: usize,
    /// The note-on gesture that fired it, shared with that hit's voices, so
    /// poly counts a phrase and a sample as one hit.
    hit: u64,
    /// Index of the next event to hand over.
    next: usize,
    /// Playhead, in frames since this runner started.
    frame: u64,
    frames: u64,
    /// Semitones added to every note, derived once at the start.
    shift: i16,
    /// What every recorded velocity is multiplied by.
    vel_scale: f32,
    gate: bool,
    /// Notes this runner has put down on the child and not yet taken up,
    /// one bit per MIDI note. Two words, no allocation, and the only record
    /// of what the child is holding on this runner's behalf.
    held: [u64; 2],
    /// Playing is over and only the note-offs are owed. See `flush`.
    ending: bool,
}

/// The word and bit a note occupies in the held-note set.
#[inline]
fn held_bit(note: u8) -> (usize, u64) {
    (usize::from(note >> 6), 1u64 << (note & 63))
}

impl PhraseRunner {
    const fn new() -> Self {
        Self {
            events: None,
            pad: 0,
            hit: 0,
            next: 0,
            frame: 0,
            frames: 0,
            shift: 0,
            vel_scale: 1.0,
            gate: false,
            held: [0; 2],
            ending: false,
        }
    }

    fn is_free(&self) -> bool {
        self.events.is_none()
    }

    /// Playing, and so counting toward its pad's poly and answering a
    /// choke. A runner that is only flushing the note-offs it owes has
    /// already been spoken for and must not block its own replacement —
    /// the rule the voice pool states as `is_active`.
    fn is_playing(&self) -> bool {
        self.events.is_some() && !self.ending
    }

    fn start(&mut self, slot: &PhraseSlot, ctx: &RunnerStart) {
        self.events = Some(Arc::clone(&slot.events));
        self.pad = ctx.pad;
        self.hit = ctx.hit;
        self.next = 0;
        self.frame = 0;
        self.frames = slot.frames;
        self.shift = if slot.transpose_with_key {
            i16::from(ctx.note) - i16::from(ctx.root)
        } else {
            0
        };
        self.vel_scale = slot.gain * ctx.vel_gain;
        self.gate = ctx.gate;
        self.held = [0; 2];
        self.ending = false;
    }

    /// Hand over every event due at this frame, then take one step.
    ///
    /// Due means "at or before the playhead", which is also what happens to
    /// an event list that is not in time order: a straggler is handed over
    /// the moment the runner reaches it, late rather than never. The capture
    /// side is contracted to deliver sorted events and does; the engine
    /// cannot sort them itself, because the list is shared and immutable on
    /// this side of the fence, so tolerating the case is the whole of the
    /// defence and it costs nothing.
    fn advance(&mut self, offset: u32, out: &mut ChildEvents) {
        if self.ending {
            self.flush(offset, out);
            return;
        }
        if self.events.is_none() {
            return;
        }
        loop {
            let due = self
                .events
                .as_deref()
                .and_then(|evs| evs.get(self.next).copied())
                .filter(|e| e.frame <= self.frame);
            let Some(ev) = due else { break };
            if !self.send(ev, offset, out) {
                // The block is full. The event keeps its place in the list
                // and goes out at the top of the next one.
                break;
            }
            self.next += 1;
        }
        self.frame += 1;
        let played_out = self.next >= self.events.as_deref().map_or(0, <[PhraseEvent]>::len);
        if played_out && self.frame >= self.frames {
            self.stop(offset, out);
        }
    }

    /// Put one recorded event on the child.
    ///
    /// `false` means the block had no room to even consider it, and the
    /// caller must leave the event where it is. Everything else — a note
    /// transposed off the keyboard, a velocity scaled to nothing, a status
    /// that is not a note — is a decision, and a decision counts as handled.
    fn send(&mut self, ev: PhraseEvent, offset: u32, out: &mut ChildEvents) -> bool {
        if !out.claim() {
            return false;
        }
        let Some(note) = self.shifted(ev.data1) else { return true };
        // A note-on at velocity 0 is a note-off, the convention every MIDI
        // source uses and the one the sampler's own input already follows.
        let is_off = ev.status & 0xF0 == 0x80 || (ev.status & 0xF0 == 0x90 && ev.data2 == 0);
        if !is_off && ev.status & 0xF0 != 0x90 {
            return true;
        }
        let (word, bit) = held_bit(note);
        if is_off {
            out.write(offset, 0x80, note, 0);
            self.held[word] &= !bit;
            return true;
        }
        let vel = (f32::from(ev.data2) * self.vel_scale).round().clamp(0.0, 127.0) as u8;
        // A phrase turned all the way down plays nothing, rather than
        // playing every note at velocity 1: the scale is a fader, and the
        // bottom of a fader is silence.
        if vel == 0 {
            return true;
        }
        out.write(offset, 0x90, note, vel);
        self.held[word] |= bit;
        true
    }

    /// The note this runner plays for a recorded one, or `None` when the
    /// transposition has pushed it off the keyboard.
    ///
    /// The shift is derived once, at the start, precisely so that a note-on
    /// and its note-off answer this question identically. A shift that could
    /// move between the two would drop one and keep the other, and the one
    /// it keeps would be a key held down on the child forever.
    fn shifted(&self, note: u8) -> Option<u8> {
        let shifted = i16::from(note) + self.shift;
        (0..=127).contains(&shifted).then_some(shifted as u8)
    }

    /// The key came up. Gates stop; one-shots ignore it by definition, the
    /// same answer the voice gives.
    fn note_off(&mut self, offset: u32, out: &mut ChildEvents) {
        if self.gate {
            self.stop(offset, out);
        }
    }

    /// Stop playing and give back every note still held.
    fn stop(&mut self, offset: u32, out: &mut ChildEvents) {
        self.ending = true;
        self.flush(offset, out);
    }

    /// Hand over the note-offs this runner owes, freeing the slot when
    /// there are none left.
    ///
    /// A block whose event list has filled up cannot take them. The runner
    /// keeps its slot and its held-note record and tries again at the top of
    /// the next block, which is why a saturated block cannot leave the child
    /// holding a key down.
    fn flush(&mut self, offset: u32, out: &mut ChildEvents) {
        for word in 0..self.held.len() {
            while self.held[word] != 0 {
                if !out.claim() {
                    return;
                }
                let bit = self.held[word].trailing_zeros();
                let note = (word * 64) as u8 + bit as u8;
                out.write(offset, 0x80, note, 0);
                self.held[word] &= !(1u64 << bit);
            }
        }
        self.release();
    }

    /// Give the slot back. Everything this runner held has been handed over.
    fn release(&mut self) {
        // A refcount decrement, never a free: the capture side keeps its
        // own reference to the event list (see [`PadPhrase`]).
        self.events = None;
        self.ending = false;
        self.next = 0;
        self.frame = 0;
        self.held = [0; 2];
    }

    /// Forget everything without handing anything over. For `reset`, where
    /// the child is being reset in the same breath.
    fn silence(&mut self) {
        self.release();
    }
}

/// The runners, and the rules that apply to all of them at once.
pub(super) struct RunnerPool {
    runners: [PhraseRunner; PHRASE_RUNNERS],
}

impl RunnerPool {
    pub(super) fn new() -> Self {
        Self { runners: [const { PhraseRunner::new() }; PHRASE_RUNNERS] }
    }

    /// Whether anything is playing or still owes a note-off. What decides
    /// that the child has to be rendered this block.
    pub(super) fn busy(&self) -> bool {
        self.runners.iter().any(|r| !r.is_free())
    }

    /// Hand over note-offs left owing from a saturated block, before
    /// anything else this block is written.
    pub(super) fn flush_owed(&mut self, out: &mut ChildEvents) {
        for r in &mut self.runners {
            if r.ending {
                r.flush(0, out);
            }
        }
    }

    /// One sample of every runner.
    pub(super) fn advance(&mut self, offset: u32, out: &mut ChildEvents) {
        for r in &mut self.runners {
            if !r.is_free() {
                r.advance(offset, out);
            }
        }
    }

    /// Start a phrase, if a slot can be had honestly.
    pub(super) fn start(
        &mut self,
        slot: &PhraseSlot,
        ctx: &RunnerStart,
        offset: u32,
        out: &mut ChildEvents,
    ) {
        if let Some(i) = self.free_slot(offset, out) {
            self.runners[i].start(slot, ctx);
        }
    }

    /// A free slot, else the oldest gesture's, stopped first.
    ///
    /// The oldest rather than a fixed slot for the reason the voice pool
    /// steals the oldest: the newest gesture is the one the player just
    /// made. `None` means the steal could not hand back the notes it held —
    /// the one block in a thousand whose event list filled up — and a phrase
    /// that cannot start without abandoning a held note does not start.
    fn free_slot(&mut self, offset: u32, out: &mut ChildEvents) -> Option<usize> {
        if let Some(i) = self.runners.iter().position(PhraseRunner::is_free) {
            return Some(i);
        }
        let mut victim = 0usize;
        let mut oldest = u64::MAX;
        for (i, r) in self.runners.iter().enumerate() {
            if r.hit < oldest {
                oldest = r.hit;
                victim = i;
            }
        }
        self.runners[victim].stop(offset, out);
        self.runners[victim].is_free().then_some(victim)
    }

    /// The hits a pad has running, for the poly census.
    pub(super) fn hits_on(&self, pad: usize) -> impl Iterator<Item = u64> + '_ {
        self.runners.iter().filter(move |r| r.is_playing() && r.pad == pad).map(|r| r.hit)
    }

    /// Stop every runner belonging to one hit — what a poly cut is.
    pub(super) fn stop_hit(&mut self, pad: usize, hit: u64, offset: u32, out: &mut ChildEvents) {
        for r in &mut self.runners {
            if r.is_playing() && r.pad == pad && r.hit == hit {
                r.stop(offset, out);
            }
        }
    }

    /// Stop every runner on a pad in `group`, other than the pad that fired.
    ///
    /// The group of a pad is asked for rather than looked up, so the pool
    /// never needs to know what a pad is.
    pub(super) fn choke(
        &mut self,
        pad: usize,
        group: u8,
        group_of: impl Fn(usize) -> u8,
        offset: u32,
        out: &mut ChildEvents,
    ) {
        for r in &mut self.runners {
            if r.is_playing() && r.pad != pad && group_of(r.pad) == group {
                r.stop(offset, out);
            }
        }
    }

    /// The key on a pad came up: its gate runners stop.
    pub(super) fn note_off(&mut self, pad: usize, offset: u32, out: &mut ChildEvents) {
        for r in &mut self.runners {
            if r.is_playing() && r.pad == pad {
                r.note_off(offset, out);
            }
        }
    }

    /// All-notes-off: gates release, one-shots play on.
    pub(super) fn release_gates(&mut self, offset: u32, out: &mut ChildEvents) {
        for r in &mut self.runners {
            if r.is_playing() {
                r.note_off(offset, out);
            }
        }
    }

    /// All-sound-off: everything stops and gives its notes back.
    pub(super) fn stop_all(&mut self, offset: u32, out: &mut ChildEvents) {
        for r in &mut self.runners {
            if r.is_playing() {
                r.stop(offset, out);
            }
        }
    }

    /// Forget everything, handing nothing over. For `reset` only, where the
    /// child is reset in the same breath and has no notes left to hold.
    pub(super) fn silence(&mut self) {
        for r in &mut self.runners {
            r.silence();
        }
    }

    #[cfg(test)]
    pub(super) fn playing(&self) -> usize {
        self.runners.iter().filter(|r| r.is_playing()).count()
    }

    #[cfg(test)]
    pub(super) fn holds_any_note(&self) -> bool {
        self.runners.iter().any(|r| r.held != [0; 2])
    }
}

/// The child instrument and the buffer it is rendered into.
pub(super) struct ChildHost {
    child: Option<Box<dyn Plugin>>,
    l: Vec<f32>,
    r: Vec<f32>,
    sample_rate: f64,
    max_block: usize,
    /// Frames the child has been silent with nothing playing.
    idle: u64,
    idle_limit: u64,
}

impl ChildHost {
    pub(super) fn new() -> Self {
        Self {
            child: None,
            l: Vec::new(),
            r: Vec::new(),
            sample_rate: 44_100.0,
            max_block: 0,
            idle: u64::MAX,
            idle_limit: u64::MAX,
        }
    }

    /// The sampler's own `init`: size the buffer, and start any child that
    /// arrived before it.
    pub(super) fn init(&mut self, sample_rate: f64, max_block: usize) {
        self.sample_rate = sample_rate;
        self.max_block = max_block;
        self.l = vec![0.0; max_block];
        self.r = vec![0.0; max_block];
        self.idle_limit = (CHILD_IDLE_SECONDS * sample_rate) as u64;
        self.idle = self.idle_limit;
        if let Some(child) = self.child.as_mut() {
            child.init(sample_rate, max_block);
        }
    }

    /// Take the child, or take it away.
    ///
    /// `init` is called here because this is where the allocation belongs:
    /// the command that carries a child is charged the heavy rate exactly so
    /// that building its voices, and freeing the child it replaces, are
    /// inside the callback's budget. That the old child is freed on the
    /// audio thread is the accepted `SetInstrument` precedent.
    pub(super) fn set(&mut self, child: Option<Box<dyn Plugin>>) {
        self.child = child;
        if let Some(child) = self.child.as_mut() {
            child.init(self.sample_rate, self.max_block);
        }
        // Whatever the old child was still ringing is gone with it, and a
        // new one has not sounded yet.
        self.idle = self.idle_limit;
    }

    pub(super) fn is_loaded(&self) -> bool {
        self.child.is_some()
    }

    /// One control on the child, in the child's own numbering.
    ///
    /// A store inside an instrument that already exists: no allocation, no
    /// `init`, nothing given back. It is how the panel a phrase was played
    /// on follows the child across, one control per command.
    pub(super) fn set_parameter(&mut self, index: usize, value: f32) {
        if let Some(child) = self.child.as_mut() {
            child.set_parameter(index, value);
        }
    }

    pub(super) fn reset(&mut self) {
        if let Some(child) = self.child.as_mut() {
            child.reset();
        }
        self.idle = self.idle_limit;
    }

    /// Render one block of the child, answering whether it wrote anything.
    ///
    /// Skipped entirely when there is no child, and when nothing is playing
    /// and the child has been silent long enough to be over. The instrument
    /// is not asked whether it is finished because no instrument can answer
    /// that — what it has been *doing* is asked instead, which is a question
    /// the output already contains.
    ///
    /// The buffer is not cleared first. Every plugin in the project writes
    /// its outputs rather than adding to them, which is the contract the
    /// mixer already runs every instrument under.
    pub(super) fn render(
        &mut self,
        frames: usize,
        events: &[MidiEvent],
        runners_busy: bool,
    ) -> bool {
        let quiet_for_good = !runners_busy && events.is_empty() && self.idle >= self.idle_limit;
        if self.child.is_none() || quiet_for_good || frames == 0 {
            return false;
        }
        // Grows only if a device hands the callback a block larger than the
        // maximum it promised — the deliberately dead branch the mixer's own
        // buffers carry, for the same reason.
        if self.l.len() < frames {
            self.l.resize(frames, 0.0);
            self.r.resize(frames, 0.0);
        }
        let (l, r) = (&mut self.l[..frames], &mut self.r[..frames]);
        let mut outs: [&mut [f32]; 2] = [l, r];
        if let Some(child) = self.child.as_mut() {
            child.process(&[], &mut outs, events);
        }

        let peak = self.l[..frames]
            .iter()
            .chain(self.r[..frames].iter())
            .fold(0.0f32, |a, s| a.max(s.abs()));
        if runners_busy || peak >= CHILD_FLOOR || !peak.is_finite() {
            self.idle = 0;
        } else {
            self.idle = self.idle.saturating_add(frames as u64);
        }
        true
    }

    /// The block just rendered, for summing into the mix.
    pub(super) fn block(&self, frames: usize) -> (&[f32], &[f32]) {
        let n = frames.min(self.l.len()).min(self.r.len());
        (&self.l[..n], &self.r[..n])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::tests::{cc, note_off, note_on, peak, process, sine_pcm, SR};
    use crate::sampler::{Sampler, PAD_BASE_NOTE};
    use crate::synth::tests::allocations_during;
    use phosphor_plugin::sample::{PadConfig, PadLayer, TrigMode};
    use phosphor_plugin::{ParameterInfo, PluginCategory, PluginInfo};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

    /// Room for the child's whole diary in one test.
    const LOG_CAP: usize = 512;

    /// What the child instrument saw, readable from the test after the child
    /// itself has been handed over to the sampler.
    ///
    /// Atomics rather than a lock, because this runs inside the audio path
    /// the engine is being tested in: a test double that breaks the rule
    /// under test is not a test double.
    struct ChildLog {
        len: AtomicUsize,
        status: [AtomicU8; LOG_CAP],
        note: [AtomicU8; LOG_CAP],
        vel: [AtomicU8; LOG_CAP],
        /// The frame, counted from the child's own start, at which the event
        /// arrived. What proves a sample offset was honoured.
        frame: [AtomicU64; LOG_CAP],
        held: [AtomicBool; 128],
        resets: AtomicUsize,
    }

    impl ChildLog {
        fn new() -> Self {
            Self {
                len: AtomicUsize::new(0),
                status: std::array::from_fn(|_| AtomicU8::new(0)),
                note: std::array::from_fn(|_| AtomicU8::new(0)),
                vel: std::array::from_fn(|_| AtomicU8::new(0)),
                frame: std::array::from_fn(|_| AtomicU64::new(0)),
                held: std::array::from_fn(|_| AtomicBool::new(false)),
                resets: AtomicUsize::new(0),
            }
        }

        fn record(&self, status: u8, note: u8, vel: u8, frame: u64) {
            let i = self.len.fetch_add(1, Ordering::Relaxed);
            if i >= LOG_CAP {
                return;
            }
            self.status[i].store(status, Ordering::Relaxed);
            self.note[i].store(note, Ordering::Relaxed);
            self.vel[i].store(vel, Ordering::Relaxed);
            self.frame[i].store(frame, Ordering::Relaxed);
        }

        /// Every event, as (status, note, velocity, frame).
        fn entries(&self) -> Vec<(u8, u8, u8, u64)> {
            let n = self.len.load(Ordering::Relaxed).min(LOG_CAP);
            (0..n)
                .map(|i| {
                    (
                        self.status[i].load(Ordering::Relaxed),
                        self.note[i].load(Ordering::Relaxed),
                        self.vel[i].load(Ordering::Relaxed),
                        self.frame[i].load(Ordering::Relaxed),
                    )
                })
                .collect()
        }

        fn ons(&self) -> Vec<(u8, u8, u64)> {
            self.entries().into_iter().filter(|e| e.0 == 0x90).map(|e| (e.1, e.2, e.3)).collect()
        }

        /// How many events the child has been given, diary size aside.
        fn count(&self) -> usize {
            self.len.load(Ordering::Relaxed)
        }

        /// The notes the child is holding down right now.
        fn held_notes(&self) -> Vec<u8> {
            (0..128u8).filter(|n| self.held[usize::from(*n)].load(Ordering::Relaxed)).collect()
        }

        fn holds_nothing(&self) -> bool {
            self.held_notes().is_empty()
        }
    }

    /// An instrument that writes down every note it is given and sounds a
    /// sine for each one it is holding.
    ///
    /// Two jobs on purpose: the diary is how a hung note is proved
    /// impossible, and the audio is how the mixing, the offsets and the
    /// transposition are proved to be real rather than bookkeeping.
    struct TestChild {
        log: Arc<ChildLog>,
        sample_rate: f64,
        clock: u64,
        on: [bool; 128],
        phase: [f32; 128],
    }

    impl TestChild {
        fn new(log: &Arc<ChildLog>) -> Self {
            Self {
                log: Arc::clone(log),
                sample_rate: SR,
                clock: 0,
                on: [false; 128],
                phase: [0.0; 128],
            }
        }
    }

    impl Plugin for TestChild {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "TestChild".into(),
                version: "0".into(),
                author: "Phosphor".into(),
                category: PluginCategory::Instrument,
            }
        }

        fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
            self.sample_rate = sample_rate;
        }

        fn process(
            &mut self,
            _inputs: &[&[f32]],
            outputs: &mut [&mut [f32]],
            midi_events: &[MidiEvent],
        ) {
            let frames = outputs[0].len();
            let mut ei = 0usize;
            for i in 0..frames {
                while ei < midi_events.len() && midi_events[ei].sample_offset as usize <= i {
                    let ev = midi_events[ei];
                    let note = usize::from(ev.data1.min(127));
                    match ev.status & 0xF0 {
                        0x90 if ev.data2 > 0 => {
                            self.on[note] = true;
                            self.phase[note] = 0.0;
                            self.log.held[note].store(true, Ordering::Relaxed);
                            self.log.record(0x90, ev.data1, ev.data2, self.clock + i as u64);
                        }
                        0x90 | 0x80 => {
                            self.on[note] = false;
                            self.log.held[note].store(false, Ordering::Relaxed);
                            self.log.record(0x80, ev.data1, 0, self.clock + i as u64);
                        }
                        0xB0 => {
                            if ev.data1 == 120 || ev.data1 == 123 {
                                for n in 0..128 {
                                    self.on[n] = false;
                                    self.log.held[n].store(false, Ordering::Relaxed);
                                }
                            }
                            self.log.record(0xB0, ev.data1, ev.data2, self.clock + i as u64);
                        }
                        _ => {}
                    }
                    ei += 1;
                }

                let mut sum = 0.0f32;
                for n in 0..128usize {
                    if !self.on[n] {
                        continue;
                    }
                    let hz = 440.0 * ((n as f32 - 69.0) / 12.0).exp2();
                    self.phase[n] += core::f32::consts::TAU * hz / self.sample_rate as f32;
                    if self.phase[n] > core::f32::consts::TAU {
                        self.phase[n] -= core::f32::consts::TAU;
                    }
                    sum += 0.25 * self.phase[n].sin();
                }
                for out in outputs.iter_mut() {
                    out[i] = sum;
                }
            }
            self.clock += frames as u64;
        }

        fn parameter_count(&self) -> usize {
            0
        }

        fn parameter_info(&self, _index: usize) -> Option<ParameterInfo> {
            None
        }

        fn get_parameter(&self, _index: usize) -> f32 {
            0.0
        }

        fn set_parameter(&mut self, _index: usize, _value: f32) {}

        fn reset(&mut self) {
            self.on = [false; 128];
            for n in 0..128 {
                self.log.held[n].store(false, Ordering::Relaxed);
            }
            self.log.resets.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A phrase holding one note for `len` frames, starting at frame 0.
    fn held_note(note: u8, vel: u8, len: u64) -> PadPhrase {
        let events = vec![
            PhraseEvent { frame: 0, status: 0x90, data1: note, data2: vel },
            PhraseEvent { frame: len, status: 0x80, data1: note, data2: 0 },
        ];
        PadPhrase::from_events(Arc::from(events), len + 1)
    }

    /// A phrase whose note-on is never answered — a capture cut short.
    fn dangling_note(note: u8, len: u64) -> PadPhrase {
        let events = vec![PhraseEvent { frame: 0, status: 0x90, data1: note, data2: 100 }];
        PadPhrase::from_events(Arc::from(events), len)
    }

    /// A sampler with a child, and `phrases` on the pad for `pad_note`.
    fn with_phrases(
        pad_note: u8,
        config: PadConfig,
        phrases: &[PadPhrase],
    ) -> (Sampler, Arc<ChildLog>) {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let log = Arc::new(ChildLog::new());
        s.set_sampler_child(Some(Box::new(TestChild::new(&log))));
        s.set_sampler_pad(pad_note - PAD_BASE_NOTE, &config, &[]);
        s.set_sampler_phrases(pad_note - PAD_BASE_NOTE, phrases);
        (s, log)
    }

    /// Render `blocks` blocks of `n` samples, the events going into the
    /// first one, and return the left channel end to end.
    fn run(s: &mut Sampler, events: &[MidiEvent], blocks: usize, n: usize) -> Vec<f32> {
        let mut all = Vec::new();
        for b in 0..blocks {
            let (l, _) = process(s, if b == 0 { events } else { &[] }, n);
            all.extend_from_slice(&l);
        }
        all
    }

    /// Zero crossings per second — proportional to the fundamental, and
    /// unlike a period estimate it cannot lock onto an octave.
    fn crossings_per_second(buf: &[f32]) -> f64 {
        let mut crossings = 0usize;
        let mut prev = 0.0f32;
        for &v in buf {
            if (prev <= 0.0 && v > 0.0) || (prev >= 0.0 && v < 0.0) {
                crossings += 1;
            }
            prev = v;
        }
        crossings as f64 * SR / buf.len() as f64
    }

    // ── It sounds ──

    #[test]
    fn a_phrase_pad_sounds_through_the_child() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 20_000)]);
        // Nothing sampled on the pad: whatever comes out is the child's.
        let out = run(&mut s, &[note_on(60, 127, 0)], 8, 512);
        assert!(peak(&out[1_000..]) > 0.05, "the phrase was silent: {}", peak(&out[1_000..]));
        assert_eq!(s.playing_phrases(), 1);
        assert_eq!(log.ons(), vec![(64, 100, 0)], "the child heard the wrong note");
    }

    #[test]
    fn the_same_phrase_twice_is_the_same_sound_twice() {
        let (mut s, _log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 20_000)]);
        let first = run(&mut s, &[note_on(60, 127, 0)], 6, 512);
        s.reset();
        let second = run(&mut s, &[note_on(60, 127, 0)], 6, 512);
        assert_eq!(first, second, "a phrase played differently the second time");
    }

    #[test]
    fn a_phrase_lands_on_the_offset_it_was_triggered_at() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 20_000)]);
        // The hit is 300 samples into the block, so the child must hear the
        // phrase's first note there rather than at the top of the block.
        process(&mut s, &[note_on(60, 127, 300)], 512);
        assert_eq!(log.ons(), vec![(64, 100, 300)], "the phrase did not keep its offset");
    }

    #[test]
    fn a_phrase_and_a_sample_share_one_pad() {
        let mut cfg = PadConfig::for_key(60);
        cfg.keytrack = true;
        let (mut s, log) = with_phrases(60, cfg, &[held_note(64, 100, 20_000)]);
        s.set_sampler_pad(39, &cfg, &[PadLayer::from_pcm(sine_pcm(0.5, 44_100))]);

        let with_both = run(&mut s, &[note_on(60, 127, 0)], 4, 512);
        assert_eq!(s.playing_phrases(), 1, "the sample took the pad from the phrase");
        assert_eq!(log.ons().len(), 1, "the sample stopped the phrase from playing");

        s.reset();
        s.set_sampler_phrases(39, &[]);
        let sample_only = run(&mut s, &[note_on(60, 127, 0)], 4, 512);
        assert!(
            peak(&with_both[1_000..]) > peak(&sample_only[1_000..]) * 1.2,
            "the phrase added nothing to the pad: {} against {}",
            peak(&with_both[1_000..]),
            peak(&sample_only[1_000..]),
        );
    }

    /// A phrase is summed in ahead of the sampler's own output stage, so the
    /// level knob moves it exactly as it moves a sample. Anything else and
    /// turning the sampler down would leave the phrases at full volume.
    #[test]
    fn the_level_knob_reaches_the_child() {
        let (mut s, _log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        let at_default = peak(&run(&mut s, &[note_on(60, 127, 0)], 4, 512)[1_000..]);
        assert!(at_default > 0.1, "the phrase was too quiet to measure: {at_default}");

        s.reset();
        s.set_parameter(crate::sampler::P_LEVEL, 0.4); // half the default
        let at_half = peak(&run(&mut s, &[note_on(60, 127, 0)], 4, 512)[1_000..]);
        assert!(
            (at_half - at_default * 0.5).abs() < 0.01,
            "level 0.4 gave {at_half} against {at_default} at default",
        );
    }

    /// Every pad running a phrase, hammered. The output must stay on the
    /// rails, the pool must stay inside itself, and when it is all over the
    /// child must be holding nothing at all.
    #[test]
    fn hammering_every_pad_with_phrases_leaks_nothing() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let log = Arc::new(ChildLog::new());
        s.set_sampler_child(Some(Box::new(TestChild::new(&log))));
        for note in 21..=108u8 {
            let mut cfg = PadConfig::for_key(note);
            cfg.poly = 2;
            cfg.choke = 1;
            let pad = note - PAD_BASE_NOTE;
            s.set_sampler_pad(pad, &cfg, &[]);
            s.set_sampler_phrases(
                pad,
                &[dangling_note(note % 128, 400_000), held_note((note + 7) % 128, 90, 400_000)],
            );
        }
        for round in 0..4 {
            for note in 21..=108u8 {
                let (l, r) = process(&mut s, &[note_on(note, 127, 0)], 64);
                for x in l.iter().chain(r.iter()) {
                    assert!(x.is_finite(), "round {round} note {note} went non-finite");
                    assert!(x.abs() <= 1.0, "round {round} note {note} left the rails: {x}");
                }
                assert!(
                    s.playing_phrases() <= PHRASE_RUNNERS,
                    "the runner pool overflowed: {}",
                    s.playing_phrases(),
                );
            }
        }
        // The panic, then long enough for any owed note-off to be handed
        // over: nothing may be left down.
        process(&mut s, &[cc(120, 0)], 512);
        for _ in 0..8 {
            process(&mut s, &[], 512);
        }
        assert_eq!(s.playing_phrases(), 0);
        assert!(!s.phrases_hold_notes(), "the storm left a note owing");
        assert!(log.holds_nothing(), "the storm left {:?} held", log.held_notes());
    }

    // ── Transposition ──

    /// A pad is one key, so the distance a phrase shifts by is the pad's key
    /// against its root — the same lever `keytrack` pulls for a sample, and
    /// the reason a kit spread chromatically across the bed transposes.
    #[test]
    fn transposition_shifts_the_phrase_by_the_distance_from_the_root() {
        let mut phrase = held_note(60, 100, 400_000);
        phrase.transpose_with_key = true;
        // Root at the pad's own key: no shift, and a pitch to measure
        // everything else against.
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[phrase.clone()]);
        let at_root = run(&mut s, &[note_on(60, 127, 0)], 12, 512);
        assert_eq!(log.ons(), vec![(60, 100, 0)]);
        let root_hz = crossings_per_second(&at_root[2_000..]);
        assert!(root_hz > 100.0, "the phrase at its root never sounded");

        // Root an octave below the pad: the phrase plays an octave up.
        let mut up_cfg = PadConfig::for_key(60);
        up_cfg.root = 48;
        let (mut s, log) = with_phrases(60, up_cfg, &[phrase]);
        let up = run(&mut s, &[note_on(60, 127, 0)], 12, 512);
        assert_eq!(log.ons(), vec![(72, 100, 0)], "the note did not shift");
        let up_hz = crossings_per_second(&up[2_000..]);
        assert!(
            (up_hz / root_hz - 2.0).abs() < 0.1,
            "an octave up gave {up_hz} against {root_hz}",
        );
    }

    #[test]
    fn a_phrase_without_transposition_ignores_the_pads_root() {
        let mut off_root = PadConfig::for_key(60);
        off_root.root = 48;
        let (mut s, log) = with_phrases(60, off_root, &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(
            log.ons(),
            vec![(64, 100, 0)],
            "a phrase transposed with the shift turned off",
        );
    }

    /// The trap the shift's whole design exists to avoid: a note pushed off
    /// the keyboard must take its note-off with it, or the off leaks and the
    /// child is told to release a key it was never given.
    #[test]
    fn a_note_transposed_off_the_keyboard_is_dropped_whole() {
        let mut phrase = held_note(120, 100, 8_000);
        phrase.transpose_with_key = true;
        // The top pad against a root of 60 is +48 semitones, which puts the
        // phrase's note 120 at 168.
        let mut cfg = PadConfig::for_key(108);
        cfg.root = 60;
        let (mut s, log) = with_phrases(108, cfg, &[phrase]);

        let out = run(&mut s, &[note_on(108, 127, 0)], 30, 512);
        assert!(
            log.entries().is_empty(),
            "an out-of-range note reached the child: {:?}",
            log.entries(),
        );
        assert!(log.holds_nothing());
        assert!(!s.phrases_hold_notes(), "the engine thinks a dropped note is held");
        assert_eq!(peak(&out), 0.0, "a phrase off the keyboard made a sound");
        assert_eq!(s.playing_phrases(), 0, "the runner outlived its phrase");
    }

    #[test]
    fn transposition_below_zero_is_dropped_the_same_way() {
        let mut phrase = held_note(2, 100, 8_000);
        phrase.transpose_with_key = true;
        // The bottom pad against a root of 60 is −39, which puts note 2 at
        // −37 — off the other end, and just as gone.
        let mut cfg = PadConfig::for_key(21);
        cfg.root = 60;
        let (mut s, log) = with_phrases(21, cfg, &[phrase]);
        run(&mut s, &[note_on(21, 127, 0)], 30, 512);
        assert!(log.entries().is_empty(), "a note below zero reached the child");
        assert!(!s.phrases_hold_notes());
        assert_eq!(s.playing_phrases(), 0, "the runner outlived its phrase");
    }

    // ── Giving notes back ──

    #[test]
    fn a_gate_release_stops_a_phrase_and_gives_every_note_back() {
        let mut cfg = PadConfig::for_key(60);
        cfg.trig = TrigMode::Gate;
        // Two notes held far past the release: the stop lands mid-phrase.
        let events = vec![
            PhraseEvent { frame: 0, status: 0x90, data1: 60, data2: 100 },
            PhraseEvent { frame: 10, status: 0x90, data1: 67, data2: 100 },
            PhraseEvent { frame: 400_000, status: 0x80, data1: 60, data2: 0 },
            PhraseEvent { frame: 400_000, status: 0x80, data1: 67, data2: 0 },
        ];
        let (mut s, log) =
            with_phrases(60, cfg, &[PadPhrase::from_events(Arc::from(events), 400_001)]);

        let sounding = run(&mut s, &[note_on(60, 127, 0)], 4, 512);
        assert!(peak(&sounding[500..]) > 0.05, "the gate phrase never sounded");
        assert_eq!(log.held_notes(), vec![60, 67]);

        let after = run(&mut s, &[note_off(60, 0)], 8, 512);
        assert!(log.holds_nothing(), "the child is still holding {:?}", log.held_notes());
        assert!(!s.phrases_hold_notes(), "the engine still owes a note-off");
        assert_eq!(s.playing_phrases(), 0, "the runner survived its release");
        // The offs went out in the block the key came up in, so what is left
        // is the DC blocker's own ring-down and nothing else.
        assert!(peak(&after[2_000..]) < 1e-3, "it kept sounding: {}", peak(&after[2_000..]));
    }

    #[test]
    fn a_one_shot_phrase_ignores_the_key_coming_up() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        process(&mut s, &[note_off(60, 0)], 512);
        assert_eq!(s.playing_phrases(), 1, "a one-shot phrase obeyed a note-off");
        assert_eq!(log.held_notes(), vec![64]);
    }

    /// A capture cut short leaves a note-on with nothing behind it. The
    /// phrase ending has to give the note back anyway, or the child holds a
    /// key down for the rest of the session.
    #[test]
    fn a_phrase_that_ends_gives_back_a_note_it_was_never_told_to_release() {
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[dangling_note(64, 1_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(log.held_notes(), vec![64]);
        process(&mut s, &[], 2_048);
        assert_eq!(s.playing_phrases(), 0, "the runner never ended");
        assert!(log.holds_nothing(), "a phrase with no note-off hung the child");
        assert!(!s.phrases_hold_notes());
    }

    #[test]
    fn poly_one_retrigger_cuts_the_old_phrase_before_the_new_one_plays() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(log.held_notes(), vec![64]);

        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 1, "poly 1 let two phrases run at once");
        assert_eq!(log.held_notes(), vec![64]);
        // And the order is what matters: the cut's note-off reached the
        // child before the retrigger's note-on, not after it.
        let statuses: Vec<u8> = log.entries().iter().map(|e| e.0).collect();
        assert_eq!(
            statuses,
            vec![0x90, 0x80, 0x90],
            "the cut did not release before the retrigger: {:?}",
            log.entries(),
        );
    }

    #[test]
    fn poly_counts_a_phrase_and_a_sample_as_one_hit() {
        let mut cfg = PadConfig::for_key(60);
        cfg.poly = 2;
        let (mut s, _log) = with_phrases(60, cfg, &[held_note(64, 100, 400_000)]);
        s.set_sampler_pad(39, &cfg, &[PadLayer::from_pcm(sine_pcm(0.3, 88_200))]);

        process(&mut s, &[note_on(60, 127, 0)], 256);
        process(&mut s, &[note_on(60, 127, 0)], 256);
        assert_eq!(s.playing_phrases(), 2);
        assert_eq!(s.active_voices(), 2);
        // The third hit cuts the first, phrase and sample together.
        process(&mut s, &[note_on(60, 127, 0)], 1_000);
        assert_eq!(s.playing_phrases(), 2, "poly 2 left three phrases running");
        assert_eq!(s.active_voices(), 2, "poly 2 left three samples running");
    }

    #[test]
    fn a_choke_stops_the_phrase_on_the_other_pad() {
        let mut open = PadConfig::for_key(46);
        open.choke = 1;
        let mut closed = PadConfig::for_key(42);
        closed.choke = 1;
        let (mut s, log) = with_phrases(46, open, &[held_note(64, 100, 400_000)]);
        s.set_sampler_pad(42 - PAD_BASE_NOTE, &closed, &[]);

        process(&mut s, &[note_on(46, 127, 0)], 512);
        assert_eq!(log.held_notes(), vec![64]);
        process(&mut s, &[note_on(42, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 0, "the phrase survived its choke");
        assert!(log.holds_nothing(), "a choked phrase left {:?} held", log.held_notes());
    }

    #[test]
    fn a_pad_outside_the_choke_group_keeps_its_phrase() {
        let mut hat = PadConfig::for_key(42);
        hat.choke = 1;
        let kick = PadConfig::for_key(36); // choke 0
        let (mut s, log) = with_phrases(36, kick, &[held_note(64, 100, 400_000)]);
        s.set_sampler_pad(42 - PAD_BASE_NOTE, &hat, &[]);
        process(&mut s, &[note_on(36, 127, 0)], 256);
        process(&mut s, &[note_on(42, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 1, "a phrase was choked by a group it is not in");
        assert_eq!(log.held_notes(), vec![64]);
    }

    #[test]
    fn all_sound_off_stops_every_phrase_and_reaches_the_child() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        let (after, _) = process(&mut s, &[cc(120, 0)], 4_410);
        assert_eq!(s.playing_phrases(), 0, "a phrase survived all-sound-off");
        assert!(log.holds_nothing());
        assert!(!s.phrases_hold_notes());
        // The panic itself was forwarded, so the child's own voices die even
        // if one is ringing on a note the sampler has already given back.
        assert!(
            log.entries().iter().any(|e| e.0 == 0xB0 && e.1 == 120),
            "the panic never reached the child: {:?}",
            log.entries(),
        );
        assert!(peak(&after[2_000..]) < 1e-3, "it kept sounding after the panic");
    }

    #[test]
    fn all_notes_off_releases_gate_phrases_and_leaves_one_shots_alone() {
        let mut gate = PadConfig::for_key(60);
        gate.trig = TrigMode::Gate;
        let (mut s, log) = with_phrases(60, gate, &[held_note(64, 100, 400_000)]);
        // A one-shot phrase on another pad, sharing the child.
        s.set_sampler_pad(62 - PAD_BASE_NOTE, &PadConfig::for_key(62), &[]);
        s.set_sampler_phrases(62 - PAD_BASE_NOTE, &[held_note(67, 100, 400_000)]);

        process(&mut s, &[note_on(60, 127, 0), note_on(62, 127, 0)], 512);
        assert_eq!(log.held_notes(), vec![64, 67]);
        process(&mut s, &[cc(123, 0)], 512);
        assert_eq!(s.playing_phrases(), 1, "all-notes-off took the one-shot too");
        assert_eq!(log.held_notes(), vec![67], "the gate phrase kept its note");
    }

    #[test]
    fn a_reset_leaves_the_child_silent_and_holding_nothing() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(log.held_notes(), vec![64]);

        s.reset();
        assert_eq!(s.playing_phrases(), 0);
        assert!(!s.phrases_hold_notes());
        assert_eq!(
            log.resets.load(Ordering::Relaxed),
            1,
            "the child was not reset with the sampler",
        );
        assert!(
            log.holds_nothing(),
            "the child came out of a reset holding {:?}",
            log.held_notes(),
        );
        let (l, _) = process(&mut s, &[], 2_048);
        assert_eq!(peak(&l), 0.0, "something sounded after a reset");
    }

    #[test]
    fn taking_the_child_away_mid_phrase_leaves_nothing_held() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 1);

        s.set_sampler_child(None);
        assert_eq!(s.playing_phrases(), 0, "a phrase kept running without an instrument");
        assert!(!s.phrases_hold_notes(), "the engine still owes notes to a child that is gone");
        // Past the DC blocker's ring-down — which outlives any sound by
        // design — there is nothing, however loud the phrase had been.
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 2_048);
        assert!(peak(&l[1_000..]) < 1e-3, "a phrase pad spoke with no child loaded");
        let before = log.entries().len();
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(log.entries().len(), before, "the discarded child was still being played");
    }

    /// Nine overlapping phrases against eight runners. The steal has to give
    /// the stolen runner's notes back before its slot is written over.
    #[test]
    fn a_stolen_runner_gives_its_notes_back_first() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let log = Arc::new(ChildLog::new());
        s.set_sampler_child(Some(Box::new(TestChild::new(&log))));
        for i in 0..9u8 {
            let note = 60 + i;
            let pad = note - PAD_BASE_NOTE;
            s.set_sampler_pad(pad, &PadConfig::for_key(note), &[]);
            s.set_sampler_phrases(pad, &[held_note(40 + i, 100, 400_000)]);
        }
        for i in 0..9u8 {
            process(&mut s, &[note_on(60 + i, 127, 0)], 128);
        }
        assert_eq!(s.playing_phrases(), PHRASE_RUNNERS, "the pool overflowed");
        // Eight notes held, not nine: the oldest gesture gave its note back
        // as its slot was taken.
        assert_eq!(log.held_notes().len(), PHRASE_RUNNERS, "the steal leaked a note");
        assert!(!log.held_notes().contains(&40), "the oldest phrase kept its note");
    }

    // ── Velocity ──

    #[test]
    fn velocity_ranges_gate_phrases() {
        let mut soft = held_note(64, 100, 400_000);
        soft.vel_lo = 0;
        soft.vel_hi = 64;
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[soft]);

        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 0, "a hard hit reached a soft-only phrase");
        assert!(log.entries().is_empty());

        process(&mut s, &[note_on(60, 40, 0)], 512);
        assert_eq!(s.playing_phrases(), 1, "a soft hit missed its own phrase");
    }

    #[test]
    fn a_muted_phrase_is_never_triggered() {
        let mut muted = held_note(64, 100, 400_000);
        muted.mute = true;
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[muted]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 0);
        assert!(log.entries().is_empty());
    }

    #[test]
    fn the_phrases_gain_scales_the_velocity_it_sends() {
        let mut half = held_note(64, 100, 400_000);
        half.gain = 0.5;
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[half]);
        // Velocity depth off, so the hit's own velocity does not colour the
        // measurement and the phrase's gain is the only scale in play.
        s.set_parameter(crate::sampler::P_VEL, 0.0);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(log.ons(), vec![(64, 50, 0)], "the gain did not scale the velocity");
    }

    #[test]
    fn a_phrase_turned_all_the_way_down_plays_nothing() {
        let mut silent = held_note(64, 100, 400_000);
        silent.gain = 0.0;
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[silent]);
        let out = run(&mut s, &[note_on(60, 127, 0)], 4, 512);
        assert!(log.ons().is_empty(), "a phrase at gain zero still played: {:?}", log.ons());
        assert_eq!(peak(&out), 0.0);
        assert!(!s.phrases_hold_notes(), "a note that was never sent is marked held");
    }

    #[test]
    fn the_keyboards_velocity_reaches_the_phrase() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        // Full depth: a soft hit plays the phrase softly, which is the
        // answer a sampled layer gives the same gesture.
        s.set_parameter(crate::sampler::P_VEL, 1.0);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        process(&mut s, &[cc(120, 0)], 512);
        process(&mut s, &[note_on(60, 40, 0)], 512);
        let vels: Vec<u8> = log.ons().into_iter().map(|e| e.1).collect();
        assert_eq!(vels.len(), 2);
        assert_eq!(vels[0], 100, "a full-velocity hit changed the recorded velocity");
        assert!(vels[1] < 40, "a soft hit played the phrase at {}", vels[1]);
        assert!(vels[1] > 0, "a soft hit silenced the phrase entirely");
    }

    // ── Delivery ──

    #[test]
    fn a_phrase_for_a_pad_off_the_key_bed_is_ignored() {
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[]);
        s.set_sampler_phrases(200, &[held_note(64, 100, 400_000)]);
        s.set_sampler_phrases(u8::MAX, &[held_note(64, 100, 400_000)]);
        for note in [21u8, 60, 108] {
            process(&mut s, &[note_on(note, 127, 0)], 256);
        }
        assert_eq!(s.playing_phrases(), 0);
        assert!(log.entries().is_empty());
    }

    #[test]
    fn a_phrase_without_a_child_takes_no_runner() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        s.set_sampler_pad(39, &PadConfig::for_key(60), &[]);
        s.set_sampler_phrases(39, &[held_note(64, 100, 400_000)]);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_024);
        assert_eq!(s.playing_phrases(), 0, "a phrase ran with no instrument to run on");
        assert_eq!(peak(&l), 0.0);
    }

    /// The child is the phrase layer's instrument, not the sampler's: the
    /// keys the player presses go to the pads and stop there.
    #[test]
    fn the_child_never_hears_the_samplers_own_notes() {
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[]);
        s.set_sampler_pad(
            39,
            &PadConfig::for_key(60),
            &[PadLayer::from_pcm(sine_pcm(0.5, 44_100))],
        );
        process(&mut s, &[note_on(60, 127, 0), note_off(60, 100)], 512);
        process(&mut s, &[cc(123, 0)], 512);
        assert!(
            log.entries().is_empty(),
            "the sampler's own keyboard reached the child: {:?}",
            log.entries(),
        );
    }

    #[test]
    fn replacing_a_pads_phrases_lets_the_one_playing_finish() {
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 400_000)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        // The pad is emptied while the phrase plays: the runner holds its
        // own reference, so it carries on and gives its note back at its own
        // end rather than being cut off mid-performance.
        s.set_sampler_phrases(39, &[]);
        process(&mut s, &[], 512);
        assert_eq!(s.playing_phrases(), 1, "the edit cut a playing phrase");
        assert_eq!(log.held_notes(), vec![64]);
        // But a fresh hit on the emptied pad starts nothing.
        process(&mut s, &[cc(120, 0)], 512);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(s.playing_phrases(), 0, "an emptied pad still has phrases");
    }

    /// The capture side delivers events in time order. If one ever does not,
    /// the straggler is late rather than lost, and nothing hangs.
    #[test]
    fn an_out_of_order_event_list_neither_hangs_nor_panics() {
        let events = vec![
            PhraseEvent { frame: 5_000, status: 0x90, data1: 64, data2: 100 },
            PhraseEvent { frame: 0, status: 0x90, data1: 67, data2: 100 },
            PhraseEvent { frame: 6_000, status: 0x80, data1: 64, data2: 0 },
            PhraseEvent { frame: 10, status: 0x80, data1: 67, data2: 0 },
        ];
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[PadPhrase::from_events(Arc::from(events), 8_000)]);
        let out = run(&mut s, &[note_on(60, 127, 0)], 40, 512);
        assert!(out.iter().all(|v| v.is_finite()));
        assert_eq!(s.playing_phrases(), 0, "an unsorted phrase never ended");
        assert!(log.holds_nothing(), "an unsorted phrase hung {:?}", log.held_notes());
        assert!(!s.phrases_hold_notes());
    }

    #[test]
    fn a_phrase_is_at_least_as_long_as_its_last_note() {
        // A claimed length of 1 against a note-off at frame 5 000: the
        // runner has to still be there to deliver the off.
        let events = vec![
            PhraseEvent { frame: 0, status: 0x90, data1: 64, data2: 100 },
            PhraseEvent { frame: 5_000, status: 0x80, data1: 64, data2: 0 },
        ];
        let (mut s, log) =
            with_phrases(60, PadConfig::for_key(60), &[PadPhrase::from_events(Arc::from(events), 1)]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(log.held_notes(), vec![64], "the phrase ended before it began");
        process(&mut s, &[], 8_192);
        assert!(log.holds_nothing());
        assert_eq!(s.playing_phrases(), 0);
    }

    /// The block's event list is finite. Saturating it must drop events
    /// rather than allocate, and must never leave a note down: what could
    /// not be handed over this block is handed over in a later one.
    #[test]
    fn a_saturated_block_hands_its_note_offs_over_afterwards() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let log = Arc::new(ChildLog::new());
        s.set_sampler_child(Some(Box::new(TestChild::new(&log))));
        // Seven pads, each with a phrase that puts forty notes down at once
        // and never lifts them: 280 note-ons against a cap of 256.
        for pad_no in 0..7u8 {
            let note = 60 + pad_no;
            let events: Vec<PhraseEvent> = (0..40u8)
                .map(|i| PhraseEvent {
                    frame: 0,
                    status: 0x90,
                    data1: 20 + pad_no * 8 + i % 8,
                    data2: 100,
                })
                .collect();
            let pad = note - PAD_BASE_NOTE;
            s.set_sampler_pad(pad, &PadConfig::for_key(note), &[]);
            s.set_sampler_phrases(pad, &[PadPhrase::from_events(Arc::from(events), 400_000)]);
        }
        let hits: Vec<MidiEvent> = (0..7u8).map(|i| note_on(60 + i, 127, 0)).collect();
        process(&mut s, &hits, 512);
        assert_eq!(s.playing_phrases(), 7);

        // Everything stops at once, which owes more note-offs than one
        // block's list can carry.
        process(&mut s, &[cc(120, 0)], 512);
        for _ in 0..4 {
            process(&mut s, &[], 512);
        }
        assert!(!s.phrases_hold_notes(), "a saturated block left a note owing");
        assert_eq!(s.playing_phrases(), 0);
        assert!(log.holds_nothing(), "the child is still holding {:?}", log.held_notes());
    }

    /// A corrupt or absurd take — a thousand notes on one frame — must cost
    /// a bounded amount of work per block rather than being walked end to
    /// end inside a single sample, and must still arrive in full.
    #[test]
    fn a_thousand_notes_on_one_frame_are_spread_over_blocks_and_none_are_lost() {
        let events: Vec<PhraseEvent> = (0..1_000u32)
            .map(|i| PhraseEvent {
                frame: 0,
                status: 0x90,
                data1: (i % 128) as u8,
                data2: 100,
            })
            .collect();
        let (mut s, log) = with_phrases(
            60,
            PadConfig::for_key(60),
            &[PadPhrase::from_events(Arc::from(events), 2_000)],
        );

        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert!(
            log.count() <= MAX_CHILD_EVENTS,
            "one block handed over {} events, past its budget of {MAX_CHILD_EVENTS}",
            log.count(),
        );
        assert_eq!(s.playing_phrases(), 1, "the phrase gave up instead of waiting");

        // Given blocks enough, every one of them lands.
        for _ in 0..16 {
            process(&mut s, &[], 512);
        }
        assert!(log.count() >= 1_000, "only {} of a thousand notes arrived", log.count());
        assert_eq!(s.playing_phrases(), 0, "the phrase never ended");
        assert!(log.holds_nothing(), "the storm left {:?} held", log.held_notes());
        assert!(!s.phrases_hold_notes());
    }

    // ── Real time ──

    #[test]
    fn process_never_allocates_with_phrases_running() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let log = Arc::new(ChildLog::new());
        s.set_sampler_child(Some(Box::new(TestChild::new(&log))));
        for i in 0..8u8 {
            let note = 60 + i;
            let pad = note - PAD_BASE_NOTE;
            let mut cfg = PadConfig::for_key(note);
            cfg.poly = 2;
            cfg.choke = 1;
            s.set_sampler_pad(pad, &cfg, &[]);
            s.set_sampler_phrases(
                pad,
                &[held_note(40 + i, 100, 2_000), held_note(50 + i, 90, 3_000)],
            );
        }
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        let hits: Vec<MidiEvent> =
            (0..8u8).map(|i| note_on(60 + i, 127, u32::from(i) * 16)).collect();
        let offs: Vec<MidiEvent> = (0..8u8).map(|i| note_off(60 + i, 256)).collect();
        // Warm the path: the first block through a runner is the same code
        // as the hundredth, but the buffers must already be sized.
        {
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &hits);
        }

        let allocations = allocations_during(|| {
            for _ in 0..8 {
                let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                s.process(&[], &mut outs, &hits);
                let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                s.process(&[], &mut outs, &offs);
                let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                s.process(&[], &mut outs, &[]);
            }
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &[cc(120, 0)]);
        });
        assert_eq!(allocations, 0, "a running phrase reached the allocator");
    }

    #[test]
    fn delivering_phrases_on_the_audio_thread_does_not_allocate() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let phrases: Vec<PadPhrase> =
            (0..MAX_PHRASES).map(|i| held_note(60 + i as u8, 100, 5_000)).collect();
        // Slots occupied first, so the replacement drops as well as clones.
        s.set_sampler_phrases(39, &phrases);
        let allocations = allocations_during(|| {
            s.set_sampler_phrases(39, &phrases);
            s.set_sampler_phrases(39, &[]);
        });
        assert_eq!(allocations, 0, "a phrase edit reached the allocator");
    }

    #[test]
    fn an_idle_child_is_left_alone_and_wakes_for_the_next_phrase() {
        let (mut s, log) = with_phrases(60, PadConfig::for_key(60), &[held_note(64, 100, 1_000)]);
        // Long past the phrase, and past the idle window.
        run(&mut s, &[note_on(60, 127, 0)], 4, 22_050);
        assert_eq!(s.playing_phrases(), 0);
        let slept = log.entries().len();

        // A new hit wakes it: the skip is an optimisation, never a mute.
        let out = run(&mut s, &[note_on(60, 127, 0)], 2, 512);
        assert!(log.entries().len() > slept, "the child never woke up");
        assert!(peak(&out) > 0.05, "the woken child made no sound: {}", peak(&out));
    }
}
