//! Keys mode: the eighty-eight keys as chromatic zones.
//!
//! A zone is a stretch of keys playing one sound, transposed from a root —
//! the loop brace's shape, laid along the keyboard instead of along the
//! bar. It lives entirely on this side of the wall: the engine has never
//! heard of a zone and never will. What it is handed is eighty-eight pads,
//! the same as always, and in keys mode those pads are *materialized* from
//! the zones — every key inside a zone stamped with the zone's layers,
//! keytracking on, and the zone's root. The layers travel as the same
//! [`Arc`](std::sync::Arc)s, so a zone across the whole bed costs
//! eighty-eight pointer copies and not one sample. That is
//! [`crate::sampler`]'s first architecture decision paying for this whole
//! mode.
//!
//! # The two truths coexist
//!
//! Switching to keys mode does not destroy the pad map, and switching back
//! does not destroy the zones. Each mode decides what the engine hears, so
//! a switch has to resync the *union* of what either of them occupies: a
//! pad that has to go quiet is a pad the engine has to be told about, and
//! shipping only what sounds now would leave the other mode's sound on the
//! key with nothing on the screen to explain it. [`SamplerState::sounding_pads`]
//! is that union, and every caller that resyncs a mode switch or an undo
//! uses it.
//!
//! # Overlap
//!
//! Zones may overlap — `w` stretches one across the bed, and the bed may
//! already have others on it. Where they do, the *first* zone covering a
//! key owns that key's settings (trigger, poly, envelope, level) and the
//! later zones lend only their layers, because a key cannot have two
//! envelopes. A lent layer is retuned by the distance between the two
//! roots, so it still sounds the pitch that was played rather than the one
//! the first zone's root would have transposed it to. Past eight layers the
//! cap turns the rest away, which [`SamplerState::zone_overflow`] counts so
//! the edit that caused it can say so.

use std::borrow::Cow;

use super::{PadState, PhraseState, SamplerState, MAX_LAYERS, NUM_PADS};

/// What the bed means right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapMode {
    /// Every key is its own pad, with its own sound and its own settings.
    /// The default, so that every sampler built before zones existed is
    /// exactly what it was.
    #[default]
    Pads,
    /// The keys are zones: a stretch of them plays one sound, transposed
    /// from a root.
    Keys,
}

impl MapMode {
    /// What the bar and the chip call it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pads => "pads",
            Self::Keys => "keys",
        }
    }

    /// The stable spelling a session stores. Pads mode writes nothing at
    /// all — see [`crate::sampler::session`] — so only the other one has a
    /// word in the file.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Pads => "",
            Self::Keys => "keys",
        }
    }

    /// A spelling this build does not know is pads mode: the kit still
    /// plays, which beats a session that will not open.
    #[must_use]
    pub fn from_key(key: &str) -> Self {
        match key {
            "keys" => Self::Keys,
            _ => Self::Pads,
        }
    }

    /// The other one — what `K` switches to.
    #[must_use]
    pub fn other(self) -> Self {
        match self {
            Self::Pads => Self::Keys,
            Self::Keys => Self::Pads,
        }
    }
}

/// Which end of a zone a nudge moves.
///
/// Named rather than a bool because the keys that move them are `h`/`l` and
/// `H`/`L`, and a caller passing `true` for "the one shift is on" is a
/// caller nobody can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneEdge {
    Low,
    High,
}

impl ZoneEdge {
    /// What a flash calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
        }
    }
}

/// A stretch of keys playing one sound.
///
/// `lo` and `hi` are pad indices, both inclusive — the brace's two edges.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub lo: usize,
    pub hi: usize,
    /// The zone's own sound: the same [`PadState`] a pad carries, because a
    /// zone's sound *is* a pad — one set of knobs, one layer list, one
    /// trim strip, one everything.
    pub pad: PadState,
}

impl Zone {
    /// A zone over `lo..=hi`, with the edges put in order and pulled onto
    /// the bed. A caller that has the two the wrong way round has asked for
    /// the same stretch of keys.
    #[must_use]
    pub fn new(lo: usize, hi: usize, pad: PadState) -> Self {
        let top = NUM_PADS - 1;
        Self { lo: lo.min(hi).min(top), hi: hi.max(lo).min(top), pad }
    }

    /// The key the sound was recorded at: playing it sounds the sample
    /// untransposed, and every other key in the span is that distance away.
    ///
    /// It lives in the zone's own [`PadConfig`](phosphor_plugin::sample::PadConfig)
    /// rather than in a field beside it, because that is the field the
    /// `root` knob turns, a capture writes, a file name teaches and the
    /// session file already stores — four doors that would otherwise all
    /// need a second one cut for zones.
    ///
    /// The root does not have to be inside the span. A zone can sit
    /// entirely above or below the pitch it was sampled at, which is what
    /// happens the moment a zone is split below its own root.
    #[must_use]
    pub fn root(&self) -> u8 {
        self.pad.config.root
    }

    #[must_use]
    pub fn covers(&self, pad: usize) -> bool {
        (self.lo..=self.hi).contains(&pad)
    }

    /// How many keys it holds.
    #[must_use]
    pub fn keys(&self) -> usize {
        self.hi + 1 - self.lo
    }

