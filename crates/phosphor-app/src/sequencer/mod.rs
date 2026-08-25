//! The step sequencer, on the UI side of the fence.
//!
//! The audio thread's half of this lives in [`phosphor_core::pattern`]: fixed
//! -size patterns, position-derived step timing, and the generator that turns
//! them into notes. This half is what a player edits — eight pattern slots, a
//! chain, a cursor, a mode — and what a session stores.
//!
//! # A sequencer track is an instrument track
//!
//! There is no sequencer plugin and no sequencer audio path. A sequencer
//! track holds an ordinary instrument in its ordinary plugin slot — the
//! *child* — and a pattern player in front of it, feeding it the notes a
//! keyboard would otherwise feed it. That is why [`InstrumentType::Sequencer`]
//! is a choice in the add-track menu but never a value stored on a track: the
//! track's `instrument_type` is its child, so the child's panel, its preset
//! bank, its selectors and its saved parameters all keep working with no code
//! that knows a sequencer exists. What marks the track is
//! [`crate::state::TrackState::sequencer`] being `Some`.
//!
//! # One mutation surface
//!
//! Nothing in this module is edited by reaching into a field. Every change
//! goes through [`ops::SeqOp`] and [`ops::dispatch`], which exists so that the
//! same edits can be driven by keys today and by a MIDI controller later
//! without two implementations of "toggle the step under the cursor" drifting
//! apart. See [`ops`].
//!
//! # What is track-level and what is pattern-level
//!
//! Five settings describe the *track* rather than a pattern — whether it is
//! running, the queued slot, the switch quantization, and the chain — and they
//! are held once here and stamped onto every block on its way out, by
//! [`SequencerState::block`]. The audio thread takes them from whichever block
//! arrived last, so changing one costs a single command rather than eight.
//!
//! Everything else, the mode and tonic included, belongs to a pattern. That is
//! a small widening of what was specified: it makes A in Dorian and B in
//! Aeolian possible, and it removes the only case where changing one setting
//! would have had to rewrite all eight slots to stay consistent.

pub mod chords;
pub mod compile;
pub mod ops;

use serde::{Deserialize, Serialize};

use phosphor_core::pattern::{
    ChainEntry, Lane, Mode, PatternBlock, Rate, Step, SwitchQuant, LANES, MAX_CHAIN, MAX_STEPS,
    SLOTS,
};

use crate::session::{apply_selectors, instrument_key, parse_instrument_type, SessionSelector};
use crate::state::InstrumentType;

/// What a new sequencer track drives until told otherwise.
///
/// A drum machine, because a step grid is a drum machine's native form and
/// because the eight lanes are immediately meaningful: one per voice, pinned
/// to the kit's note map. A melodic child gets the same grid with the pitch
/// controls live instead.
pub const DEFAULT_CHILD: InstrumentType = InstrumentType::DrumRack;

/// The kit voices a drum pattern's eight lanes start pinned to, in the order
/// a drum machine's front panel puts them.
///
/// General MIDI note numbers, which is what [`phosphor_dsp::drum_rack`] reads:
/// bass drum, snare, closed hat, open hat, clap, and the three toms.
pub const DEFAULT_DRUM_LANES: [u8; LANES] = [36, 38, 42, 46, 39, 41, 45, 48];

/// The short names for [`DEFAULT_DRUM_LANES`], for a lane strip.
pub const DEFAULT_DRUM_LABELS: [&str; LANES] = ["BD", "SD", "CH", "OH", "CP", "LT", "MT", "HT"];

/// Whether a child instrument is played as a kit rather than as a keyboard.
///
/// The one thing this decides is where a step's pitch comes from: a drum
/// pattern's lanes are each pinned to a voice and the step only says *when*,
/// while a melodic pattern's steps carry pitch, chord and voicing.
#[must_use]
pub const fn is_drum_child(child: InstrumentType) -> bool {
    matches!(child, InstrumentType::DrumRack)
}

// ── State ──

