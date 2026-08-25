//! Every edit a sequencer can be given, as one enum and one function.
//!
//! # Why this exists
//!
//! Because the keyboard is not going to be the only thing driving it. A step
//! grid wants a box with sixteen buttons on it, and the day that box is
//! plugged in, "toggle the step under the cursor" must not have to be written
//! a second time against a MIDI note number. So the keys do not edit
//! anything: they name a [`SeqOp`], and [`dispatch`] is the only code in the
//! project that changes a pattern. A controller mapping is then a table from
//! CC and note numbers to the same ops, and there is nothing for the two
//! paths to disagree about.
//!
//! The same shape pays off twice over before any hardware arrives:
//!
//! * every edit is testable without a terminal, a mixer or an audio device —
//!   the tests at the bottom of this file are the whole editor;
//! * every edit reports what the audio thread now needs to be told, as a
//!   [`SeqEffect`], so nothing can quietly change a pattern and forget to
//!   send it.
//!
//! # The signature
//!
//! [`dispatch`] takes the whole [`TrackState`] rather than the
//! [`SequencerState`] inside it, because two of the operations are about the
//! track: changing the child instrument replaces what is in the plugin slot
//! and reloads its panel. Passing the sequencer alone would mean those two
//! had to live somewhere else, and then "every mutation goes through one
//! function" would already not be true.

use phosphor_core::pattern::{
    ChainEntry, Chord, PatternBlock, Step, Voicing, LANES, MAX_CHAIN, MAX_CHORD_NOTES, MAX_STEPS,
    SLOTS, STEP_COUNTS,
};

use super::{chords, is_drum_child, SequencerState, DEFAULT_DRUM_LANES};
use crate::state::{InstrumentType, TrackState};

/// Notes held at the moment a step was recorded.
///
/// Fixed size and `Copy` so that an op stays a plain value: five is what the
/// widest chord in the table produces, and a sixth finger is not something
/// the chord identifier could name anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeldNotes {
    notes: [u8; MAX_CHORD_NOTES],
    len: u8,
}

impl HeldNotes {
    /// The notes currently down, in any order. Anything past the fifth is
    /// ignored.
    #[must_use]
    pub fn new(held: &[u8]) -> Self {
        let mut notes = [0u8; MAX_CHORD_NOTES];
        let len = held.len().min(MAX_CHORD_NOTES);
        notes[..len].copy_from_slice(&held[..len]);
        notes[..len].sort_unstable();
        Self { notes, len: len as u8 }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.notes[..self.len as usize]
    }
}

/// One edit.
///
/// Deliberately small and orthogonal: every entry is something a player does
/// in one press, so that a key map and a controller map are both just tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqOp {
    // ── Cursor ──
    /// Look at a different pattern slot. Does not change what is playing.
    SelectSlot(u8),
    SelectLane(u8),
    MoveLane(i8),
    SelectStep(u8),
    MoveStep(i8),

    // ── The step under the cursor ──
    ToggleStep,
    SetStep(bool),
    ClearStep,
    ToggleAccent,
    /// Move the pitch by semitones, or by scale degrees when the pattern is
    /// in a mode.
    NudgePitch(i8),
    NudgeOctave(i8),
    CycleChord(i8),
    CycleVoicing(i8),
    ToggleRootBelow,
    /// Step the gate through the percentages, and off the end into the tie.
    NudgeGate(i8),
    ToggleTie,

    // ── The lane under the cursor ──
    ToggleLaneMute,
    ToggleLaneSolo,
    /// Pin this lane to a drum voice.
    SetLaneNote(u8),

    // ── The pattern under the editor ──
    /// Step the length through [`STEP_COUNTS`]. Shortening masks.
    CycleLength(i8),
    CycleRate(i8),
    NudgeSwing(i8),
    NudgeBaseVelocity(i8),
    NudgeAccentVelocity(i8),
    /// Move the gate newly enabled steps inherit.
    NudgeDefaultGate(i8),
    CycleMode(i8),
    SetTonic(u8),
    ClearPattern,
    CopyPattern { from: u8, to: u8 },

    // ── The track ──
    SetPlaying(bool),
    TogglePlaying,
    /// Queue a slot to take over at the next quantization point.
    QueueSlot(u8),
    ClearQueue,
    CycleSwitchQuant(i8),

    // ── The chain ──
    PushChainEntry { slot: u8, repeats: u8 },
    SetChainRepeats { index: u8, repeats: u8 },
    RemoveChainEntry(u8),
    ClearChain,

    // ── The child instrument ──
    SetChild(InstrumentType),

    // ── Step record ──
    ArmStepRecord(bool),
    /// Write what is being held to the step under the cursor and move on.
    RecordNotes(HeldNotes),
    /// Leave the step as it is and move on.
    RecordRest,
    /// Tie the step before the cursor to whatever follows it.
    RecordTie,
}

/// What the audio thread now has to be told.
///
/// Returned by every dispatch, so that a change to a pattern and the command
/// that carries it cannot come apart: there is no path that edits without
/// saying what it edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeqEffect {
    /// A bit per slot whose block has to be sent.
    ///
    /// The track-level settings ride on any block, so a change to one of them
    /// marks a single slot rather than all eight.
    pub patterns: u8,
    /// The child instrument changed: the plugin slot has to be replaced and
    /// its whole panel resent.
    pub child: bool,
}

impl SeqEffect {
    /// Nothing to do.
    pub const NOTHING: Self = Self { patterns: 0, child: false };

