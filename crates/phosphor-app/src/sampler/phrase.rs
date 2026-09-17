//! Phrase layers on this side of the wall: a performance kept as notes
//! instead of audio, and the one child instrument every one of them plays
//! through.
//!
//! The memory bargain is the whole point. A four-bar take rendered to audio
//! is a few megabytes; the same four bars kept as a phrase is a few hundred
//! events and an instrument that was going to be loaded anyway. What it
//! costs is that a phrase is not a sound of its own — it is note traffic for
//! an instrument shared by every phrase on the kit — and three of the
//! sampler's controls fall away with that:
//!
//! * the pad's level, pan and envelope do not reach a phrase, because there
//!   is no per-phrase place in a shared render to put them;
//! * a phrase's own loudness control scales the velocities it sends rather
//!   than a fader, which is why the panel calls it `vel` and reads it as a
//!   percentage;
//! * there is one child per sampler, so recording a phrase from a second
//!   instrument replaces it — and the flash says so, because a silent
//!   change to how every phrase on the kit sounds is the kind of thing a
//!   player finds out about a week later.
//!
//! # Tempo is baked
//!
//! Event offsets are engine frames, decided when the performance was
//! captured — the same as an audio take, and for the same reason: a phrase
//! is a recording, and the sibling it has to sound like is the recording on
//! the pad beside it. A phrase does not follow a tempo change.
//!
//! # Ownership
//!
//! [`PhraseState::events`] is the `Arc` the engine is handed, and the rule
//! is [`SamplePcm`](phosphor_plugin::sample::SamplePcm)'s: this side — the
//! pad, or an undo step holding a pad it used to be on — keeps a reference
//! for as long as any plugin might, so every clone and drop on the audio
//! thread is refcount-only.

use std::sync::Arc;

use phosphor_plugin::sample::{PadPhrase, PhraseEvent};

pub use phosphor_dsp::sampler::MAX_PHRASES;

use super::{LayerState, PadState, SamplerState};

/// What `r` lands on a pad: audio, or the performance itself.
///
/// An enum rather than a flag because it is drawn in the banner and stored
/// in the session, and "true" is not a word a player or a file can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TakeKind {
    /// Render the performance through the instrument and stack the audio.
    /// The default, so every pad that has ever existed still behaves the
    /// way it did.
    #[default]
    Audio,
    /// Keep the performance as notes and play it back through the
    /// sampler's child instrument.
    Phrase,
}

impl TakeKind {
    /// What the banner calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Phrase => "phrase",
        }
    }

    /// The stable spelling a session stores. Audio writes nothing at all,
    /// so every pad saved before phrases existed reads back unchanged.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Audio => "",
            Self::Phrase => "phrase",
        }
    }

    /// A spelling this build does not know lands audio: a take the player
    /// can hear beats a take the player cannot explain.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            "phrase" => Self::Phrase,
            _ => Self::Audio,
        }
    }

    /// The other one — what `p` switches to.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Audio => Self::Phrase,
            Self::Phrase => Self::Audio,
        }
    }
}

/// A recorded performance stacked on a pad, plus what the UI knows that the
/// engine does not: what to call it.
#[derive(Debug, Clone)]
pub struct PhraseState {
    /// Display name — `phrase 1`, `phrase 2`, numbered by its place in the
    /// stack the way a take is.
    pub name: String,
    /// The performance. Shared with the engine as a refcount handle.
    pub events: Arc<[PhraseEvent]>,
    /// How long the pad plays before the phrase is over, in engine frames.
    /// Not the same as the frame of its last note-off: a phrase can end in
    /// silence, and a bar-quantised one has to.
    pub frames: u64,
    /// The engine rate `frames` and the events were counted at, in Hz.
    ///
    /// Zero is unknown, which plays as-is — every phrase written before this
    /// existed says nothing about rates and opens exactly as it always has.
    /// Stamped at capture, so a performance recorded on a 48 kHz device and
    /// opened on a 44.1 kHz one plays at its own speed rather than 9% slow.
    pub sample_rate: f32,
    /// What every recorded velocity is multiplied by.
    pub gain: f32,
    /// When on, notes shift by the distance between the played key and the
    /// pad's root.
    pub transpose_with_key: bool,
    pub mute: bool,
    pub vel_lo: u8,
    pub vel_hi: u8,
}