/// A sequencer track's patterns, and where the player is in them.
#[derive(Debug, Clone, PartialEq)]
pub struct SequencerState {
    /// The eight slots. Everything about a *pattern* lives in one of these.
    patterns: [PatternBlock; SLOTS],
    /// The slot the editor is looking at.
    selected: u8,
    /// The slot the UI believes is sounding. A mirror of the audio thread's,
    /// refreshed by [`SequencerState::sync_from_audio`].
    live: u8,
    /// The queued slot, mirroring what the audio thread was last told.
    pending: Option<u8>,
    switch_quant: SwitchQuant,
    playing: bool,
    chain: [ChainEntry; MAX_CHAIN],
    chain_len: u8,
    /// Row under the editor's cursor.
    lane: u8,
    /// Column under the editor's cursor.
    step: u8,
    /// Whether played notes are written into the pattern.
    step_record: bool,
}

impl SequencerState {
    /// A new sequencer for `child`: eight empty patterns, laid out for a kit
    /// or for a keyboard depending on what it is driving.
    #[must_use]
    pub fn new(child: InstrumentType) -> Self {
        let mut blank = PatternBlock::empty();
        if is_drum_child(child) {
            for (lane, note) in blank.lanes.iter_mut().zip(DEFAULT_DRUM_LANES) {
                *lane = Lane::drum(note);
            }
        }
        Self {
            patterns: [blank; SLOTS],
            selected: 0,
            live: 0,
            pending: None,
            switch_quant: SwitchQuant::PatternEnd,
            playing: false,
            chain: [ChainEntry { slot: 0, repeats: 1 }; MAX_CHAIN],
            chain_len: 0,
            lane: 0,
            step: 0,
            step_record: false,
        }
    }

    // ── Reads ──

    #[must_use]
    pub fn selected_slot(&self) -> u8 {
        self.selected
    }

    #[must_use]
    pub fn live_slot(&self) -> u8 {
        self.live
    }