    /// One slot has to be sent.
    #[must_use]
    pub const fn slot(slot: u8) -> Self {
        Self { patterns: 1 << (slot & 0b111), child: false }
    }

    /// Every slot has to be sent.
    #[must_use]
    pub const fn all_slots() -> Self {
        Self { patterns: 0xFF, child: false }
    }

    #[must_use]
    pub const fn is_nothing(self) -> bool {
        self.patterns == 0 && !self.child
    }

    /// Whether `slot` is one of the ones that has to be sent.
    #[must_use]
    pub const fn wants(self, slot: u8) -> bool {
        self.patterns & (1 << (slot & 0b111)) != 0
    }

    /// The slots to send, in order.
    pub fn slots(self) -> impl Iterator<Item = u8> {
        (0..SLOTS as u8).filter(move |&slot| self.wants(slot))
    }

    /// Both effects, for a key that dispatches more than one op.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self { patterns: self.patterns | other.patterns, child: self.child || other.child }
    }
}

/// Apply one edit.
///
/// The single mutation surface. Everything a key, a menu or a controller can
/// do to a sequencer arrives here, and nothing else in the project writes to
/// a [`SequencerState`].
///
/// A track with no sequencer on it is not an error — the same key may be live
/// on an ordinary track — so it returns [`SeqEffect::NOTHING`] and changes
/// nothing.
pub fn dispatch(track: &mut TrackState, op: SeqOp) -> SeqEffect {
    // The child swap is the one operation that reaches outside the sequencer,
    // so it is handled before the borrow that the rest of them share.
    if let SeqOp::SetChild(child) = op {
        return set_child(track, child);
    }

    let Some(state) = track.sequencer.as_mut() else {
        return SeqEffect::NOTHING;
    };
    apply(state, op)
}

/// Replace what a sequencer track is driving.
///
/// The child is the track's own `instrument_type`, so this is a track edit as
/// much as a sequencer one: the panel is reloaded from the new instrument's
/// defaults, and the lanes are re-laid-out only when the *kind* of child
/// changes. Swapping one drum machine for another leaves a kit pattern's
/// lanes where the player put them.
fn set_child(track: &mut TrackState, child: InstrumentType) -> SeqEffect {
    if track.sequencer.is_none() || child.is_sequencer() {
        return SeqEffect::NOTHING;
    }
    let previous = track.instrument_type;
    if previous == Some(child) {
        return SeqEffect::NOTHING;
    }

    track.instrument_type = Some(child);
    track.synth_params = crate::preset::defaults(child);

    let was_drums = previous.is_some_and(is_drum_child);
    let effect = if was_drums == is_drum_child(child) {
        SeqEffect::NOTHING
    } else {
        let state = track.sequencer.as_mut().expect("checked above");
        relay_lanes(state, child);
        SeqEffect::all_slots()
    };
    SeqEffect { child: true, ..effect }
}

/// Point every lane at a drum voice, or at its own steps, to match the child.
fn relay_lanes(state: &mut SequencerState, child: InstrumentType) {
    let drums = is_drum_child(child);
    for slot in 0..SLOTS {
        for (lane_index, lane) in state.patterns[slot].lanes.iter_mut().enumerate() {
            lane.note = if drums {
                DEFAULT_DRUM_LANES[lane_index]
            } else {
                phosphor_core::pattern::Lane::FROM_STEP
            };
        }
    }
}

#[allow(clippy::too_many_lines)] // One arm per operation; splitting it would
                                 // only move the list somewhere else.