impl PhraseState {
    /// A fresh phrase over the whole of `events`, at unity, answering every
    /// velocity and playing at the pitch it was recorded at.
    ///
    /// `sample_rate` is the engine rate the events were counted at; zero when
    /// that is not known, which plays them as-is.
    #[must_use]
    pub fn new(
        name: String,
        events: Arc<[PhraseEvent]>,
        frames: u64,
        sample_rate: f32,
    ) -> Self {
        Self {
            name,
            events,
            frames,
            sample_rate,
            gain: 1.0,
            transpose_with_key: false,
            mute: false,
            vel_lo: 0,
            vel_hi: 127,
        }
    }

    /// How long it plays, in seconds.
    ///
    /// Its own capture rate when it has one, because that is what the engine
    /// compensates to: a phrase counted in 48 kHz frames lasts the second it
    /// was played for, on any device. `rate` — the engine's — is the fallback
    /// for a phrase that never said what it was captured at, which is the
    /// only reading that was ever available before rates were stamped.
    #[must_use]
    pub fn seconds(&self, rate: f32) -> f32 {
        let counted_at = if self.sample_rate > 0.0 { self.sample_rate } else { rate };
        self.frames as f32 / counted_at.max(1.0)
    }

    /// How many notes are in it — what tells a phrase apart from the empty
    /// list a hand-edited session could put on a pad.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.events.iter().filter(|e| e.status & 0xF0 == 0x90 && e.data2 > 0).count()
    }

    /// The engine's view of this phrase.
    #[must_use]
    pub fn engine_phrase(&self) -> PadPhrase {
        PadPhrase {
            events: Arc::clone(&self.events),
            frames: self.frames,
            gain: self.gain,
            transpose_with_key: self.transpose_with_key,
            mute: self.mute,
            vel_lo: self.vel_lo,
            vel_hi: self.vel_hi,
            sample_rate: self.sample_rate,
        }
    }
}

/// Two phrases are the same phrase when they point at the same events and
/// carry the same settings.
///
/// Identity on the event list, never contents — [`LayerState`]'s rule, for
/// [`LayerState`]'s reason: undo compares slices of this state on every
/// commit, and a phrase is a list the pointer already answers for.
impl PartialEq for PhraseState {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.events, &other.events)
            && self.name == other.name
            && self.frames == other.frames
            && self.sample_rate == other.sample_rate
            && self.gain == other.gain
            && self.transpose_with_key == other.transpose_with_key
            && self.mute == other.mute
            && self.vel_lo == other.vel_lo
            && self.vel_hi == other.vel_hi
    }
}

/// What the sound list's cursor is standing on.
///
/// One list, two kinds of row: the audio layers first and the phrases after
/// them, because that is the order they were stacked in and because a
/// phrase is the newer idea. The kind decides which controls the panel
/// offers — see [`super::knobs::PadKnob::visible`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Layer,
    Phrase,
}

/// One row of the sound list, borrowed.
///
/// A borrow of one *or* the other, rather than two `Option`s a caller has to
/// check against each other: a control that reads a phrase can never be
/// handed a layer by mistake.
#[derive(Debug, Clone, Copy)]
pub enum PadRow<'a> {
    Layer(&'a LayerState),
    Phrase(&'a PhraseState),
}

impl<'a> PadRow<'a> {
    #[must_use]
    pub fn kind(self) -> RowKind {
        match self {
            Self::Layer(_) => RowKind::Layer,
            Self::Phrase(_) => RowKind::Phrase,
        }
    }

    /// What the list and the flashes call it.
    #[must_use]
    pub fn name(self) -> &'a str {
        match self {
            Self::Layer(l) => &l.name,
            Self::Phrase(p) => &p.name,
        }
    }
}