    #[must_use]
    pub fn queued_slot(&self) -> Option<u8> {
        self.pending
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    #[must_use]
    pub fn switch_quant(&self) -> SwitchQuant {
        self.switch_quant
    }

    #[must_use]
    pub fn is_step_recording(&self) -> bool {
        self.step_record
    }

    #[must_use]
    pub fn lane_cursor(&self) -> usize {
        (self.lane as usize).min(LANES - 1)
    }

    #[must_use]
    pub fn step_cursor(&self) -> usize {
        (self.step as usize).min(MAX_STEPS - 1)
    }

    /// The pattern under the editor.
    #[must_use]
    pub fn pattern(&self) -> &PatternBlock {
        &self.patterns[(self.selected as usize).min(SLOTS - 1)]
    }

    /// One of the eight, as stored — without the track-level settings. Use
    /// [`SequencerState::block`] for what the audio thread should be given.
    #[must_use]
    pub fn pattern_at(&self, slot: usize) -> &PatternBlock {
        &self.patterns[slot.min(SLOTS - 1)]
    }

    /// The lane under the editor.
    #[must_use]
    pub fn lane(&self) -> &Lane {
        &self.pattern().lanes[self.lane_cursor()]
    }

    /// The step under the editor.
    #[must_use]
    pub fn step(&self) -> &Step {
        &self.lane().steps[self.step_cursor()]
    }

    /// The chain, as far as it is filled in.
    #[must_use]
    pub fn chain(&self) -> &[ChainEntry] {
        &self.chain[..(self.chain_len as usize).min(MAX_CHAIN)]
    }

    /// Whether a chain is running. A chain owns the slot outright, so
    /// queueing one by hand does nothing until the chain is cleared.
    #[must_use]
    pub fn is_chained(&self) -> bool {
        self.chain_len > 0
    }

    /// The block for `slot` as the audio thread should see it: the pattern's
    /// own data with the track-level settings stamped on.
    ///
    /// The only way a block leaves this module, which is what keeps the two
    /// sides of the fence agreeing about what "running" and "queued" mean.
    #[must_use]
    pub fn block(&self, slot: usize) -> PatternBlock {
        let mut block = self.patterns[slot.min(SLOTS - 1)];
        block.playing = self.playing;
        block.pending_slot = self.pending;
        block.switch_quant = self.switch_quant;
        block.chain = self.chain;
        block.chain_len = self.chain_len;
        block
    }

    /// Where a queued switch lands, and how many steps away it is.
    ///
    /// The same arithmetic the audio thread does, on the same inputs, so the
    /// countdown on screen is not a message that may not have arrived yet.
    #[must_use]
    pub fn countdown(&self, position: i64) -> Option<(u8, i64)> {
        let slot = self.pending?;
        let live = self.pattern_at(self.live as usize);
        let at = self.switch_quant.boundary(position, live.length_ticks());
        Some((slot, (at - position).div_euclid(live.ticks_per_step())))
    }

    /// Take the live slot, the queued slot and the playhead from the audio
    /// thread's own copy. Called once a frame, from whatever draws.
    ///
    /// Without it the UI's idea of which pattern is playing would be a guess
    /// that has to survive chain advances and quantized switches; with it,
    /// the guess is only ever used before the first callback lands.
    pub fn sync_from_audio(&mut self, status: &phosphor_core::project::PatternStatus) {
        self.live = status.live_slot().min(SLOTS as u8 - 1);
        self.pending = status.queued_slot().filter(|&s| (s as usize) < SLOTS);
    }

    /// The pattern-level settings, as a panel: label, value, and whether it
    /// is meaningful for what this sequencer is driving.
    ///
    /// A placeholder for the sequencer's own controls until the grid view
    /// exists — enough to see what a pattern is set to, and to prove the ops
    /// reach it.
    #[must_use]
    pub fn panel_rows(&self) -> Vec<(&'static str, String)> {
        let p = self.pattern();
        vec![
            ("slot", format!("{}", self.selected + 1)),
            ("steps", format!("{}", p.step_count())),
            ("rate", p.rate.label().to_string()),
            ("swing", format!("{}%", p.swing)),
            ("mode", p.mode.label().to_string()),
            ("key", chords::note_name(p.tonic % 12).to_string()),
            ("gate", format!("{}%", p.default_gate)),
            ("accent", format!("{}", p.accent_vel)),
            ("base", format!("{}", p.base_vel)),
            ("switch", self.switch_quant.label().to_string()),
            ("run", if self.playing { "yes".into() } else { "no".into() }),
        ]
    }
}

// ── The application model's side ──

/// One pattern block on its way to the audio thread.
///
/// The app model produces these rather than sending them, because it has no
/// channel and should not grow one: a frontend holds the sender and turns
/// each of these into a command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternSync {
    pub track_id: usize,
    pub slot: u8,
    pub block: PatternBlock,
}

impl PatternSync {
    /// The command that carries this block.
    #[must_use]
    pub fn command(self) -> phosphor_core::mixer::MixerCommand {
        phosphor_core::mixer::MixerCommand::SetPattern {
            track_id: self.track_id,
            slot: self.slot,
            block: self.block,
        }
    }
}

impl crate::state::NavState {
    /// Apply a sequencer op to the track under the cursor, and say what the
    /// audio thread now needs.
    ///
    /// The two halves come back together on purpose: an edit that changed a
    /// pattern and a caller that forgot to send it is a sequencer that plays
    /// what it used to, which is the hardest kind of bug to see.
    pub fn sequencer_op(&mut self, op: ops::SeqOp) -> (ops::SeqEffect, Vec<PatternSync>) {
        let index = self.track_cursor;
        let Some(track) = self.tracks.get_mut(index) else {
            return (ops::SeqEffect::NOTHING, Vec::new());
        };
        let effect = ops::dispatch(track, op);
        (effect, self.sequencer_syncs(index, effect))
    }