fn apply(state: &mut SequencerState, op: SeqOp) -> SeqEffect {
    let selected = state.selected_slot();
    let lane_index = state.lane_cursor();
    let step_index = state.step_cursor();
    let here = SeqEffect::slot(selected);

    match op {
        SeqOp::SetChild(_) => SeqEffect::NOTHING, // handled by `dispatch`

        // ── Cursor ──
        SeqOp::SelectSlot(slot) => {
            state.selected = slot.min(SLOTS as u8 - 1);
            SeqEffect::NOTHING
        }
        SeqOp::SelectLane(lane) => {
            state.lane = lane.min(LANES as u8 - 1);
            SeqEffect::NOTHING
        }
        SeqOp::MoveLane(delta) => {
            state.lane = step_within(state.lane, delta, LANES as u8);
            SeqEffect::NOTHING
        }
        SeqOp::SelectStep(step) => {
            state.step = step.min(MAX_STEPS as u8 - 1);
            SeqEffect::NOTHING
        }
        SeqOp::MoveStep(delta) => {
            // Within the pattern's *current* length, so the cursor cannot
            // walk off into the masked tail by accident. Reaching a masked
            // step is what `SelectStep` is for.
            let len = state.pattern().step_count() as u8;
            state.step = wrap_within(state.step.min(len - 1), delta, len);
            SeqEffect::NOTHING
        }

        // ── Step ──
        SeqOp::ToggleStep => {
            let on = !state.step().on;
            set_step(state, lane_index, step_index, on);
            here
        }
        SeqOp::SetStep(on) => {
            set_step(state, lane_index, step_index, on);
            here
        }
        SeqOp::ClearStep => {
            let default_gate = state.pattern().default_gate;
            let step = step_mut(state, lane_index, step_index);
            *step = Step { gate: default_gate, ..Step::silent() };
            here
        }
        SeqOp::ToggleAccent => {
            let step = step_mut(state, lane_index, step_index);
            step.accent = !step.accent;
            here
        }
        SeqOp::NudgePitch(delta) => {
            if state.lane().is_pitched() {
                let (mode, tonic) = {
                    let p = state.pattern();
                    (p.mode, p.tonic)
                };
                let step = step_mut(state, lane_index, step_index);
                let note = mode.walk(step.root(), tonic, i32::from(delta));
                step.octave = note / 12;
                step.key = note % 12;
                here
            } else {
                SeqEffect::NOTHING
            }
        }
        SeqOp::NudgeOctave(delta) => {
            if state.lane().is_pitched() {
                let step = step_mut(state, lane_index, step_index);
                let note = (i32::from(step.root()) + i32::from(delta) * 12).clamp(0, 127) as u8;
                step.octave = note / 12;
                step.key = note % 12;
                here
            } else {
                SeqEffect::NOTHING
            }
        }
        SeqOp::CycleChord(delta) => {
            if state.lane().is_pitched() {
                let step = step_mut(state, lane_index, step_index);
                step.chord = step.chord_kind().stepped(i32::from(delta)).index();
                here
            } else {
                SeqEffect::NOTHING
            }
        }
        SeqOp::CycleVoicing(delta) => {
            if state.lane().is_pitched() {
                let step = step_mut(state, lane_index, step_index);
                let voicing = step.voicing_kind().stepped(i32::from(delta)).index();
                step.voicing = voicing | (step.voicing & Step::ROOT_BELOW);
                here
            } else {
                SeqEffect::NOTHING
            }
        }
        SeqOp::ToggleRootBelow => {
            if state.lane().is_pitched() {
                let step = step_mut(state, lane_index, step_index);
                step.voicing ^= Step::ROOT_BELOW;
                here
            } else {
                SeqEffect::NOTHING
            }
        }
        SeqOp::NudgeGate(delta) => {
            let step = step_mut(state, lane_index, step_index);
            step.gate = nudge_gate(step.gate, delta);
            here
        }
        SeqOp::ToggleTie => {
            let default_gate = state.pattern().default_gate;
            let step = step_mut(state, lane_index, step_index);
            step.gate = if step.gate == Step::TIE { default_gate } else { Step::TIE };
            here
        }

        // ── Lane ──
        SeqOp::ToggleLaneMute => {
            let lane = &mut state.patterns[selected as usize].lanes[lane_index];
            lane.muted = !lane.muted;
            here
        }
        SeqOp::ToggleLaneSolo => {
            let lane = &mut state.patterns[selected as usize].lanes[lane_index];
            lane.soloed = !lane.soloed;
            here
        }
        SeqOp::SetLaneNote(note) => {
            let lane = &mut state.patterns[selected as usize].lanes[lane_index];
            lane.note = note;
            here
        }

        // ── Pattern ──
        SeqOp::CycleLength(delta) => {
            let block = &mut state.patterns[selected as usize];
            let current = STEP_COUNTS
                .iter()
                .position(|&c| c == block.steps)
                .unwrap_or(3) as i32;
            let index = (current + i32::from(delta)).clamp(0, STEP_COUNTS.len() as i32 - 1);
            block.steps = STEP_COUNTS[index as usize];
            here
        }
        SeqOp::CycleRate(delta) => {
            let block = &mut state.patterns[selected as usize];
            block.rate = block.rate.stepped(i32::from(delta));
            here
        }
        SeqOp::NudgeSwing(delta) => {
            let block = &mut state.patterns[selected as usize];
            block.swing = clamp_u8(
                block.swing,
                delta,
                PatternBlock::MIN_SWING,
                PatternBlock::MAX_SWING,
            );
            here
        }
        SeqOp::NudgeBaseVelocity(delta) => {
            let block = &mut state.patterns[selected as usize];
            block.base_vel = clamp_u8(block.base_vel, delta, 1, 127);
            here
        }
        SeqOp::NudgeAccentVelocity(delta) => {
            let block = &mut state.patterns[selected as usize];
            block.accent_vel = clamp_u8(block.accent_vel, delta, 1, 127);
            here
        }
        SeqOp::NudgeDefaultGate(delta) => {
            // In fives, like the per-step gate: two controls that look the
            // same and move at different rates is a control that feels broken.
            let block = &mut state.patterns[selected as usize];
            block.default_gate = clamp_u8(
                block.default_gate,
                delta.saturating_mul(GATE_STEP),
                Step::MIN_GATE,
                Step::MAX_GATE,
            );
            here
        }
        SeqOp::CycleMode(delta) => {
            let block = &mut state.patterns[selected as usize];
            block.mode = block.mode.stepped(i32::from(delta));
            here
        }
        SeqOp::SetTonic(tonic) => {
            let block = &mut state.patterns[selected as usize];
            block.tonic = tonic % 12;
            here
        }
        SeqOp::ClearPattern => {
            // The lanes stay pointed where they were: clearing a kit pattern
            // should leave the kit, not turn it into a keyboard.
            let block = &mut state.patterns[selected as usize];
            for lane in &mut block.lanes {
                for step in &mut lane.steps {
                    *step = Step { gate: block.default_gate, ..Step::silent() };
                }
            }
            here
        }
        SeqOp::CopyPattern { from, to } => {
            let from = (from as usize).min(SLOTS - 1);
            let to = (to as usize).min(SLOTS - 1);
            if from == to {
                return SeqEffect::NOTHING;
            }
            state.patterns[to] = state.patterns[from];
            SeqEffect::slot(to as u8)
        }

        // ── Track ──
        SeqOp::SetPlaying(playing) => {
            if state.playing == playing {
                return SeqEffect::NOTHING;
            }
            state.playing = playing;
            here
        }
        SeqOp::TogglePlaying => {
            state.playing = !state.playing;
            here
        }
        SeqOp::QueueSlot(slot) => {
            let slot = slot.min(SLOTS as u8 - 1);
            // A running chain owns the slot, and queueing against one would
            // put a number on screen that nothing is ever going to act on.
            if state.is_chained() || slot == state.live {
                return SeqEffect::NOTHING;
            }
            state.pending = Some(slot);
            here
        }
        SeqOp::ClearQueue => {
            if state.pending.take().is_none() {
                return SeqEffect::NOTHING;
            }
            here
        }
        SeqOp::CycleSwitchQuant(delta) => {
            state.switch_quant = state.switch_quant.stepped(i32::from(delta));
            here
        }

        // ── Chain ──
        SeqOp::PushChainEntry { slot, repeats } => {
            let len = state.chain_len as usize;
            if len >= MAX_CHAIN {
                return SeqEffect::NOTHING;
            }
            state.chain[len] =
                ChainEntry { slot: slot.min(SLOTS as u8 - 1), repeats: repeats.max(1) };
            state.chain_len += 1;
            // A chain takes over from the queue outright.
            state.pending = None;
            here
        }
        SeqOp::SetChainRepeats { index, repeats } => {
            let Some(entry) = state.chain.get_mut(index as usize) else {
                return SeqEffect::NOTHING;
            };
            if index >= state.chain_len {
                return SeqEffect::NOTHING;
            }
            entry.repeats = repeats.max(1);
            here
        }
        SeqOp::RemoveChainEntry(index) => {
            let len = state.chain_len as usize;
            if index as usize >= len {
                return SeqEffect::NOTHING;
            }
            for i in index as usize..len - 1 {
                state.chain[i] = state.chain[i + 1];
            }
            state.chain_len -= 1;
            here
        }
        SeqOp::ClearChain => {
            if state.chain_len == 0 {
                return SeqEffect::NOTHING;
            }
            state.chain_len = 0;
            here
        }

        // ── Step record ──
        SeqOp::ArmStepRecord(armed) => {
            state.step_record = armed;
            SeqEffect::NOTHING
        }
        SeqOp::RecordNotes(held) => {
            if !state.step_record || held.as_slice().is_empty() {
                return SeqEffect::NOTHING;
            }
            let pitched = state.lane().is_pitched();
            let default_gate = state.pattern().default_gate;
            let (mode, tonic) = {
                let p = state.pattern();
                (p.mode, p.tonic)
            };

            if pitched {
                // What was played, named: the identifier walks the chord
                // table looking for the set of notes that came in, so playing
                // a first-inversion minor seventh stores exactly that rather
                // than its lowest note.
                let found = chords::identify(held.as_slice(), mode, tonic);
                let step = step_mut(state, lane_index, step_index);
                step.on = true;
                step.gate = default_gate;
                match found {
                    Some(named) => {
                        step.octave = named.root / 12;
                        step.key = named.root % 12;
                        step.chord = named.chord.index();
                        step.voicing = named.voicing.index()
                            | if named.root_below { Step::ROOT_BELOW } else { 0 };
                    }
                    None => {
                        let root = held.as_slice()[0];
                        step.octave = root / 12;
                        step.key = root % 12;
                        step.chord = Chord::None.index();
                        step.voicing = Voicing::Close.index();
                    }
                }
            } else {
                // A drum lane's pitch is the lane's, so a played note picks
                // the lane rather than setting a pitch: hitting the pad for
                // the voice this lane is pinned to writes a hit.
                let step = step_mut(state, lane_index, step_index);
                step.on = true;
                step.gate = default_gate;
            }

            advance_record_cursor(state);
            here
        }
        SeqOp::RecordRest => {
            if !state.step_record {
                return SeqEffect::NOTHING;
            }
            advance_record_cursor(state);
            SeqEffect::NOTHING
        }
        SeqOp::RecordTie => {
            if !state.step_record {
                return SeqEffect::NOTHING;
            }
            // The tie belongs to the step just written, which is the one
            // behind the cursor.
            let len = state.pattern().step_count() as u8;
            let previous = wrap_within(state.step.min(len - 1), -1, len) as usize;
            let step = step_mut(state, lane_index, previous);
            step.gate = Step::TIE;
            advance_record_cursor(state);
            here
        }
    }
}