impl PadState {
    /// How many rows the sound list has: the layers and the phrases as one
    /// list, which is what `[`/`]` and `1`-`8` walk.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.layers.len() + self.phrases.len()
    }

    /// One row, borrowed, or `None` when the cursor is past the end.
    #[must_use]
    pub fn row(&self, row: usize) -> Option<PadRow<'_>> {
        match self.layers.get(row) {
            Some(layer) => Some(PadRow::Layer(layer)),
            None => self.phrases.get(row - self.layers.len()).map(PadRow::Phrase),
        }
    }

    /// What kind of row the cursor is on, if any.
    #[must_use]
    pub fn row_kind(&self, row: usize) -> Option<RowKind> {
        self.row(row).map(PadRow::kind)
    }

    /// The phrase a row addresses, for editing.
    pub fn phrase_at_row(&mut self, row: usize) -> Option<&mut PhraseState> {
        let index = row.checked_sub(self.layers.len())?;
        self.phrases.get_mut(index)
    }

    /// Phrases already here — what the next one is numbered after. Counted
    /// rather than stored, the take's rule: a phrase removed and undone
    /// back on must not leave two called `phrase 2`.
    #[must_use]
    pub fn phrase_count(&self) -> usize {
        self.phrases.len()
    }

    /// Keep one more performance, if the bed has room for it.
    ///
    /// `sample_rate` is the engine rate the frames were counted at, so the
    /// performance can be played back at the speed it was played; zero when
    /// the caller does not know, which plays it as-is.
    ///
    /// `title` is what a refusal calls this place — "pad C3", "zone C2-B3" —
    /// the same courtesy [`PadState::add_wav`] pays.
    pub fn add_phrase(
        &mut self,
        events: Arc<[PhraseEvent]>,
        frames: u64,
        sample_rate: f32,
        title: &str,
    ) -> Result<usize, String> {
        if self.phrases.len() >= MAX_PHRASES {
            return Err(SamplerState::phrase_full_message(title));
        }
        let name = format!("phrase {}", self.phrase_count() + 1);
        self.phrases.push(PhraseState::new(name, events, frames, sample_rate));
        Ok(self.phrases.len() - 1)
    }

    /// What a list calls this sound: the one thing on it by name, a count
    /// when there are several, a dash when there is nothing here yet.
    ///
    /// Phrases count, because a pad carrying nothing but a performance is
    /// not an empty pad and a list that said `—` would be pointing a player
    /// at the wrong repair.
    #[must_use]
    pub fn sound_label(&self) -> String {
        match (self.layers.len(), self.phrases.len()) {
            (0, 0) => "\u{2014}".into(),
            (1, 0) => self.layers[0].name.clone(),
            (0, 1) => self.phrases[0].name.clone(),
            (layers, 0) => format!("{layers} layers"),
            (0, phrases) => format!("{phrases} phrases"),
            (layers, phrases) => format!("{} sounds", layers + phrases),
        }
    }
}

impl SamplerState {
    /// Whether the thing under the cursor has room for one more phrase.
    ///
    /// Asked before the performance, never after it: finding out that a pad
    /// was full once the playing is over is losing the take.
    pub fn phrase_room_here(&self) -> Result<(), String> {
        match self.edited() {
            None => Err(Self::no_zone_message()),
            Some(state) if state.phrases.len() >= MAX_PHRASES => {
                Err(Self::phrase_full_message(&self.edit_title()))
            }
            Some(_) => Ok(()),
        }
    }

    /// Keep a performance on whatever the cursor is editing, and let it
    /// teach its root when it was played on one key.
    ///
    /// The root matters more here than it does for audio: a phrase's only
    /// transposition is the distance between the key played and the pad's
    /// root, so a phrase on a pad rooted somewhere else plays the wrong
    /// notes the moment the switch is turned on.
    pub fn add_phrase_here(
        &mut self,
        events: Arc<[PhraseEvent]>,
        frames: u64,
        sample_rate: f32,
        root: Option<u8>,
    ) -> Result<usize, String> {
        let title = self.edit_title();
        let index = self
            .edited_mut()
            .ok_or_else(Self::no_zone_message)?
            .add_phrase(events, frames, sample_rate, &title)?;
        if let Some(root) = root {
            self.set_edit_root(root);
        }
        Ok(index)
    }