    /// The blocks an effect asks to be sent for one track.
    #[must_use]
    pub fn sequencer_syncs(&self, track_idx: usize, effect: ops::SeqEffect) -> Vec<PatternSync> {
        let Some(track) = self.tracks.get(track_idx) else { return Vec::new() };
        let (Some(state), Some(track_id)) = (track.sequencer.as_ref(), track.mixer_id) else {
            return Vec::new();
        };
        effect
            .slots()
            .map(|slot| PatternSync { track_id, slot, block: state.block(slot as usize) })
            .collect()
    }

    /// Every block a track has, for when the audio thread has none of them:
    /// a track that has just been created, or one that has just been read
    /// out of a session.
    #[must_use]
    pub fn all_sequencer_syncs(&self, track_idx: usize) -> Vec<PatternSync> {
        self.sequencer_syncs(track_idx, ops::SeqEffect::all_slots())
    }

    /// Make the track under the cursor a sequencer track, and hand back the
    /// eight blocks that have to reach the audio thread for it to play.
    pub fn attach_sequencer(&mut self, state: SequencerState) -> Vec<PatternSync> {
        let index = self.track_cursor;
        if let Some(track) = self.tracks.get_mut(index) {
            track.sequencer = Some(Box::new(state));
        }
        self.all_sequencer_syncs(index)
    }

    /// Take every sequencer's live slot, queued slot and playhead from the
    /// audio thread. Called once a frame by whatever draws.
    pub fn sync_sequencers_from_audio(&mut self) {
        for track in &mut self.tracks {
            if let (Some(state), Some(handle)) = (track.sequencer.as_mut(), track.handle.as_ref()) {
                state.sync_from_audio(&handle.pattern);
            }
        }
    }

    /// Where the playhead is inside the pattern on a track, for the marker on
    /// the step grid. `None` when the track has no sequencer running.
    #[must_use]
    pub fn sequencer_playhead(&self, track_idx: usize) -> Option<usize> {
        let track = self.tracks.get(track_idx)?;
        track.sequencer.as_ref()?;
        let handle = track.handle.as_ref()?;
        handle.pattern.is_running().then(|| handle.pattern.step() as usize)
    }
}

// ── Session ──

/// A sequencer as a session stores it.
///
/// Sparse: only the steps that are on are written, so a track with four hits
/// on it is four lines of JSON rather than two thousand. Steps past the
/// pattern's current length are stored too — shortening a pattern masks its
/// tail rather than clearing it, and a session that dropped the masked steps
/// would turn that into a truncation the next time it was opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSequencer {
    /// The child instrument, by the same key a track stores.
    pub child: String,
    /// The child's panel, so a sequencer track restores the sound it had.
    #[serde(default)]
    pub child_params: Vec<f32>,
    /// The child's selectors, by position. Same reasoning as
    /// [`crate::session::SessionTrack::discrete`].
    #[serde(default)]
    pub discrete: Vec<SessionSelector>,
    pub selected: u8,
    pub live: u8,
    /// Where the editor's cursor was. Not needed to play anything, and kept
    /// because reopening a session on the step you were working on is the
    /// difference between resuming and starting again.
    #[serde(default)]
    pub lane: u8,
    #[serde(default)]
    pub step: u8,
    pub playing: bool,
    /// [`SwitchQuant::index`].
    pub switch_quant: u8,
    /// `(slot, repeats)` per chain entry.
    #[serde(default)]
    pub chain: Vec<(u8, u8)>,
    pub patterns: Vec<SessionPattern>,
}

/// One pattern slot on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPattern {
    pub steps: u8,
    /// [`Rate::index`].
    pub rate: u8,
    pub swing: u8,
    pub base_vel: u8,
    pub accent_vel: u8,
    pub default_gate: u8,
    /// [`Mode::index`].
    pub mode: u8,
    pub tonic: u8,
    pub lanes: Vec<SessionLane>,
}

/// One lane on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionLane {
    /// The pinned drum voice, or 255 when the pitch comes from the step.
    pub note: u8,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub soloed: bool,
    /// Only the steps that are on.
    #[serde(default)]
    pub steps: Vec<SessionStep>,
}