    /// The span as a player reads it — "C2-B3", or one key's name when it
    /// is one key wide.
    #[must_use]
    pub fn span_label(&self) -> String {
        if self.lo == self.hi {
            return SamplerState::pad_label(self.lo);
        }
        format!("{}-{}", SamplerState::pad_label(self.lo), SamplerState::pad_label(self.hi))
    }

    /// What the zone list calls its sound — the pad's own word for it, so
    /// the zone list and the pad list can never disagree about what is on a
    /// sound.
    #[must_use]
    pub fn sound_label(&self) -> String {
        self.pad.sound_label()
    }
}

impl SamplerState {
    // ── Finding a zone ──

    /// The first zone covering a key, if any.
    ///
    /// The first, not "the" — zones may overlap, and the one with the
    /// lowest edge is the one that owns the key's settings and the one the
    /// panel edits. The list is kept in edge order, so "first" is also
    /// "leftmost", which is what a player pointing at the band means.
    #[must_use]
    pub fn zone_at(&self, pad: usize) -> Option<usize> {
        self.zones.iter().position(|z| z.covers(pad))
    }

    /// The zone under the cursor.
    #[must_use]
    pub fn cursor_zone(&self) -> Option<usize> {
        self.zone_at(self.cursor.min(NUM_PADS - 1))
    }

    /// The pad every control on the screen edits: the pad under the cursor
    /// in pads mode, the zone's own in keys mode.
    ///
    /// `None` only in keys mode, on a key no zone covers — there is nothing
    /// to edit there until one is made, and a panel that answered with the
    /// hidden pads-mode pad would be taking edits for a sound the player
    /// cannot hear.
    #[must_use]
    pub fn edited(&self) -> Option<&PadState> {
        match self.mode {
            MapMode::Pads => Some(self.current()),
            MapMode::Keys => self.cursor_zone().map(|i| &self.zones[i].pad),
        }
    }

    pub fn edited_mut(&mut self) -> Option<&mut PadState> {
        match self.mode {
            MapMode::Pads => Some(self.current_mut()),
            MapMode::Keys => self.cursor_zone().map(|i| &mut self.zones[i].pad),
        }
    }

    /// The keys an edit under the cursor is heard on — what a sync ships
    /// after it. One key in pads mode, the whole span in keys mode.
    #[must_use]
    pub fn edit_span(&self) -> Option<(usize, usize)> {
        match self.mode {
            MapMode::Pads => Some((self.cursor.min(NUM_PADS - 1), self.cursor.min(NUM_PADS - 1))),
            MapMode::Keys => self.cursor_zone().map(|i| (self.zones[i].lo, self.zones[i].hi)),
        }
    }

    /// What a flash calls the thing being edited: the key in pads mode, the
    /// span in keys mode, and the key again when keys mode has no zone
    /// under the cursor — a refusal still has to name where the player is
    /// standing.
    #[must_use]
    pub fn edit_label(&self) -> String {
        match self.mode {
            MapMode::Keys => match self.cursor_zone() {
                Some(i) => self.zones[i].span_label(),
                None => Self::pad_label(self.cursor),
            },
            MapMode::Pads => Self::pad_label(self.cursor),
        }
    }

    /// Whether the sound under the cursor transposes with the keyboard.
    ///
    /// A zone always does; a pad only when its own switch is on. It is what
    /// decides whether a root is worth learning from a file name: on a
    /// sound that plays the same on every key, a root is a number nobody
    /// can hear and guessing it from a string is a change the player did
    /// not ask for.
    #[must_use]
    pub fn edit_keytracks(&self) -> bool {
        match self.mode {
            MapMode::Keys => self.cursor_zone().is_some(),
            MapMode::Pads => self.current().config.keytrack,
        }
    }

    /// The same with its noun in front — "pad C3", "zone C2-B3". What a
    /// flash opens with, so that the word for the thing being edited always
    /// matches the mode the keys are in.
    #[must_use]
    pub fn edit_title(&self) -> String {
        match self.mode {
            MapMode::Pads => Self::pad_title(self.cursor),
            MapMode::Keys => format!("zone {}", self.edit_label()),
        }
    }

    // ── Materialization ──