// ── Helpers ──

fn step_mut(state: &mut SequencerState, lane: usize, step: usize) -> &mut Step {
    let slot = state.selected as usize;
    &mut state.patterns[slot].lanes[lane].steps[step]
}

/// Turn a step on or off. Enabling inherits the pattern's default gate, which
/// is the one thing a newly written step needs and the one thing a player
/// would otherwise have to set on every hit.
fn set_step(state: &mut SequencerState, lane: usize, step_index: usize, on: bool) {
    let default_gate = state.pattern().default_gate;
    let step = step_mut(state, lane, step_index);
    step.on = on;
    if on {
        step.gate = default_gate;
    }
}

/// Move the record cursor one step on, wrapping at the pattern's length.
fn advance_record_cursor(state: &mut SequencerState) {
    let len = state.pattern().step_count() as u8;
    state.step = wrap_within(state.step.min(len - 1), 1, len);
}

/// Move a cursor by `delta`, stopping at both ends.
fn step_within(current: u8, delta: i8, count: u8) -> u8 {
    (i32::from(current) + i32::from(delta)).clamp(0, i32::from(count) - 1) as u8
}

/// Move a cursor by `delta`, wrapping round.
fn wrap_within(current: u8, delta: i8, count: u8) -> u8 {
    let count = i32::from(count.max(1));
    ((i32::from(current) + i32::from(delta)).rem_euclid(count)) as u8
}