/// One step that is switched on, and where.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SessionStep {
    pub index: u8,
    pub octave: u8,
    pub key: u8,
    pub chord: u8,
    pub voicing: u8,
    pub accent: bool,
    pub gate: u8,
}

impl SessionSequencer {
    /// Write a sequencer track out.
    #[must_use]
    pub fn from_state(
        state: &SequencerState,
        child: InstrumentType,
        child_params: &[f32],
    ) -> Self {
        Self {
            child: instrument_key(child).to_string(),
            child_params: child_params.to_vec(),
            discrete: crate::session::selectors_of(child, child_params),
            selected: state.selected,
            live: state.live,
            lane: state.lane,
            step: state.step,
            playing: state.playing,
            switch_quant: state.switch_quant.index(),
            chain: state.chain().iter().map(|e| (e.slot, e.repeats)).collect(),
            patterns: state
                .patterns
                .iter()
                .map(|block| SessionPattern {
                    steps: block.steps,
                    rate: block.rate.index(),
                    swing: block.swing,
                    base_vel: block.base_vel,
                    accent_vel: block.accent_vel,
                    default_gate: block.default_gate,
                    mode: block.mode.index(),
                    tonic: block.tonic,
                    lanes: block
                        .lanes
                        .iter()
                        .map(|lane| SessionLane {
                            note: lane.note,
                            muted: lane.muted,
                            soloed: lane.soloed,
                            steps: lane
                                .steps
                                .iter()
                                .enumerate()
                                .filter(|(_, step)| step.on)
                                .map(|(index, step)| SessionStep {
                                    index: index as u8,
                                    octave: step.octave,
                                    key: step.key,
                                    chord: step.chord,
                                    voicing: step.voicing,
                                    accent: step.accent,
                                    gate: step.gate,
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// The child this session named, or the default when the name is one this
    /// build does not have — a track with no instrument at all is worse than
    /// a track with the wrong one, and the patterns are still right.
    #[must_use]
    pub fn child_instrument(&self) -> InstrumentType {
        parse_instrument_type(&self.child)
            .filter(|i| !i.is_sequencer())
            .unwrap_or(DEFAULT_CHILD)
    }

    /// The child's panel as it should be applied, with the stored selector
    /// positions resolved against today's banks.
    ///
    /// Returns `None` when the saved block is a different length from the
    /// instrument's, which means it is a different panel — the same rule
    /// ordinary tracks follow, and for the same reason: copying it in slot by
    /// slot would load every value into the wrong control.
    #[must_use]
    pub fn child_panel(&self, expected: usize) -> Option<Vec<f32>> {
        if self.child_params.len() != expected {
            return None;
        }
        let mut params = self.child_params.clone();
        apply_selectors(self.child_instrument(), &mut params, &self.discrete);
        Some(params)
    }

    /// Read a sequencer track back.
    #[must_use]
    pub fn to_state(&self) -> SequencerState {
        let mut state = SequencerState::new(self.child_instrument());
        state.selected = self.selected.min(SLOTS as u8 - 1);
        state.live = self.live.min(SLOTS as u8 - 1);
        state.lane = self.lane.min(LANES as u8 - 1);
        state.step = self.step.min(MAX_STEPS as u8 - 1);
        state.playing = self.playing;
        state.switch_quant = SwitchQuant::from_index(self.switch_quant);
        state.chain_len = self.chain.len().min(MAX_CHAIN) as u8;
        for (entry, &(slot, repeats)) in state.chain.iter_mut().zip(&self.chain) {
            *entry = ChainEntry { slot: slot.min(SLOTS as u8 - 1), repeats: repeats.max(1) };
        }

        for (block, stored) in state.patterns.iter_mut().zip(&self.patterns) {
            block.steps = stored.steps;
            block.rate = Rate::from_index(stored.rate);
            block.swing = stored
                .swing
                .clamp(PatternBlock::MIN_SWING, PatternBlock::MAX_SWING);
            block.base_vel = stored.base_vel;
            block.accent_vel = stored.accent_vel;
            block.default_gate = stored.default_gate;
            block.mode = Mode::from_index(stored.mode);
            block.tonic = stored.tonic % 12;
            for (lane, stored_lane) in block.lanes.iter_mut().zip(&stored.lanes) {
                lane.note = stored_lane.note;
                lane.muted = stored_lane.muted;
                lane.soloed = stored_lane.soloed;
                for stored_step in &stored_lane.steps {
                    let Some(step) = lane.steps.get_mut(stored_step.index as usize) else {
                        continue;
                    };
                    step.on = true;
                    step.octave = stored_step.octave;
                    step.key = stored_step.key;
                    step.chord = stored_step.chord;
                    step.voicing = stored_step.voicing;
                    step.accent = stored_step.accent;
                    step.gate = stored_step.gate;
                }
            }
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::ops::{dispatch, SeqOp};
    use super::*;
    use crate::state::TrackState;
    use phosphor_core::project::TrackKind;

    pub(super) fn drum_track() -> TrackState {
        let mut track = TrackState::new("seq", 0, false, TrackKind::Instrument, vec![]);
        track.instrument_type = Some(InstrumentType::DrumRack);
        track.synth_params = phosphor_dsp::drum_rack::PARAM_DEFAULTS.to_vec();
        track.sequencer = Some(Box::new(SequencerState::new(InstrumentType::DrumRack)));
        track
    }

    /// A drum child gets eight lanes pinned to eight voices; a melodic child
    /// gets eight lanes that take their pitch from the steps. That one
    /// difference is the whole of "what kind of pattern is this".
    #[test]
    fn a_drum_child_pins_the_lanes_and_a_melodic_one_does_not() {
        let drums = SequencerState::new(InstrumentType::DrumRack);
        for (lane, note) in drums.pattern().lanes.iter().zip(DEFAULT_DRUM_LANES) {
            assert_eq!(lane.note, note);
            assert!(!lane.is_pitched());
        }

        let keys = SequencerState::new(InstrumentType::Juno60);
        assert!(keys.pattern().lanes.iter().all(Lane::is_pitched));
    }

    /// The track-level settings are held once and stamped on the way out, so
    /// a block from any slot carries the same answer.
    #[test]
    fn every_block_carries_the_track_level_settings() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SetPlaying(true));
        dispatch(&mut track, SeqOp::CycleSwitchQuant(1));
        dispatch(&mut track, SeqOp::QueueSlot(3));

        let state = track.sequencer.as_ref().unwrap();
        for slot in 0..SLOTS {
            let block = state.block(slot);
            assert!(block.playing);
            assert_eq!(block.switch_quant, SwitchQuant::Bar);
            assert_eq!(block.pending_slot, Some(3));
        }
    }

    /// Everything a player can set has to survive a save and a load. The
    /// masked tail included: a pattern shortened to four steps still has the
    /// other twenty-eight, and reopening the session must not be what erases
    /// them.
    #[test]
    fn a_sequencer_round_trips_through_a_session() {
        let mut track = drum_track();
        for op in [
            SeqOp::SelectSlot(2),
            SeqOp::SelectLane(3),
            SeqOp::SelectStep(20),
            SeqOp::ToggleStep,
            SeqOp::ToggleAccent,
            SeqOp::NudgeGate(3),
            SeqOp::CycleLength(-2),
            SeqOp::CycleRate(1),
            SeqOp::NudgeSwing(8),
            SeqOp::CycleMode(2),
            SeqOp::SetTonic(7),
            SeqOp::ToggleLaneSolo,
            SeqOp::PushChainEntry { slot: 2, repeats: 4 },
            SeqOp::PushChainEntry { slot: 0, repeats: 1 },
            SeqOp::SetPlaying(true),
        ] {
            dispatch(&mut track, op);
        }
        let before = *track.sequencer.clone().unwrap();

        let stored = SessionSequencer::from_state(
            &before,
            InstrumentType::DrumRack,
            &track.synth_params,
        );
        let json = serde_json::to_string(&stored).unwrap();
        let read: SessionSequencer = serde_json::from_str(&json).unwrap();
        let after = read.to_state();

        assert_eq!(before, after, "a sequencer changed shape across a session");
        assert_eq!(read.child_instrument(), InstrumentType::DrumRack);

        // The child's panel comes back with its selectors resolved by
        // position, which is what an ordinary track load does to them too: a
        // stored fraction is only the patch it named while the bank is the
        // size it was.
        let mut expected = track.synth_params.clone();
        crate::session::apply_selectors(InstrumentType::DrumRack, &mut expected, &stored.discrete);
        assert_eq!(read.child_panel(expected.len()).unwrap(), expected);
    }

    /// The masked tail, specifically: a step past the end of a shortened
    /// pattern is on disk and comes back on.
    #[test]
    fn a_masked_step_survives_a_session() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SelectStep(30));
        dispatch(&mut track, SeqOp::ToggleStep);
        dispatch(&mut track, SeqOp::CycleLength(-2)); // 16 -> 8 steps

        let state = *track.sequencer.clone().unwrap();
        assert_eq!(state.pattern().step_count(), 8);
        let stored =
            SessionSequencer::from_state(&state, InstrumentType::DrumRack, &track.synth_params);
        let back = stored.to_state();
        assert!(back.pattern().lanes[0].steps[30].on, "the masked step was lost");
    }

    /// A saved child this build does not have leaves the patterns intact and
    /// falls back to a real instrument, rather than producing a track with
    /// nothing in its plugin slot.
    #[test]
    fn an_unknown_child_falls_back_rather_than_failing() {
        let state = SequencerState::new(InstrumentType::DrumRack);
        let mut stored = SessionSequencer::from_state(&state, InstrumentType::DrumRack, &[]);
        stored.child = "moog-model-d".into();
        assert_eq!(stored.child_instrument(), DEFAULT_CHILD);
        assert_eq!(stored.to_state().patterns.len(), SLOTS);
    }

    /// A panel of the wrong length is a different panel, not a panel with
    /// missing values — the same rule ordinary tracks follow.
    #[test]
    fn a_child_panel_of_the_wrong_length_is_refused() {
        let state = SequencerState::new(InstrumentType::DrumRack);
        let stored =
            SessionSequencer::from_state(&state, InstrumentType::DrumRack, &[0.1, 0.2, 0.3]);
        assert!(stored.child_panel(3).is_some());
        assert!(stored.child_panel(4).is_none());
    }

    // ── The application model's glue ──

    use crate::state::{initial_tracks, NavState};

    fn nav_with_sequencer() -> NavState {
        let mut nav = NavState::new(initial_tracks());
        let mut track = drum_track();
        track.mixer_id = Some(7);
        nav.tracks.insert(0, track);
        nav.track_cursor = 0;
        nav
    }

    /// An edit produces exactly the commands the audio thread is now missing,
    /// addressed to the right track.
    #[test]
    fn an_edit_produces_the_command_that_carries_it() {
        let mut nav = nav_with_sequencer();
        let (effect, syncs) = nav.sequencer_op(SeqOp::SelectSlot(2));
        assert!(effect.is_nothing());
        assert!(syncs.is_empty(), "moving the cursor sent something");

        let (_, syncs) = nav.sequencer_op(SeqOp::ToggleStep);
        assert_eq!(syncs.len(), 1);
        assert_eq!(syncs[0].track_id, 7);
        assert_eq!(syncs[0].slot, 2);
        assert!(syncs[0].block.lanes[0].steps[0].on);

        // ...and the command it turns into names the same track and slot.
        match syncs[0].command() {
            phosphor_core::mixer::MixerCommand::SetPattern { track_id, slot, .. } => {
                assert_eq!((track_id, slot), (7, 2));
            }
            _ => panic!("a pattern sync produced something else"),
        }
    }

    /// A track that is not wired to the audio engine has nothing to send, and
    /// a track with no sequencer has nothing to change.
    #[test]
    fn a_track_with_no_engine_or_no_sequencer_sends_nothing() {
        let mut nav = nav_with_sequencer();
        nav.tracks[0].mixer_id = None;
        let (effect, syncs) = nav.sequencer_op(SeqOp::ToggleStep);
        assert!(!effect.is_nothing(), "the edit still happened");
        assert!(syncs.is_empty(), "a track with no mixer id sent a command");

        nav.track_cursor = 1; // a bus track
        let (effect, syncs) = nav.sequencer_op(SeqOp::ToggleStep);
        assert!(effect.is_nothing());
        assert!(syncs.is_empty());
    }

    /// A track that has just been created, or just been read out of a
    /// session, needs all eight of its patterns sent.
    #[test]
    fn attaching_a_sequencer_sends_every_slot() {
        let mut nav = NavState::new(initial_tracks());
        let mut track = crate::state::TrackState::new(
            "seq",
            0,
            false,
            phosphor_core::project::TrackKind::Instrument,
            vec![],
        );
        track.instrument_type = Some(InstrumentType::DrumRack);
        track.mixer_id = Some(3);
        nav.tracks.insert(0, track);
        nav.track_cursor = 0;

        let syncs = nav.attach_sequencer(SequencerState::new(InstrumentType::DrumRack));
        assert_eq!(syncs.len(), SLOTS);
        assert_eq!(
            syncs.iter().map(|s| s.slot).collect::<Vec<_>>(),
            (0..SLOTS as u8).collect::<Vec<_>>()
        );
        assert!(nav.tracks[0].sequencer.is_some());
    }

    /// The playhead the grid draws comes from the audio thread, and is
    /// nothing at all when the pattern is not running.
    #[test]
    fn the_playhead_comes_from_the_audio_thread() {
        let mut nav = nav_with_sequencer();
        let handle = std::sync::Arc::new(phosphor_core::project::TrackHandle::new(
            7,
            phosphor_core::project::TrackKind::Instrument,
        ));
        nav.tracks[0].handle = Some(handle.clone());

        assert_eq!(nav.sequencer_playhead(0), None);
        handle.pattern.publish(1, Some(4), 11, true);
        assert_eq!(nav.sequencer_playhead(0), Some(11));

        nav.sync_sequencers_from_audio();
        let state = nav.tracks[0].sequencer.as_ref().unwrap();
        assert_eq!(state.live_slot(), 1);
        assert_eq!(state.queued_slot(), Some(4));
    }

    /// The UI's mirror of which slot is playing comes from the audio thread,
    /// because a chain advance and a quantized switch both happen there.
    #[test]
    fn the_live_slot_is_read_back_from_the_audio_thread() {
        let status = phosphor_core::project::PatternStatus::new();
        status.publish(5, Some(2), 9, true);

        let mut state = SequencerState::new(InstrumentType::DrumRack);
        state.sync_from_audio(&status);
        assert_eq!(state.live_slot(), 5);
        assert_eq!(state.queued_slot(), Some(2));

        status.publish(2, None, 0, true);
        state.sync_from_audio(&status);
        assert_eq!(state.live_slot(), 2);
        assert_eq!(state.queued_slot(), None, "a switch that happened was not noticed");
    }

    /// The countdown is arithmetic on the transport position, so it is the
    /// same number the audio thread will act on rather than a guess about it.
    #[test]
    fn the_countdown_is_in_steps_to_the_switch() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::QueueSlot(1));
        let state = track.sequencer.as_ref().unwrap();
        // A 16-step sixteenth pattern is 3840 ticks; from step 12 that is
        // four steps to the end of it.
        assert_eq!(state.countdown(2880), Some((1, 4)));
        assert_eq!(state.countdown(3600), Some((1, 1)));
    }
}