    /// What every door says when there are no phrase slots left.
    #[must_use]
    pub fn phrase_full_message(title: &str) -> String {
        Self::bed_is_full(title, "four phrases")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::{MapMode, Zone};

    fn events(notes: &[(u64, u8)]) -> Arc<[PhraseEvent]> {
        let mut out = Vec::new();
        for &(frame, note) in notes {
            out.push(PhraseEvent { frame, status: 0x90, data1: note, data2: 100 });
            out.push(PhraseEvent { frame: frame + 100, status: 0x80, data1: note, data2: 0 });
        }
        Arc::from(out)
    }

    fn phrase(name: &str) -> PhraseState {
        PhraseState::new(name.into(), events(&[(0, 60)]), 44_100, 44_100.0)
    }

    #[test]
    fn a_fresh_phrase_answers_every_velocity_at_unity() {
        let p = phrase("phrase 1");
        assert_eq!(p.gain, 1.0);
        assert!(!p.transpose_with_key);
        assert!(!p.mute);
        assert_eq!((p.vel_lo, p.vel_hi), (0, 127));
        assert_eq!(p.note_count(), 1);
        assert_eq!(p.seconds(44_100.0), 1.0);
        // The engine's copy points at the same list rather than a copy of
        // it: the whole memory bargain is one allocation, shared.
        assert!(Arc::ptr_eq(&p.engine_phrase().events, &p.events));
    }

    /// A phrase lasts the time it was played for, on whatever device it is
    /// opened on: the frames are counted at the rate they were captured at,
    /// and the runner compensates. A phrase that never said what it was
    /// captured at falls back to the engine's rate, which is the only reading
    /// that was ever available for it.
    #[test]
    fn the_length_is_read_at_the_rate_it_was_captured_at() {
        let captured = PhraseState::new("phrase 1".into(), events(&[(0, 60)]), 48_000, 48_000.0);
        assert_eq!(captured.seconds(48_000.0), 1.0);
        assert_eq!(captured.seconds(44_100.0), 1.0, "the length followed the device");
        // A nonsense rate is a division nobody survives.
        assert!(captured.seconds(0.0).is_finite());

        let unknown = PhraseState::new("phrase 1".into(), events(&[(0, 60)]), 48_000, 0.0);
        assert_eq!(unknown.seconds(48_000.0), 1.0);
        assert!((unknown.seconds(44_100.0) - 1.088).abs() < 0.001);
        assert!(unknown.seconds(0.0).is_finite());
    }

    /// The rate travels to the engine with the phrase, survives the file,
    /// and is absent from a file that has none — the version rule every
    /// other field on a phrase already follows.
    #[test]
    fn a_phrase_carries_the_rate_it_was_captured_at() {
        let p = phrase("phrase 1");
        assert_eq!(p.engine_phrase().sample_rate, 44_100.0);
        // Two phrases off one recording differ when their rates differ: undo
        // compares these, and a rate change is a change to how it plays.
        let mut other = PhraseState::new("phrase 1".into(), Arc::clone(&p.events), 44_100, 0.0);
        assert_ne!(p, other);
        other.sample_rate = 44_100.0;
        assert_eq!(p, other);
    }

    #[test]
    fn the_fifth_phrase_is_refused_in_words() {
        let mut pad = PadState::empty(60);
        for i in 0..MAX_PHRASES {
            assert_eq!(pad.add_phrase(events(&[(0, 60)]), 100, 0.0, "pad C3").unwrap(), i);
        }
        let err = pad.add_phrase(events(&[(0, 60)]), 100, 0.0, "pad C3").unwrap_err();
        assert!(err.contains("four phrases"), "{err}");
        assert_eq!(pad.phrases.len(), MAX_PHRASES);
    }

    /// Numbered by what is on the pad, not by a counter that forgets — the
    /// take's rule, because undo puts things back.
    #[test]
    fn a_phrase_is_named_for_its_place_in_the_stack() {
        let mut pad = PadState::empty(60);
        pad.add_phrase(events(&[(0, 60)]), 100, 0.0, "pad C3").unwrap();
        pad.add_phrase(events(&[(0, 62)]), 100, 0.0, "pad C3").unwrap();
        let names: Vec<&str> = pad.phrases.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["phrase 1", "phrase 2"]);
        pad.phrases.remove(0);
        pad.add_phrase(events(&[(0, 64)]), 100, 0.0, "pad C3").unwrap();
        assert_eq!(pad.phrases[1].name, "phrase 2");
    }

    /// The sound list is one list: the layers, then the phrases, and a row
    /// past the end is `None` rather than the first phrase by accident.
    #[test]
    fn the_rows_are_the_layers_then_the_phrases() {
        let mut state = SamplerState::new();
        let pcm = std::sync::Arc::new(phosphor_plugin::sample::SamplePcm {
            data: vec![0.0; 8],
            channels: 1,
            sample_rate: 44_100.0,
        });
        state.add_wav_layer(0, std::path::PathBuf::from("kick.wav"), pcm).unwrap();
        state.pads[0].add_phrase(events(&[(0, 60)]), 100, 0.0, "pad").unwrap();
        state.pads[0].add_phrase(events(&[(0, 62)]), 100, 0.0, "pad").unwrap();

        let pad = &state.pads[0];
        assert_eq!(pad.rows(), 3);
        assert_eq!(pad.row_kind(0), Some(RowKind::Layer));
        assert_eq!(pad.row_kind(1), Some(RowKind::Phrase));
        assert_eq!(pad.row_kind(2), Some(RowKind::Phrase));
        assert_eq!(pad.row_kind(3), None);
        assert_eq!(pad.row(0).unwrap().name(), "kick");
        assert_eq!(pad.row(2).unwrap().name(), "phrase 2");
        assert_eq!(pad.sound_label(), "3 sounds");
    }