    /// What a key plays right now.
    ///
    /// The pad itself in pads mode — borrowed, because that is the truth
    /// already. In keys mode it is built from the zones standing over the
    /// key: the first one's settings, keytracking forced on, its root, and
    /// every covering zone's layers stacked in edge order. A key no zone
    /// covers plays nothing, which the engine has to be told in the same
    /// words as anything else.
    #[must_use]
    pub fn voice(&self, pad: usize) -> Cow<'_, PadState> {
        match self.mode {
            MapMode::Pads => match self.pads.get(pad) {
                Some(state) => Cow::Borrowed(state),
                None => Cow::Owned(PadState::empty(Self::note_of_pad(pad))),
            },
            MapMode::Keys => Cow::Owned(self.stacked(pad)),
        }
    }

    /// The zones over one key, flattened into the pad the engine plays.
    fn stacked(&self, pad: usize) -> PadState {
        let note = Self::note_of_pad(pad);
        let Some(first) = self.zone_at(pad) else {
            return PadState::empty(note);
        };
        let owner = &self.zones[first];
        // Keytracking is not a decision here the way it is on a pad: a zone
        // that did not transpose would be eighty-eight keys playing one
        // pitch, which is a pad map with extra steps. The root comes with
        // the config, because the zone's root *is* its pad's root.
        let mut config = owner.pad.config;
        config.keytrack = true;
        // A zone's phrases transpose for the reason its layers do: the
        // engine shifts a phrase by the played key against the pad's root,
        // and the root here is the zone's own. A phrase that did not would
        // be one performance at one pitch across the whole span.
        //
        // The owner's only. A lent *layer* is retuned by the distance
        // between the two roots and a phrase has no field for that — its
        // only transposition is the one shift, measured from this key's
        // root — so a borrowed phrase would play the overlap in the wrong
        // key. It stays home instead. See SAMPLER.md's M8 line.
        let phrases = owner
            .pad
            .phrases
            .iter()
            .map(|p| PhraseState { transpose_with_key: true, ..p.clone() })
            .collect();
        let mut layers = owner.pad.layers.clone();
        'lending: for zone in self.zones.iter().skip(first + 1).filter(|z| z.covers(pad)) {
            // The lent layer is retuned by the distance between the two
            // roots. Without this it would be transposed from the owning
            // zone's root and sound the wrong note — the whole point of a
            // root being that it says what pitch the recording is at.
            let shift = i32::from(owner.root()) - i32::from(zone.root());
            for layer in &zone.pad.layers {
                if layers.len() >= MAX_LAYERS {
                    break 'lending;
                }
                let mut lent = layer.clone();
                // Clamped to the engine's own tune range, not the i8 the
                // field can hold: the engine clamps at ±48, and a wider
                // number here was a silent disagreement — the app promised
                // -87 and the pad played -48, thirty-nine semitones sharp
                // with nothing on the screen to say so. Two zones rooted
                // more than four octaves apart still meet this clamp; they
                // now at least clamp to the SAME pitch the engine plays.
                lent.tune_st =
                    (i32::from(lent.tune_st) + shift).clamp(-48, 48) as i8;
                layers.push(lent);
            }
        }
        PadState {
            config,
            layers,
            phrases,
            source: owner.pad.source.clone(),
            take: owner.pad.take,
        }
    }

    /// What a key is carrying: how many sounds stand over it, and whether
    /// any of them has lost its file.
    ///
    /// Sounds, not layers: a key carrying nothing but a performance is a
    /// key that plays, and a band that left it dark would be pointing the
    /// player at the wrong repair. In keys mode only the owning zone's
    /// phrases sound the key, which is what [`Self::voice`] materializes.
    ///
    /// Answered without building the pad, because the band asks it
    /// eighty-eight times a frame and materializing to find out would clone
    /// every name and path on the bed sixty times a second. It is the same
    /// count [`Self::voice`] would end up with, cap included.
    #[must_use]
    pub fn key_load(&self, pad: usize) -> (usize, bool) {
        match self.mode {
            MapMode::Pads => self.pads.get(pad).map_or((0, false), |state| {
                (state.layers.len() + state.phrases.len(), state.has_missing())
            }),
            MapMode::Keys => {
                let owner = self.zone_at(pad);
                let (mut layers, mut phrases, mut missing) = (0usize, 0usize, false);
                for (index, zone) in self.zones.iter().enumerate().filter(|(_, z)| z.covers(pad)) {
                    layers += zone.pad.layers.len();
                    if Some(index) == owner {
                        phrases = zone.pad.phrases.len();
                    }
                    missing |= zone.pad.has_missing();
                }
                (layers.min(MAX_LAYERS) + phrases, missing)
            }
        }
    }

    /// Whether a key is the root of a zone that covers it — the anchor
    /// mark on the band. A root outside its own zone's span is not marked:
    /// it is a number the zone is tuned from, not a key that plays it.
    #[must_use]
    pub fn is_zone_root(&self, pad: usize) -> bool {
        self.mode == MapMode::Keys
            && self
                .zones
                .iter()
                .any(|z| z.covers(pad) && z.root() == Self::note_of_pad(pad))
    }

    /// How many layers the eight-layer cap turns away on the key where
    /// overlapping zones stack deepest.
    ///
    /// The worst key rather than a total across the bed: the sentence a
    /// player can act on is "two of these do not fit", and a number that
    /// counted the same dropped layer once per key would be a number about
    /// the keyboard's width.
    #[must_use]
    pub fn zone_overflow(&self) -> usize {
        (0..NUM_PADS)
            .map(|pad| {
                self.zones
                    .iter()
                    .filter(|z| z.covers(pad))
                    .map(|z| z.pad.layers.len())
                    .sum::<usize>()
                    .saturating_sub(MAX_LAYERS)
            })
            .max()
            .unwrap_or(0)
    }

    /// Every key the engine might be holding a sound for, under either
    /// mode.
    ///
    /// What a mode switch and an undo have to resend. The union, because
    /// the engine is holding whatever it was last told: a pad occupied only
    /// in the mode being left has to be told it is empty now, or it plays
    /// on with nothing on the screen to explain it.
    #[must_use]
    pub fn sounding_pads(&self) -> Vec<usize> {
        let mut seen = [false; NUM_PADS];
        for pad in self.occupied_pads() {
            seen[pad] = true;
        }
        for zone in &self.zones {
            seen[zone.lo..=zone.hi.min(NUM_PADS - 1)].fill(true);
        }
        seen.iter().enumerate().filter_map(|(pad, on)| on.then_some(pad)).collect()
    }

    // ── Editing the zone list ──

    /// Switch the bed between pads and keys. Nothing is destroyed either
    /// way; the caller resyncs [`SamplerState::sounding_pads`] afterwards.
    pub fn set_mode(&mut self, mode: MapMode) {
        self.mode = mode;
    }

    /// `w` and `o`: a zone over `lo..=hi`, from the cursor.
    ///
    /// Inside a zone already, the two keys *resize* that zone rather than
    /// laying a second one over it — the brace is the thing being moved, so
    /// the whole bed and one octave are two places to throw it. On bare
    /// keys they make a new zone, seeded with the cursor pad's sound when
    /// it has one (`a` loads into it when it does not) and rooted where
    /// that pad is rooted, which for a pad nothing has taught is its own
    /// key.
    ///
    /// Returns the keys that have to be resynced: both the span it left and
    /// the one it landed on.
    pub fn zone_span(&mut self, lo: usize, hi: usize) -> (usize, usize) {
        let top = NUM_PADS - 1;
        let (lo, hi) = (lo.min(top), hi.min(top));
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        match self.cursor_zone() {
            Some(index) => {
                let was = (self.zones[index].lo, self.zones[index].hi);
                self.zones[index].lo = lo;
                self.zones[index].hi = hi;
                self.sort_zones();
                (was.0.min(lo), was.1.max(hi))
            }
            None => {
                // The seed carries its own root with it: a pad nothing has
                // taught is rooted at its own key, and one a take or a file
                // name has taught keeps what it learned.
                let seed = self.pads[self.cursor.min(top)].clone();
                self.zones.push(Zone::new(lo, hi, seed));
                self.sort_zones();
                (lo, hi)
            }
        }
    }

    /// `s`: split the zone under the cursor at the cursor key.
    ///
    /// The left half keeps the sound and the root it was tuned to — even
    /// when the split leaves that root on the other side, because a root is
    /// the pitch a recording was made at and not a key that has to be
    /// inside the span. The right half starts as a copy of the same sound
    /// (the same buffers, refcounted) rooted at its own first key, which is
    /// what makes a split useful: two halves of one instrument, each free
    /// to be retuned or reloaded.
    ///
    /// `Err` is a status-bar sentence and nothing moves.
    pub fn zone_split(&mut self) -> Result<(usize, usize), String> {
        let at = self.cursor.min(NUM_PADS - 1);
        let Some(index) = self.zone_at(at) else {
            return Err("no zone on this key \u{00b7} w covers the bed \u{00b7} o the octave".into());
        };
        if at == self.zones[index].lo {
            return Err(format!(
                "{} is the zone's first key \u{00b7} there is nothing to its left",
                Self::pad_label(at),
            ));
        }
        let mut pad = self.zones[index].pad.clone();
        pad.config.root = Self::note_of_pad(at);
        let right = Zone::new(at, self.zones[index].hi, pad);
        let span = (self.zones[index].lo, self.zones[index].hi);
        self.zones[index].hi = at - 1;
        self.zones.insert(index + 1, right);
        self.sort_zones();
        Ok(span)
    }

    /// `d`: take a zone off the bed, and say which keys went quiet.
    pub fn zone_remove(&mut self, index: usize) -> Option<(usize, usize)> {
        if index >= self.zones.len() {
            return None;
        }
        let zone = self.zones.remove(index);
        Some((zone.lo, zone.hi))
    }

    /// The brace: move one edge of a zone by `delta` keys.
    ///
    /// An edge stops where its neighbour begins and never pushes it — the
    /// simpler of the two rules, and the one a player can predict from the
    /// picture. Where two zones already overlap (a `w` stretched one across
    /// another) an edge can still move in the direction that reduces the
    /// overlap; it just cannot make it worse. The two edges never cross, so
    /// a zone is always at least one key wide.
    ///
    /// Returns the keys to resync — the span before and after together —
    /// or `None` when the edge was already against its stop.
    pub fn move_zone_edge(
        &mut self,
        index: usize,
        edge: ZoneEdge,
        delta: i32,
    ) -> Option<(usize, usize)> {
        let zone = self.zones.get(index)?;
        let (lo, hi) = (zone.lo, zone.hi);
        let moved = match edge {
            ZoneEdge::Low => {
                // The floor is the key after the zone below, never further
                // right than where this edge already is.
                let floor = index
                    .checked_sub(1)
                    .and_then(|below| self.zones.get(below))
                    .map_or(0, |below| below.hi + 1)
                    .min(lo);
                (lo as i32 + delta).clamp(floor as i32, hi as i32) as usize
            }
            ZoneEdge::High => {
                let ceiling = self
                    .zones
                    .get(index + 1)
                    .map_or(NUM_PADS - 1, |above| above.lo.saturating_sub(1))
                    .max(hi);
                (hi as i32 + delta).clamp(lo as i32, ceiling as i32) as usize
            }
        };
        let zone = self.zones.get_mut(index)?;
        match edge {
            ZoneEdge::Low if moved == lo => return None,
            ZoneEdge::High if moved == hi => return None,
            ZoneEdge::Low => zone.lo = moved,
            ZoneEdge::High => zone.hi = moved,
        }
        // The caret rides the edge it is pushing. Without this, moving an
        // edge past the key the caret is standing on leaves the caret
        // outside the zone it is editing — and the next press of the same
        // key is refused by a panel that has stopped showing the thing
        // being moved.
        if !self.zones[index].covers(self.cursor) {
            self.cursor = moved;
        }
        // An edge that has walked past a neighbour's has changed the order
        // the list is kept in, and "the zone under the caret" is the
        // leftmost one covering it.
        self.sort_zones();
        Some((lo.min(moved), hi.max(moved)))
    }

    /// Keep the list in edge order, so that "the zone under the cursor" is
    /// the leftmost one covering it and the zone list reads like a
    /// keyboard. Stable, so two zones starting on the same key keep the
    /// order they were made in — which is the order their layers stack in.
    pub(crate) fn sort_zones(&mut self) {
        self.zones.sort_by_key(|z| (z.lo, z.hi));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::{LayerState, SamplerState};
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn pcm() -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![0.25; 64], channels: 1, sample_rate: 44_100.0 })
    }

    /// A zone over `lo..=hi` carrying `count` layers off one buffer.
    fn zone(lo: u8, hi: u8, root: u8, count: usize) -> Zone {
        let mut pad = PadState::empty(root);
        for i in 0..count {
            pad.layers.push(LayerState::from_wav(PathBuf::from(format!("z{i}.wav")), pcm()));
        }
        Zone::new(
            SamplerState::pad_of_note(lo).unwrap(),
            SamplerState::pad_of_note(hi).unwrap(),
            pad,
        )
    }

    fn keys_state(zones: Vec<Zone>) -> SamplerState {
        let mut state = SamplerState::new();
        state.mode = MapMode::Keys;
        state.zones = zones;
        state
    }

    /// The memory story, asserted: every key of a zone plays the *same*
    /// buffers, keytracked from the zone's root. Eighty-eight keys of a
    /// ten-megabyte piano cost eighty-eight pointers.
    #[test]
    fn a_zone_stamps_every_key_with_the_same_buffers() {
        let state = keys_state(vec![zone(48, 71, 60, 1)]); // C2-B3, rooted C3
        assert_eq!(state.zones[0].span_label(), "C2-B3");
        let original = state.zones[0].pad.layers[0].pcm.clone().unwrap();
        for note in [48u8, 60, 71] {
            let pad = SamplerState::pad_of_note(note).unwrap();
            let voice = state.voice(pad);
            assert!(voice.config.keytrack, "{note} does not track the keyboard");
            assert_eq!(voice.config.root, 60, "{note} is rooted somewhere else");
            assert_eq!(voice.layers.len(), 1);
            assert!(
                Arc::ptr_eq(voice.layers[0].pcm.as_ref().unwrap(), &original),
                "key {note} copied the audio instead of pointing at it",
            );
        }
        // A key outside the span plays nothing at all.
        for note in [47u8, 72, 108] {
            let pad = SamplerState::pad_of_note(note).unwrap();
            assert!(state.voice(pad).layers.is_empty(), "{note} is outside every zone");
        }
    }

    /// The pad map is not destroyed by keys mode and the zones are not
    /// destroyed by pads mode: each says what the engine hears, and the
    /// other one is still there when the switch comes back.
    #[test]
    fn the_two_truths_coexist_across_a_switch() {
        let mut state = keys_state(vec![zone(36, 47, 36, 1)]);
        let pad = SamplerState::pad_of_note(72).unwrap();
        state.add_wav_layer(pad, PathBuf::from("hat.wav"), pcm()).unwrap();

        // In keys mode the pad's own sound is inaudible but intact.
        assert!(state.voice(pad).layers.is_empty(), "a pads-mode sound leaked into keys mode");
        assert_eq!(state.pads[pad].layers.len(), 1, "keys mode ate the pad");

        state.set_mode(MapMode::Pads);
        assert_eq!(state.voice(pad).layers.len(), 1, "the pad did not come back");
        let zoned = SamplerState::pad_of_note(40).unwrap();
        assert!(state.voice(zoned).layers.is_empty(), "a zone leaked into pads mode");
        assert_eq!(state.zones.len(), 1, "pads mode ate the zones");

        // And the union names both, which is what has to be resynced.
        let sounding = state.sounding_pads();
        assert!(sounding.contains(&pad), "the pad is not in the union");
        assert!(sounding.contains(&zoned), "the zone's keys are not in the union");
    }

    /// Overlapping zones stack in edge order, the ninth layer is turned
    /// away, and the overflow is counted so the edit can say so.
    #[test]
    fn overlapping_zones_stack_in_order_and_stop_at_eight() {
        let state = keys_state(vec![zone(36, 59, 40, 5), zone(48, 71, 60, 5)]);
        let shared = SamplerState::pad_of_note(50).unwrap();
        let voice = state.voice(shared);
        assert_eq!(voice.layers.len(), MAX_LAYERS, "the cap did not hold");
        assert_eq!(voice.config.root, 40, "the later zone took the key's root");
        assert_eq!(state.zone_overflow(), 2, "the turned-away layers were not counted");

        // The lent layers are retuned by the distance between the roots, so
        // the borrowed sound still plays the pitch that was pressed.
        assert_eq!(voice.layers[0].tune_st, 0, "the owning zone's layer was retuned");
        assert_eq!(voice.layers[5].tune_st, 40 - 60, "the lent layer was not retuned");

        // Where they do not overlap, each zone is alone and untouched.
        assert_eq!(state.voice(SamplerState::pad_of_note(40).unwrap()).layers.len(), 5);
        assert_eq!(state.voice(SamplerState::pad_of_note(65).unwrap()).layers.len(), 5);
        assert_eq!(
            state.voice(SamplerState::pad_of_note(65).unwrap()).config.root,
            60,
            "the second zone lost its own root outside the overlap",
        );
    }

    /// A retune past the engine's ±48 clamps to the engine's number, not
    /// to the field's. Two clamps with two answers was the audit's C3:
    /// the app promised -87, the pad played -48, and nothing said so.
    #[test]
    fn an_extreme_retune_clamps_where_the_engine_clamps() {
        // Roots 87 semitones apart: A0-rooted zone lending to a C8 owner.
        let state = keys_state(vec![zone(21, 108, 108, 1), zone(21, 108, 21, 1)]);
        let shared = SamplerState::pad_of_note(60).unwrap();
        let voice = state.voice(shared);
        assert_eq!(voice.layers.len(), 2);
        // 108 - 21 = 87 wanted; the engine's range ends at 48, and the
        // materializer must promise no more than the pad will play.
        assert_eq!(voice.layers[1].tune_st, 48, "the app promised a pitch the engine clamps");
    }

    /// A zone's phrases travel to every key it covers, transposing with the
    /// keyboard — the engine shifts a phrase by the played key against the
    /// pad's root, and the materializer is what sets that root.
    #[test]
    fn a_zone_stamps_its_phrases_onto_every_key_it_covers() {
        let events: Arc<[phosphor_plugin::sample::PhraseEvent]> =
            Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
                frame: 0,
                status: 0x90,
                data1: 60,
                data2: 100,
            }]);
        let mut owner = zone(48, 71, 60, 0);
        owner.pad.add_phrase(Arc::clone(&events), 44_100, "zone").unwrap();
        // Transposition off on the zone's own copy: what the materializer
        // does with it is the thing under test.
        assert!(!owner.pad.phrases[0].transpose_with_key);
        let state = keys_state(vec![owner]);

        for note in [48u8, 60, 71] {
            let pad = SamplerState::pad_of_note(note).unwrap();
            let voice = state.voice(pad);
            assert_eq!(voice.phrases.len(), 1, "{note} lost the zone's phrase");
            assert!(voice.phrases[0].transpose_with_key, "{note} would play one pitch");
            assert_eq!(voice.config.root, 60, "{note} is rooted somewhere else");
            assert!(
                Arc::ptr_eq(&voice.phrases[0].events, &events),
                "key {note} copied the performance instead of pointing at it",
            );
            assert_eq!(state.key_load(pad).0, 1, "the band does not know the key plays");
        }
        // A key outside the span plays none of it.
        let outside = SamplerState::pad_of_note(72).unwrap();
        assert!(state.voice(outside).phrases.is_empty());
        assert_eq!(state.key_load(outside).0, 0);
    }

    /// Where zones overlap, only the owning zone's phrases sound the key.
    ///
    /// A lent *layer* is retuned by the distance between the two roots, and
    /// a phrase has no field for that — its one shift is measured from the
    /// key's own root, which belongs to the owner. A borrowed phrase would
    /// play the overlap in the wrong key, so it stays home.
    #[test]
    fn a_lent_zone_lends_its_layers_and_keeps_its_phrases() {
        let events: Arc<[phosphor_plugin::sample::PhraseEvent]> =
            Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
                frame: 0,
                status: 0x90,
                data1: 60,
                data2: 100,
            }]);
        let mut low = zone(36, 59, 40, 1);
        low.pad.add_phrase(Arc::clone(&events), 100, "zone").unwrap();
        let mut high = zone(48, 71, 60, 1);
        high.pad.add_phrase(events, 100, "zone").unwrap();
        let state = keys_state(vec![low, high]);

        let shared = SamplerState::pad_of_note(50).unwrap();
        let voice = state.voice(shared);
        assert_eq!(voice.layers.len(), 2, "the lent layer did not travel");
        assert_eq!(voice.phrases.len(), 1, "a lent phrase would play the wrong key");
        assert_eq!(voice.config.root, 40, "the later zone took the key's root");
        // Two layers and one phrase: the band counts all three.
        assert_eq!(state.key_load(shared), (3, false));

        // Outside the overlap each zone keeps its own.
        assert_eq!(state.voice(SamplerState::pad_of_note(65).unwrap()).phrases.len(), 1);
    }

    /// One zone and no overlap never counts an overflow, however full it is.
    #[test]
    fn a_full_zone_on_its_own_is_not_an_overflow() {
        let state = keys_state(vec![zone(36, 59, 60, MAX_LAYERS)]);
        assert_eq!(state.zone_overflow(), 0);
        assert_eq!(state.voice(SamplerState::pad_of_note(40).unwrap()).layers.len(), MAX_LAYERS);
    }

    /// `w` on bare keys makes a zone over the whole bed, seeded with the
    /// sound already under the cursor and rooted where that pad is rooted.
    #[test]
    fn whole_seeds_itself_from_the_pad_under_the_cursor() {
        let mut state = SamplerState::new();
        state.cursor = SamplerState::pad_of_note(45).unwrap();
        state.add_wav_layer(state.cursor, PathBuf::from("piano.wav"), pcm()).unwrap();
        state.pads[state.cursor].config.root = 45;
        state.mode = MapMode::Keys;

        let span = state.zone_span(0, NUM_PADS - 1);
        assert_eq!(span, (0, NUM_PADS - 1));
        assert_eq!(state.zones.len(), 1);
        assert_eq!(state.zones[0].root(), 45);
        assert_eq!(state.zones[0].pad.layers.len(), 1, "the zone did not take the pad's sound");
        // Every key of the bed now plays it, including both ends.
        assert_eq!(state.voice(0).layers.len(), 1);
        assert_eq!(state.voice(NUM_PADS - 1).layers.len(), 1);
    }

    /// Inside a zone, `w` and `o` move the brace rather than laying a
    /// second zone over it — and the resync span covers the keys it left as
    /// well as the ones it took.
    #[test]
    fn whole_and_octave_resize_the_zone_under_the_cursor() {
        let mut state = keys_state(vec![zone(60, 71, 60, 1)]);
        state.cursor = SamplerState::pad_of_note(64).unwrap();
        let span = state.zone_span(0, NUM_PADS - 1);
        assert_eq!(state.zones.len(), 1, "a second zone was laid over the first");
        assert_eq!((state.zones[0].lo, state.zones[0].hi), (0, NUM_PADS - 1));
        assert_eq!(span, (0, NUM_PADS - 1));

        // ...and back down to the octave the cursor is in.
        let octave = SamplerState::pad_of_note(60).unwrap();
        let span = state.zone_span(octave, octave + 11);
        assert_eq!((state.zones[0].lo, state.zones[0].hi), (octave, octave + 11));
        assert_eq!(span, (0, NUM_PADS - 1), "the keys the zone left were not resynced");
    }

    /// A split keeps the left half as it was and starts the right half as
    /// the same sound rooted at its own first key — including when the
    /// cursor is on the zone's own root, which leaves that root outside the
    /// span it belongs to.
    #[test]
    fn a_split_leaves_the_left_alone_and_roots_the_right_at_itself() {
        let mut state = keys_state(vec![zone(36, 71, 60, 1)]);
        state.cursor = SamplerState::pad_of_note(60).unwrap(); // the zone's own root
        let span = state.zone_split().unwrap();
        assert_eq!(span, (SamplerState::pad_of_note(36).unwrap(), SamplerState::pad_of_note(71).unwrap()));
        assert_eq!(state.zones.len(), 2);
        let (left, right) = (&state.zones[0], &state.zones[1]);
        assert_eq!(left.hi, state.cursor - 1, "the halves overlap");
        assert_eq!(right.lo, state.cursor);
        assert_eq!(left.root(), 60, "the left half lost the root it was tuned to");
        assert_eq!(right.root(), 60, "the right half is not rooted at its own first key");
        assert!(
            Arc::ptr_eq(
                left.pad.layers[0].pcm.as_ref().unwrap(),
                right.pad.layers[0].pcm.as_ref().unwrap(),
            ),
            "the split copied the audio",
        );

        // Splitting again, off the root this time.
        state.cursor = SamplerState::pad_of_note(67).unwrap();
        state.zone_split().unwrap();
        assert_eq!(state.zones.len(), 3);
        assert_eq!(state.zones[2].root(), 67);
    }

    /// The two refusals, in words, with nothing moved.
    #[test]
    fn a_split_with_nothing_to_split_says_so() {
        let mut state = keys_state(Vec::new());
        state.cursor = 40;
        assert!(state.zone_split().unwrap_err().contains("no zone"));
        assert!(state.zones.is_empty());

        let mut state = keys_state(vec![zone(36, 47, 36, 1)]);
        state.cursor = SamplerState::pad_of_note(36).unwrap();
        let err = state.zone_split().unwrap_err();
        assert!(err.contains("first key"), "{err}");
        assert_eq!(state.zones.len(), 1, "a refused split still cut the zone");
    }

    /// An edge stops at its neighbour rather than pushing it, and the two
    /// edges of one zone never cross.
    #[test]
    fn an_edge_stops_where_the_next_zone_begins() {
        let mut state = keys_state(vec![zone(36, 47, 36, 1), zone(48, 59, 48, 1)]);
        // The low zone's high edge walks up into the high zone and stops.
        for _ in 0..40 {
            state.move_zone_edge(0, ZoneEdge::High, 1);
        }
        assert_eq!(state.zones[0].hi, state.zones[1].lo - 1, "the edges ran together");
        assert!(state.move_zone_edge(0, ZoneEdge::High, 1).is_none(), "a stopped edge reported a move");

        // The same zone's low edge walks up to its own high edge and stops.
        for _ in 0..80 {
            state.move_zone_edge(0, ZoneEdge::Low, 1);
        }
        assert_eq!(state.zones[0].lo, state.zones[0].hi, "the brace inverted");
        // And down to the bottom of the bed.
        for _ in 0..200 {
            state.move_zone_edge(0, ZoneEdge::Low, -1);
        }
        assert_eq!(state.zones[0].lo, 0);
    }

    /// An edge that moves past the caret takes the caret with it: a brace
    /// whose zone the caret has fallen out of is a brace the next press of
    /// the same key is refused by.
    #[test]
    fn the_caret_rides_the_edge_it_is_pushing() {
        let mut state = keys_state(vec![zone(60, 71, 60, 1)]);
        state.cursor = SamplerState::pad_of_note(60).unwrap(); // the low edge
        for _ in 0..3 {
            state.move_zone_edge(0, ZoneEdge::Low, 1);
        }
        assert_eq!(state.cursor, state.zones[0].lo, "the caret was left behind");
        assert_eq!(state.cursor_zone(), Some(0), "the caret fell out of its own zone");

        // The high edge, the same way.
        state.cursor = state.zones[0].hi;
        state.move_zone_edge(0, ZoneEdge::High, -1);
        assert_eq!(state.cursor, state.zones[0].hi);

        // An edge moving *away* from the caret leaves it where it is.
        let was = state.cursor;
        state.move_zone_edge(0, ZoneEdge::High, 1);
        assert_eq!(state.cursor, was, "the caret followed an edge it was not on");
    }

    /// A moved edge names both the keys it gained and the keys it lost, so
    /// the ones that went quiet are resynced too.
    #[test]
    fn a_moved_edge_names_the_keys_it_left() {
        let mut state = keys_state(vec![zone(48, 59, 48, 1)]);
        let (lo, hi) = (state.zones[0].lo, state.zones[0].hi);
        let span = state.move_zone_edge(0, ZoneEdge::High, -1).unwrap();
        assert_eq!(span, (lo, hi), "the key that went quiet is not in the resync");
        let span = state.move_zone_edge(0, ZoneEdge::Low, -1).unwrap();
        assert_eq!(span, (lo - 1, hi - 1), "the key that was gained is not in the resync");
    }

    /// Overlapping zones can be pulled apart but not pushed further
    /// together: an edge already past its neighbour moves only the way that
    /// mends it.
    #[test]
    fn an_overlapping_edge_can_only_come_back() {
        let mut state = keys_state(vec![zone(36, 71, 36, 1), zone(48, 59, 48, 1)]);
        // The second zone's low edge is inside the first. It cannot go
        // further left...
        assert!(state.move_zone_edge(1, ZoneEdge::Low, -1).is_none());
        // ...and can come back to the right.
        assert!(state.move_zone_edge(1, ZoneEdge::Low, 1).is_some());
    }

    /// The list stays in edge order, so the zone under a key is the
    /// leftmost one covering it however the zones were made.
    #[test]
    fn the_zone_list_reads_like_a_keyboard() {
        let mut state = keys_state(Vec::new());
        state.cursor = SamplerState::pad_of_note(72).unwrap();
        state.zone_span(state.cursor, state.cursor + 11);
        state.cursor = SamplerState::pad_of_note(36).unwrap();
        state.zone_span(state.cursor, state.cursor + 11);
        let los: Vec<usize> = state.zones.iter().map(|z| z.lo).collect();
        assert_eq!(los, vec![SamplerState::pad_of_note(36).unwrap(), SamplerState::pad_of_note(72).unwrap()]);
        assert_eq!(state.zone_at(SamplerState::pad_of_note(40).unwrap()), Some(0));
    }

    /// Removing a zone says which keys went quiet, and asking for one that
    /// is not there is not a panic.
    #[test]
    fn removing_a_zone_names_the_keys_that_went_quiet() {
        let mut state = keys_state(vec![zone(36, 47, 36, 1)]);
        let span = state.zone_remove(0).unwrap();
        assert_eq!(span, (SamplerState::pad_of_note(36).unwrap(), SamplerState::pad_of_note(47).unwrap()));
        assert!(state.zones.is_empty());
        assert!(state.zone_remove(0).is_none());
    }

    /// What the panel and the flashes read, in both modes.
    #[test]
    fn the_edited_sound_follows_the_mode() {
        let mut state = SamplerState::new();
        state.cursor = SamplerState::pad_of_note(60).unwrap();
        state.add_wav_layer(state.cursor, PathBuf::from("kick.wav"), pcm()).unwrap();
        assert_eq!(state.edited().unwrap().layers.len(), 1);
        assert_eq!(state.edit_label(), "C3");
        assert_eq!(state.edit_span(), Some((state.cursor, state.cursor)));

        state.mode = MapMode::Keys;
        assert!(state.edited().is_none(), "a bare key offered the hidden pad to edit");
        assert_eq!(state.edit_label(), "C3", "a refusal has to name where the cursor is");
        assert!(state.edit_span().is_none());

        state.zones.push(zone(48, 71, 48, 2));
        assert_eq!(state.edited().unwrap().layers.len(), 2);
        assert_eq!(state.edit_label(), "C2-B3");
        assert_eq!(
            state.edit_span(),
            Some((SamplerState::pad_of_note(48).unwrap(), SamplerState::pad_of_note(71).unwrap())),
        );
    }

    /// A one-key zone reads as that key rather than as "C3-C3".
    #[test]
    fn a_zone_one_key_wide_reads_as_the_key() {
        assert_eq!(zone(60, 60, 60, 0).span_label(), "C3");
        assert_eq!(zone(60, 72, 60, 0).span_label(), "C3-C4");
    }
}