/// How much one press moves a gate, in percent.
const GATE_STEP: i8 = 5;

fn clamp_u8(current: u8, delta: i8, low: u8, high: u8) -> u8 {
    (i32::from(current) + i32::from(delta)).clamp(i32::from(low), i32::from(high)) as u8
}

/// The gate control, walked.
///
/// Percentages in fives, and off the top of the range into the tie — which is
/// where a player looks for it, because "hold this note until the next one"
/// is the longest gate there is rather than a mode somewhere else.
fn nudge_gate(gate: u8, delta: i8) -> u8 {
    if gate == Step::TIE {
        return if delta < 0 { Step::MAX_GATE } else { Step::TIE };
    }
    let next = i32::from(gate) + i32::from(delta) * i32::from(GATE_STEP);
    if next > i32::from(Step::MAX_GATE) {
        return Step::TIE;
    }
    next.clamp(i32::from(Step::MIN_GATE), i32::from(Step::MAX_GATE)) as u8
}

#[cfg(test)]
mod tests {
    use super::super::tests::drum_track;
    use super::*;
    use crate::state::TrackState;
    use phosphor_core::pattern::{Lane, Mode, Rate, SwitchQuant};
    use phosphor_core::project::TrackKind;

    fn melodic_track() -> TrackState {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SetChild(InstrumentType::Juno60));
        track
    }

    fn seq(track: &TrackState) -> &SequencerState {
        track.sequencer.as_ref().unwrap()
    }

    /// An op on a track with no sequencer is not an error: the same key can
    /// be live on an ordinary track, and a dispatch that panicked there would
    /// make the key map a minefield.
    #[test]
    fn a_track_without_a_sequencer_ignores_the_ops() {
        let mut track = TrackState::new("synth", 0, false, TrackKind::Instrument, vec![]);
        assert_eq!(dispatch(&mut track, SeqOp::ToggleStep), SeqEffect::NOTHING);
        assert!(track.sequencer.is_none());
    }

    /// Every edit says which slots the audio thread now needs, so nothing can
    /// change a pattern and forget to send it.
    #[test]
    fn an_edit_reports_the_slot_it_changed() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SelectSlot(3));
        let effect = dispatch(&mut track, SeqOp::ToggleStep);
        assert!(effect.wants(3));
        assert_eq!(effect.slots().collect::<Vec<_>>(), vec![3]);
        assert!(!effect.wants(0));

        // Moving the cursor changes nothing the audio thread can see.
        assert!(dispatch(&mut track, SeqOp::MoveStep(1)).is_nothing());
        assert!(dispatch(&mut track, SeqOp::SelectLane(2)).is_nothing());
    }

    #[test]
    fn toggling_a_step_writes_the_patterns_default_gate() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::NudgeDefaultGate(4)); // four presses of five
        dispatch(&mut track, SeqOp::ToggleStep);
        assert!(seq(&track).step().on);
        assert_eq!(seq(&track).step().gate, 70);

        dispatch(&mut track, SeqOp::ToggleStep);
        assert!(!seq(&track).step().on);
    }

    /// The cursor walks the pattern that is there, and wraps at its end
    /// rather than at thirty-two.
    #[test]
    fn the_step_cursor_wraps_at_the_patterns_length() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::CycleLength(-2)); // 16 -> 8
        dispatch(&mut track, SeqOp::SelectStep(7));
        dispatch(&mut track, SeqOp::MoveStep(1));
        assert_eq!(seq(&track).step_cursor(), 0);
        dispatch(&mut track, SeqOp::MoveStep(-1));
        assert_eq!(seq(&track).step_cursor(), 7);
    }

    /// Shortening a pattern hides its tail. The steps are still there, and
    /// lengthening it brings them back — which is why a length change is a
    /// mask and not an edit.
    #[test]
    fn shortening_a_pattern_masks_rather_than_clears() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SelectStep(30));
        dispatch(&mut track, SeqOp::ToggleStep);
        dispatch(&mut track, SeqOp::CycleLength(-3));
        assert_eq!(seq(&track).pattern().step_count(), 4);
        assert!(seq(&track).pattern().lanes[0].steps[30].on);
        dispatch(&mut track, SeqOp::CycleLength(5));
        assert_eq!(seq(&track).pattern().step_count(), 32);
        assert!(seq(&track).pattern().lanes[0].steps[30].on);
    }

    /// Pitch is a lane property before it is a step property: a lane pinned
    /// to the snare has no pitch to walk, and the key that would walk one
    /// does nothing rather than something surprising.
    #[test]
    fn a_pinned_drum_lane_has_no_pitch_to_walk() {
        let mut track = drum_track();
        assert!(dispatch(&mut track, SeqOp::NudgePitch(1)).is_nothing());
        assert!(dispatch(&mut track, SeqOp::CycleChord(1)).is_nothing());
        assert_eq!(seq(&track).lane().note, DEFAULT_DRUM_LANES[0]);
    }

    /// In a mode the pitch control walks the scale, which is the whole point
    /// of having one.
    #[test]
    fn pitch_walks_semitones_off_a_mode_and_degrees_on_one() {
        let mut track = melodic_track();
        let start = seq(&track).step().root();
        dispatch(&mut track, SeqOp::NudgePitch(1));
        assert_eq!(seq(&track).step().root(), start + 1, "chromatic is semitones");

        dispatch(&mut track, SeqOp::CycleMode(1)); // Ionian
        dispatch(&mut track, SeqOp::SetTonic(0));
        dispatch(&mut track, SeqOp::NudgePitch(1));
        assert_eq!(seq(&track).pattern().mode, Mode::Ionian);
        assert_eq!(seq(&track).step().root(), 62, "C# up a degree in C major is D");
    }

    #[test]
    fn octaves_move_by_twelve_and_stop_at_the_ends() {
        let mut track = melodic_track();
        dispatch(&mut track, SeqOp::NudgeOctave(1));
        assert_eq!(seq(&track).step().root(), 72);
        for _ in 0..20 {
            dispatch(&mut track, SeqOp::NudgeOctave(1));
        }
        assert!(seq(&track).step().root() <= 127);
        for _ in 0..40 {
            dispatch(&mut track, SeqOp::NudgeOctave(-1));
        }
        assert!(seq(&track).step().root() < 12);
    }

    /// The gate walks up through the percentages and off the end into the
    /// tie, and comes back the same way.
    #[test]
    fn the_gate_walks_off_the_top_into_the_tie() {
        let mut track = drum_track();
        for _ in 0..40 {
            dispatch(&mut track, SeqOp::NudgeGate(1));
        }
        assert_eq!(seq(&track).step().gate, Step::TIE);
        dispatch(&mut track, SeqOp::NudgeGate(-1));
        assert_eq!(seq(&track).step().gate, Step::MAX_GATE);
        for _ in 0..80 {
            dispatch(&mut track, SeqOp::NudgeGate(-1));
        }
        assert_eq!(seq(&track).step().gate, Step::MIN_GATE);
    }

    /// Root-below composes with every voicing rather than being a fifth
    /// entry in the list, so toggling it does not disturb the voicing.
    #[test]
    fn root_below_is_independent_of_the_voicing() {
        let mut track = melodic_track();
        dispatch(&mut track, SeqOp::CycleVoicing(1));
        dispatch(&mut track, SeqOp::ToggleRootBelow);
        assert_eq!(seq(&track).step().voicing_kind(), Voicing::Drop2);
        assert!(seq(&track).step().root_below());
        dispatch(&mut track, SeqOp::CycleVoicing(1));
        assert_eq!(seq(&track).step().voicing_kind(), Voicing::First);
        assert!(seq(&track).step().root_below(), "the voicing wiped the bass double");
    }

    /// Swapping one drum machine for another leaves the lanes where the
    /// player put them; swapping a kit for a keyboard cannot.
    #[test]
    fn changing_the_child_relays_the_lanes_only_when_the_kind_changes() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SetLaneNote(75));
        let effect = dispatch(&mut track, SeqOp::SetChild(InstrumentType::DrumRack));
        assert!(effect.is_nothing(), "the same child is not a change");
        assert_eq!(seq(&track).lane().note, 75);

        let effect = dispatch(&mut track, SeqOp::SetChild(InstrumentType::Juno60));
        assert!(effect.child);
        assert_eq!(effect.patterns, 0xFF, "every slot's lanes moved");
        assert!(seq(&track).lane().is_pitched());
        assert_eq!(track.instrument_type, Some(InstrumentType::Juno60));
        assert_eq!(
            track.synth_params.len(),
            crate::preset::param_count(InstrumentType::Juno60)
        );

        // ...and back again.
        dispatch(&mut track, SeqOp::SetChild(InstrumentType::DrumRack));
        assert_eq!(seq(&track).lane().note, DEFAULT_DRUM_LANES[0]);
    }

    /// A sequencer cannot become its own child.
    #[test]
    fn a_sequencer_cannot_drive_a_sequencer() {
        let mut track = drum_track();
        assert!(dispatch(&mut track, SeqOp::SetChild(InstrumentType::Sequencer)).is_nothing());
        assert_eq!(track.instrument_type, Some(InstrumentType::DrumRack));
    }

    /// Queueing the slot that is already playing is not a switch — the audio
    /// thread ignores it, and the UI should not be counting down to it.
    #[test]
    fn queueing_the_live_slot_is_not_a_queue() {
        let mut track = drum_track();
        assert!(dispatch(&mut track, SeqOp::QueueSlot(0)).is_nothing());
        assert_eq!(seq(&track).queued_slot(), None);
        assert!(!dispatch(&mut track, SeqOp::QueueSlot(4)).is_nothing());
        assert_eq!(seq(&track).queued_slot(), Some(4));
    }

    /// A chain owns the slot, so a queue against one would be a number on
    /// screen that nothing ever acts on.
    #[test]
    fn a_chain_takes_over_from_the_queue() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::QueueSlot(2));
        dispatch(&mut track, SeqOp::PushChainEntry { slot: 0, repeats: 2 });
        assert_eq!(seq(&track).queued_slot(), None);
        assert!(dispatch(&mut track, SeqOp::QueueSlot(3)).is_nothing());

        dispatch(&mut track, SeqOp::ClearChain);
        assert!(!seq(&track).is_chained());
        assert!(!dispatch(&mut track, SeqOp::QueueSlot(3)).is_nothing());
    }

    #[test]
    fn chain_entries_can_be_added_edited_and_removed() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::PushChainEntry { slot: 0, repeats: 4 });
        dispatch(&mut track, SeqOp::PushChainEntry { slot: 1, repeats: 0 });
        dispatch(&mut track, SeqOp::PushChainEntry { slot: 2, repeats: 3 });
        assert_eq!(
            seq(&track).chain().iter().map(|e| (e.slot, e.repeats)).collect::<Vec<_>>(),
            vec![(0, 4), (1, 1), (2, 3)],
            "a repeat count of zero is one time through"
        );

        dispatch(&mut track, SeqOp::SetChainRepeats { index: 1, repeats: 8 });
        dispatch(&mut track, SeqOp::RemoveChainEntry(0));
        assert_eq!(
            seq(&track).chain().iter().map(|e| (e.slot, e.repeats)).collect::<Vec<_>>(),
            vec![(1, 8), (2, 3)]
        );

        assert!(dispatch(&mut track, SeqOp::RemoveChainEntry(9)).is_nothing());
    }

    /// Sixteen entries, and the seventeenth is refused rather than
    /// overwriting one.
    #[test]
    fn the_chain_is_bounded() {
        let mut track = drum_track();
        for _ in 0..MAX_CHAIN + 4 {
            dispatch(&mut track, SeqOp::PushChainEntry { slot: 1, repeats: 1 });
        }
        assert_eq!(seq(&track).chain().len(), MAX_CHAIN);
    }

    #[test]
    fn copying_a_pattern_marks_the_slot_it_landed_in() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::ToggleStep);
        let effect = dispatch(&mut track, SeqOp::CopyPattern { from: 0, to: 5 });
        assert_eq!(effect.slots().collect::<Vec<_>>(), vec![5]);
        assert!(seq(&track).pattern_at(5).lanes[0].steps[0].on);
        assert!(dispatch(&mut track, SeqOp::CopyPattern { from: 3, to: 3 }).is_nothing());
    }

    /// Clearing a kit pattern leaves it a kit pattern.
    #[test]
    fn clearing_a_pattern_leaves_the_lanes_pointed_where_they_were() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::ToggleStep);
        dispatch(&mut track, SeqOp::ClearPattern);
        assert!(!seq(&track).step().on);
        assert_eq!(seq(&track).lane().note, DEFAULT_DRUM_LANES[0]);
    }

    // ── Step record ──

    /// The thing the sequencer is unusable without: playing a key writes the
    /// pitch and moves on, so entering a line takes as long as playing it.
    #[test]
    fn step_record_writes_a_note_and_advances() {
        let mut track = melodic_track();
        // Nothing happens until it is armed.
        assert!(dispatch(&mut track, SeqOp::RecordNotes(HeldNotes::new(&[64]))).is_nothing());

        dispatch(&mut track, SeqOp::ArmStepRecord(true));
        dispatch(&mut track, SeqOp::RecordNotes(HeldNotes::new(&[64])));
        assert!(seq(&track).pattern().lanes[0].steps[0].on);
        assert_eq!(seq(&track).pattern().lanes[0].steps[0].root(), 64);
        assert_eq!(seq(&track).step_cursor(), 1);

        // A rest moves on without writing.
        dispatch(&mut track, SeqOp::RecordRest);
        assert!(!seq(&track).pattern().lanes[0].steps[1].on);
        assert_eq!(seq(&track).step_cursor(), 2);
    }

    /// Several keys at once are a chord, and they are stored as one — root,
    /// quality and voicing — rather than as the lowest note played.
    #[test]
    fn step_record_names_the_chord_that_was_played() {
        let mut track = melodic_track();
        dispatch(&mut track, SeqOp::ArmStepRecord(true));
        dispatch(&mut track, SeqOp::RecordNotes(HeldNotes::new(&[60, 63, 67, 70])));

        let step = seq(&track).pattern().lanes[0].steps[0];
        assert_eq!(step.root(), 60);
        assert_eq!(step.chord_kind(), Chord::Min7);
        assert_eq!(step.voicing_kind(), Voicing::Close);
    }

    /// A tie extends the step that was just written, which is the one behind
    /// the cursor.
    #[test]
    fn step_record_ties_the_step_behind_the_cursor() {
        let mut track = melodic_track();
        dispatch(&mut track, SeqOp::ArmStepRecord(true));
        dispatch(&mut track, SeqOp::RecordNotes(HeldNotes::new(&[60])));
        dispatch(&mut track, SeqOp::RecordTie);
        assert_eq!(seq(&track).pattern().lanes[0].steps[0].gate, Step::TIE);
        assert_eq!(seq(&track).step_cursor(), 2);
    }

    /// A drum lane has no pitch, so a played pad writes a hit on the lane it
    /// belongs to rather than retuning it.
    #[test]
    fn step_record_on_a_drum_lane_writes_a_hit() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::ArmStepRecord(true));
        dispatch(&mut track, SeqOp::RecordNotes(HeldNotes::new(&[38])));
        assert!(seq(&track).pattern().lanes[0].steps[0].on);
        assert_eq!(seq(&track).pattern().lanes[0].steps[0].octave, Step::silent().octave);
        assert_eq!(seq(&track).lane().note, DEFAULT_DRUM_LANES[0]);
    }

    /// A held chord with no name in the table still writes something: the
    /// note the player would expect to hear, rather than nothing at all.
    #[test]
    fn an_unnameable_chord_falls_back_to_its_lowest_note() {
        let mut track = melodic_track();
        dispatch(&mut track, SeqOp::ArmStepRecord(true));
        dispatch(&mut track, SeqOp::RecordNotes(HeldNotes::new(&[60, 61, 62])));
        let step = seq(&track).pattern().lanes[0].steps[0];
        assert_eq!(step.root(), 60);
        assert_eq!(step.chord_kind(), Chord::None);
    }

    #[test]
    fn held_notes_are_bounded_and_sorted() {
        let held = HeldNotes::new(&[67, 60, 64, 72, 76, 79, 83]);
        assert_eq!(held.as_slice(), &[60, 64, 67, 72, 76]);
        assert!(HeldNotes::new(&[]).as_slice().is_empty());
    }

    /// A lane with no name of its own is a lane that takes pitch from its
    /// steps; setting a note pins it.
    #[test]
    fn a_lane_can_be_pinned_and_unpinned() {
        let mut track = melodic_track();
        assert!(seq(&track).lane().is_pitched());
        dispatch(&mut track, SeqOp::SetLaneNote(42));
        assert!(!seq(&track).lane().is_pitched());
        dispatch(&mut track, SeqOp::SetLaneNote(Lane::FROM_STEP));
        assert!(seq(&track).lane().is_pitched());
    }

    #[test]
    fn mute_and_solo_are_per_lane() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SelectLane(2));
        dispatch(&mut track, SeqOp::ToggleLaneMute);
        assert!(seq(&track).pattern().lanes[2].muted);
        assert!(!seq(&track).pattern().lanes[0].muted);
        dispatch(&mut track, SeqOp::ToggleLaneSolo);
        assert!(seq(&track).pattern().lanes[2].soloed);
    }

    /// Everything with a range has one at both ends, and no sequence of
    /// presses can put a value outside it.
    #[test]
    fn every_control_stops_at_its_ends() {
        let mut track = drum_track();
        for _ in 0..200 {
            dispatch(&mut track, SeqOp::NudgeSwing(1));
            dispatch(&mut track, SeqOp::NudgeBaseVelocity(1));
            dispatch(&mut track, SeqOp::NudgeAccentVelocity(1));
            dispatch(&mut track, SeqOp::NudgeDefaultGate(1));
            dispatch(&mut track, SeqOp::CycleRate(1));
            dispatch(&mut track, SeqOp::NudgeGate(1));
            dispatch(&mut track, SeqOp::CycleLength(1));
            dispatch(&mut track, SeqOp::CycleMode(1));
            dispatch(&mut track, SeqOp::CycleSwitchQuant(1));
        }
        {
            let block = seq(&track).pattern();
            assert_eq!(block.swing, PatternBlock::MAX_SWING);
            assert_eq!(block.base_vel, 127);
            assert_eq!(block.accent_vel, 127);
            assert_eq!(block.default_gate, Step::MAX_GATE);
            assert_eq!(block.rate, Rate::SixteenthTriplet);
            assert_eq!(block.steps, 32);
            assert_eq!(block.mode, Mode::Locrian);
        }
        assert_eq!(seq(&track).switch_quant(), SwitchQuant::Immediate);

        for _ in 0..200 {
            dispatch(&mut track, SeqOp::NudgeSwing(-1));
            dispatch(&mut track, SeqOp::NudgeBaseVelocity(-1));
            dispatch(&mut track, SeqOp::NudgeAccentVelocity(-1));
            dispatch(&mut track, SeqOp::NudgeDefaultGate(-1));
            dispatch(&mut track, SeqOp::CycleRate(-1));
            dispatch(&mut track, SeqOp::CycleLength(-1));
            dispatch(&mut track, SeqOp::CycleMode(-1));
            dispatch(&mut track, SeqOp::CycleSwitchQuant(-1));
        }
        {
            let block = seq(&track).pattern();
            assert_eq!(block.swing, PatternBlock::MIN_SWING);
            assert_eq!(block.base_vel, 1);
            assert_eq!(block.default_gate, Step::MIN_GATE);
            assert_eq!(block.rate, Rate::Quarter);
            assert_eq!(block.steps, 4);
            assert_eq!(block.mode, Mode::Chromatic);
        }
        assert_eq!(seq(&track).switch_quant(), SwitchQuant::PatternEnd);
    }

    /// An index out of range is clamped rather than panicking. Every one of
    /// these can arrive from a controller mapping.
    #[test]
    fn out_of_range_indices_are_clamped() {
        let mut track = drum_track();
        dispatch(&mut track, SeqOp::SelectSlot(200));
        dispatch(&mut track, SeqOp::SelectLane(200));
        dispatch(&mut track, SeqOp::SelectStep(200));
        dispatch(&mut track, SeqOp::QueueSlot(200));
        dispatch(&mut track, SeqOp::SetTonic(200));
        dispatch(&mut track, SeqOp::CopyPattern { from: 200, to: 201 });
        let state = seq(&track);
        assert_eq!(state.selected_slot(), SLOTS as u8 - 1);
        assert_eq!(state.lane_cursor(), LANES - 1);
        assert_eq!(state.step_cursor(), MAX_STEPS - 1);
        assert_eq!(state.queued_slot(), Some(SLOTS as u8 - 1));
        assert!(state.pattern().tonic < 12);
    }
}