    #[test]
    fn a_row_cursor_past_the_end_edits_nothing() {
        let mut pad = PadState::empty(60);
        pad.add_phrase(events(&[(0, 60)]), 100, 0.0, "pad").unwrap();
        assert!(pad.phrase_at_row(9).is_none());
        assert!(pad.phrase_at_row(0).is_some());
    }

    /// The list's word for what is on a pad, in every shape it comes in.
    #[test]
    fn a_pad_with_only_a_phrase_is_not_an_empty_pad() {
        let mut pad = PadState::empty(60);
        assert_eq!(pad.sound_label(), "\u{2014}");
        pad.add_phrase(events(&[(0, 60)]), 100, 0.0, "pad").unwrap();
        assert_eq!(pad.sound_label(), "phrase 1");
        pad.add_phrase(events(&[(0, 62)]), 100, 0.0, "pad").unwrap();
        assert_eq!(pad.sound_label(), "2 phrases");
    }

    /// A phrase teaches its root the way a take does, in both modes — and
    /// in keys mode it lands on the zone rather than on the hidden pad.
    #[test]
    fn a_phrase_lands_where_the_cursor_is_and_teaches_its_root() {
        let mut state = SamplerState::new();
        state.add_phrase_here(events(&[(0, 45)]), 1_000, 0.0, Some(45)).unwrap();
        assert_eq!(state.current().phrases.len(), 1);
        assert_eq!(state.current().config.root, 45);

        let mut keys = SamplerState::new();
        keys.mode = MapMode::Keys;
        // No zone under the cursor: the refusal names the three keys.
        let err = keys.add_phrase_here(events(&[(0, 45)]), 1_000, 0.0, None).unwrap_err();
        assert!(err.contains("no zone"), "{err}");
        keys.zones.push(Zone::new(0, 87, PadState::empty(60)));
        keys.add_phrase_here(events(&[(0, 50)]), 1_000, 0.0, Some(50)).unwrap();
        assert_eq!(keys.zones[0].pad.phrases.len(), 1);
        assert_eq!(keys.zones[0].root(), 50, "the zone did not learn the root");
        assert!(keys.pads[keys.cursor].phrases.is_empty(), "it landed on the hidden pad");
    }

    #[test]
    fn a_full_pad_refuses_the_arm_in_the_same_words_as_the_landing() {
        let mut state = SamplerState::new();
        for _ in 0..MAX_PHRASES {
            state.add_phrase_here(events(&[(0, 60)]), 100, 0.0, None).unwrap();
        }
        let refusal = state.phrase_room_here().unwrap_err();
        assert!(refusal.contains("four phrases"), "{refusal}");
        assert_eq!(refusal, state.add_phrase_here(events(&[(0, 60)]), 100, 0.0, None).unwrap_err());
    }

    /// Two phrases off one recording are the same phrase; a second copy of
    /// the same notes is not. Undo compares these on every commit, and a
    /// comparison that walked the events would walk a performance.
    #[test]
    fn phrase_equality_is_the_event_lists_identity() {
        let shared = events(&[(0, 60)]);
        let a = PhraseState::new("phrase 1".into(), Arc::clone(&shared), 100, 0.0);
        let mut b = PhraseState::new("phrase 1".into(), shared, 100, 0.0);
        assert_eq!(a, b);
        b.gain = 0.5;
        assert_ne!(a, b);
        let elsewhere = PhraseState::new("phrase 1".into(), events(&[(0, 60)]), 100, 0.0);
        assert_ne!(a, elsewhere, "two decodes of one performance compared equal");
    }

    #[test]
    fn a_take_kind_reads_and_spells_itself() {
        assert_eq!(TakeKind::default(), TakeKind::Audio);
        assert_eq!(TakeKind::Audio.other(), TakeKind::Phrase);
        assert_eq!(TakeKind::Phrase.other(), TakeKind::Audio);
        assert_eq!(TakeKind::Audio.key(), "", "audio wrote itself into the file");
        assert_eq!(TakeKind::from_key("phrase"), TakeKind::Phrase);
        assert_eq!(TakeKind::from_key("sideways"), TakeKind::Audio);
        assert_eq!(TakeKind::Phrase.label(), "phrase");
    }
}
