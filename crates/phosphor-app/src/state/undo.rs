//! Undo/redo — every change is a step holding the state it replaced.
//!
//! A step stores the touched slice of the project twice: as it was before
//! the change and as it was after. Undo puts `before` back, redo puts
//! `after` back, and the code that applies a slice cannot tell which
//! direction it is working in. That symmetry is the whole design: a change
//! that can be undone can always be redone, and a new kind of change is
//! mapped by capturing its slice — never by writing its inverse by hand,
//! which is how three kinds of redo quietly went missing from the first
//! version of this file.
//!
//! Mapping a mutation is two lines around it:
//!
//! ```ignore
//! let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx });
//! /* ...mutate clips however you like... */
//! nav.commit_undo(before, "move note");
//! ```
//!
//! `commit_undo` captures the same scope again as `after`, drops the step
//! when nothing actually changed — so a gesture that hit a wall does not
//! eat a press of `u` — and pushes it. Steps the recorder commits go
//! through [`NavState::commit_undo_take`] instead, which marks them as
//! takes: the ones undo may peel while the transport is still recording.

use super::{Clip, FxInstance, InstrumentType, NavState, TrackState};

/// Addresses one slice of undoable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoScope {
    /// A track's entire clip list — notes, positions, lengths, existence.
    ///
    /// The whole list rather than one clip, because the operations that
    /// need more than one are real: a recorded take can absorb every clip
    /// it covers, and undo has to put all of them back at once.
    TrackClips { track_idx: usize },
    /// A whole track, existence included.
    Track { track_idx: usize },
    /// A strip's insert chain — slots, order, bypass switches, parameters.
    /// The bus strips are rows in the same list, so one scope covers a
    /// track's inserts and a bus's alike.
    TrackFx { track_idx: usize },
    /// An instrument panel's parameter block, patch selectors included.
    SynthParams { track_idx: usize },
    /// A strip's mix position: fader, pan, sends, mute. Not solo — solo is
    /// audition state, like a selection, and undoing an edit should never
    /// un-audition a track. Not arm — that is the transport's business.
    TrackMix { track_idx: usize },
    /// A step sequencer's music — patterns, chain, switch quantize, and the
    /// editing cursors, but never its run state. See
    /// [`crate::sequencer::SeqContent`].
    Sequencer { track_idx: usize },
    /// A sequencer track's child instrument, whole: which instrument sits
    /// in the plugin slot, its entire panel, and the sequencer content —
    /// because swapping a drum machine for a keyboard re-lays the lanes,
    /// and undoing the swap has to bring the lanes back with it.
    SeqChild { track_idx: usize },
    /// A track's name — the one field of a track that renames without
    /// touching anything the audio thread holds.
    TrackName { track_idx: usize },
    /// The transport's tempo, read from and applied through the
    /// [`NavState::tempo_bpm`] mirror — every tempo edit refreshes the
    /// mirror in the same breath, which is what makes the mirror safe to
    /// checkpoint from.
    Tempo,
    /// The loop brace's *range*. Its on/off switch is transport state and
    /// stays off the stack: undoing an edit must not stop the loop.
    LoopRange,
}

/// A captured slice: the scope plus everything that was in it.
#[derive(Debug, Clone)]
pub enum StateSlice {
    TrackClips { track_idx: usize, clips: Vec<Clip> },
    /// `None` means no track lived at this index — which is how one slice
    /// type captures delete (before: some, after: none) and create
    /// (before: none, after: some) without either being a special case.
    Track { track_idx: usize, track: Option<Box<TrackState>> },
    TrackFx { track_idx: usize, chain: Vec<FxInstance> },
    SynthParams { track_idx: usize, params: Vec<f32> },
    TrackMix { track_idx: usize, volume: f32, pan: f32, sends: [f32; 2], muted: bool },
    /// `None` when the track had no sequencer — captured for completeness,
    /// applied as a no-op, and never produced by the capture sites in
    /// practice: only sequencer edits checkpoint this scope.
    Sequencer { track_idx: usize, content: Option<Box<crate::sequencer::SeqContent>> },
    SeqChild {
        track_idx: usize,
        instrument: Option<InstrumentType>,
        params: Vec<f32>,
        content: Option<Box<crate::sequencer::SeqContent>>,
    },
    TrackName { track_idx: usize, name: String },
    Tempo { bpm: f32 },
    LoopRange { start_bar: u32, end_bar: u32 },
}

impl StateSlice {
    /// Clone the current contents of `scope` out of the navigation state.
    pub fn capture(nav: &NavState, scope: UndoScope) -> Self {
        match scope {
            UndoScope::TrackClips { track_idx } => Self::TrackClips {
                track_idx,
                clips: nav.tracks.get(track_idx).map(|t| t.clips.clone()).unwrap_or_default(),
            },
            UndoScope::Track { track_idx } => Self::Track {
                track_idx,
                track: nav.tracks.get(track_idx).map(|t| Box::new(t.clone())),
            },
            UndoScope::TrackFx { track_idx } => Self::TrackFx {
                track_idx,
                chain: nav.tracks.get(track_idx).map(|t| t.fx_chain.clone()).unwrap_or_default(),
            },
            UndoScope::SynthParams { track_idx } => Self::SynthParams {
                track_idx,
                params: nav.tracks.get(track_idx).map(|t| t.synth_params.clone()).unwrap_or_default(),
            },
            UndoScope::TrackMix { track_idx } => {
                let track = nav.tracks.get(track_idx);
                Self::TrackMix {
                    track_idx,
                    volume: track.map(|t| t.volume).unwrap_or(1.0),
                    pan: track.map(|t| t.pan).unwrap_or(0.0),
                    sends: track.map(|t| t.sends).unwrap_or([0.0; 2]),
                    muted: track.map(|t| t.muted).unwrap_or(false),
                }
            }
            UndoScope::Sequencer { track_idx } => Self::Sequencer {
                track_idx,
                content: nav
                    .tracks
                    .get(track_idx)
                    .and_then(|t| t.sequencer.as_ref())
                    .map(|s| Box::new(s.content())),
            },
            UndoScope::SeqChild { track_idx } => {
                let track = nav.tracks.get(track_idx);
                Self::SeqChild {
                    track_idx,
                    instrument: track.and_then(|t| t.instrument_type),
                    params: track.map(|t| t.synth_params.clone()).unwrap_or_default(),
                    content: track
                        .and_then(|t| t.sequencer.as_ref())
                        .map(|s| Box::new(s.content())),
                }
            }
            UndoScope::TrackName { track_idx } => Self::TrackName {
                track_idx,
                name: nav.tracks.get(track_idx).map(|t| t.name.clone()).unwrap_or_default(),
            },
            UndoScope::Tempo => Self::Tempo { bpm: nav.tempo_bpm },
            UndoScope::LoopRange => Self::LoopRange {
                start_bar: nav.loop_editor.start_bar,
                end_bar: nav.loop_editor.end_bar,
            },
        }
    }

    pub fn scope(&self) -> UndoScope {
        match self {
            Self::TrackClips { track_idx, .. } => UndoScope::TrackClips { track_idx: *track_idx },
            Self::Track { track_idx, .. } => UndoScope::Track { track_idx: *track_idx },
            Self::TrackFx { track_idx, .. } => UndoScope::TrackFx { track_idx: *track_idx },
            Self::SynthParams { track_idx, .. } => UndoScope::SynthParams { track_idx: *track_idx },
            Self::TrackMix { track_idx, .. } => UndoScope::TrackMix { track_idx: *track_idx },
            Self::Sequencer { track_idx, .. } => UndoScope::Sequencer { track_idx: *track_idx },
            Self::SeqChild { track_idx, .. } => UndoScope::SeqChild { track_idx: *track_idx },
            Self::TrackName { track_idx, .. } => UndoScope::TrackName { track_idx: *track_idx },
            Self::Tempo { .. } => UndoScope::Tempo,
            Self::LoopRange { .. } => UndoScope::LoopRange,
        }
    }

    /// Whether two captures of the same scope hold the same state.
    ///
    /// Clip and insert slices can answer: they are plain data (an
    /// [`FxInstance`]'s equality already knows to ignore its meter). A track
    /// slice says "changed" unconditionally — every operation that captures
    /// one really does create or delete a track, and a false "changed" costs
    /// one harmless step where a false "unchanged" would eat one.
    fn same_as(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::TrackClips { track_idx: a, clips: ca },
                Self::TrackClips { track_idx: b, clips: cb },
            ) => a == b && ca == cb,
            (
                Self::TrackFx { track_idx: a, chain: ca },
                Self::TrackFx { track_idx: b, chain: cb },
            ) => a == b && ca == cb,
            (
                Self::SynthParams { track_idx: a, params: pa },
                Self::SynthParams { track_idx: b, params: pb },
            ) => a == b && pa == pb,
            (
                Self::TrackMix {
                    track_idx: a, volume: va, pan: pa, sends: sa, muted: ma,
                },
                Self::TrackMix {
                    track_idx: b, volume: vb, pan: pb, sends: sb, muted: mb,
                },
            ) => a == b && va == vb && pa == pb && sa == sb && ma == mb,
            (
                Self::Sequencer { track_idx: a, content: ca },
                Self::Sequencer { track_idx: b, content: cb },
            ) => a == b && ca == cb,
            (
                Self::SeqChild { track_idx: a, instrument: ia, params: pa, content: ca },
                Self::SeqChild { track_idx: b, instrument: ib, params: pb, content: cb },
            ) => a == b && ia == ib && pa == pb && ca == cb,
            (
                Self::TrackName { track_idx: a, name: na },
                Self::TrackName { track_idx: b, name: nb },
            ) => a == b && na == nb,
            (Self::Tempo { bpm: a }, Self::Tempo { bpm: b }) => a == b,
            (
                Self::LoopRange { start_bar: sa, end_bar: ea },
                Self::LoopRange { start_bar: sb, end_bar: eb },
            ) => sa == sb && ea == eb,
            _ => false,
        }
    }
}

/// Names the continuous control a step came from, so that a sweep — forty
/// presses of `l` on one knob — folds into one step instead of forty.
///
/// The grain is the *panel or control group*, not the single parameter, on
/// purpose: some single gestures write two parameters at once (switching a
/// compressor off auto-makeup seeds the manual makeup from it), and two
/// steps for one keypress means undo lands on a state the player never saw.
/// The cost is that two different knobs on the same slot touched within the
/// window merge too, which reads as "undo my last burst of tweaking".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoGesture {
    /// One effect slot's controls.
    FxSlot { track_idx: usize, slot: usize },
    /// An instrument panel's controls.
    SynthPanel { track_idx: usize },
    /// A strip's fader.
    Fader { track_idx: usize },
    /// A strip's pan or one of its sends.
    Routing { track_idx: usize },
    /// A sequencer's grid and knobs.
    Sequencer { track_idx: usize },
    /// The knob that walks a sequencer's child instrument list. Its own
    /// gesture, so a flick through five instruments is one step back to
    /// the one the player left — and never folds into a pattern sweep.
    ChildSwap { track_idx: usize },
    /// Drawing on an automation lane — a whole sweep across columns folds
    /// into one step, so one `u` lifts the curve back to where it began.
    Automation { track_idx: usize },
    /// Riding note velocities in the editor — held presses fold, so one
    /// `u` puts the dynamics back where the ride began.
    Velocity { track_idx: usize },
    /// The transport's tempo.
    Tempo,
    /// The loop brace.
    LoopRange,
}

/// How long a gesture stays open: a commit with the same gesture inside
/// this window folds into the step before it.
const GESTURE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

/// One undoable change.
#[derive(Debug, Clone)]
pub struct UndoStep {
    pub before: StateSlice,
    pub after: StateSlice,
    /// What the status bar calls this change — "draw note", "delete clip",
    /// "take". Named at the call site, printed by undo and redo alike.
    pub label: &'static str,
    /// True when the recorder committed this step. While the transport is
    /// recording, `u` peels takes and only takes — it must not reach past
    /// the recording and eat an edit made before it.
    pub is_take: bool,
    /// The continuous control this step came from and when it last moved,
    /// for steps that are part of a sweep. `None` for discrete edits, which
    /// never fold into anything.
    gesture: Option<(UndoGesture, std::time::Instant)>,
}

impl NavState {
    /// Capture `scope` before mutating it. Pair with [`Self::commit_undo`].
    #[must_use]
    pub fn undo_checkpoint(&self, scope: UndoScope) -> StateSlice {
        StateSlice::capture(self, scope)
    }

    /// Push the step for a mutation that already happened. A no-op when the
    /// scope holds exactly what the checkpoint saw.
    pub fn commit_undo(&mut self, before: StateSlice, label: &'static str) {
        self.commit_step(before, label, false);
    }

    /// [`Self::commit_undo`], marked as a recorded take.
    pub fn commit_undo_take(&mut self, before: StateSlice) {
        self.commit_step(before, "take", true);
    }

    fn commit_step(&mut self, before: StateSlice, label: &'static str, is_take: bool) {
        let after = StateSlice::capture(self, before.scope());
        if before.same_as(&after) {
            return;
        }
        self.undo_stack.push(UndoStep { before, after, label, is_take, gesture: None });
    }

    /// [`Self::commit_undo`] for a continuous control: successive commits
    /// with the same gesture inside [`GESTURE_WINDOW`] fold into one step,
    /// so a sweep is one press of `u`, not one per tick of the knob.
    ///
    /// The fold keeps the first commit's `before` — the state the sweep
    /// started from — and takes the newest `after`. A sweep that comes all
    /// the way back to where it began dissolves: the step is dropped rather
    /// than left as a change that changes nothing.
    pub fn commit_undo_coalesced(
        &mut self,
        before: StateSlice,
        label: &'static str,
        gesture: UndoGesture,
    ) {
        let after = StateSlice::capture(self, before.scope());
        if before.same_as(&after) {
            return;
        }
        let now = std::time::Instant::now();
        if let Some(top) = self.undo_stack.top_mut() {
            if let Some((key, at)) = top.gesture {
                // The scope check is belt and braces: gesture keys carry
                // their scope in practice, but a fold across two different
                // slices would weld half of one change onto half of another.
                if key == gesture
                    && now.duration_since(at) <= GESTURE_WINDOW
                    && top.before.scope() == before.scope()
                {
                    top.after = after;
                    top.gesture = Some((gesture, now));
                    if top.before.same_as(&top.after) {
                        self.undo_stack.drop_top();
                    }
                    return;
                }
            }
        }
        self.undo_stack.push(UndoStep {
            before,
            after,
            label,
            is_take: false,
            gesture: Some((gesture, now)),
        });
    }

    /// Push a step whose two sides the caller built by hand.
    ///
    /// For changes to a track's *existence*. The checkpoint/commit pair
    /// cannot capture those: after a delete, re-capturing the same index
    /// would photograph whichever track slid into the gap, and the step
    /// would redo into the wrong track. Deleting and creating construct
    /// their `Some`/`None` sides explicitly instead.
    pub fn push_undo_step(&mut self, before: StateSlice, after: StateSlice, label: &'static str) {
        self.undo_stack.push(UndoStep { before, after, label, is_take: false, gesture: None });
    }
}

/// Undo/redo stack.
#[derive(Debug, Default)]
pub struct UndoStack {
    undo: Vec<UndoStep>,
    redo: Vec<UndoStep>,
}

impl UndoStack {
    /// How much history is kept. Old steps fall off the far end.
    const MAX: usize = 100;

    pub fn new() -> Self {
        Self::default()
    }

    /// Push a new step. Clears the redo stack (new timeline branch).
    pub fn push(&mut self, step: UndoStep) {
        self.undo.push(step);
        self.redo.clear();
        if self.undo.len() > Self::MAX {
            self.undo.remove(0);
        }
    }

    /// Push to undo stack WITHOUT clearing redo (used during redo).
    pub fn push_undo_only(&mut self, step: UndoStep) {
        self.undo.push(step);
        if self.undo.len() > Self::MAX {
            self.undo.remove(0);
        }
    }

    /// Pop the last step (for undoing).
    pub fn pop_undo(&mut self) -> Option<UndoStep> {
        self.undo.pop()
    }

    /// The newest step, for a gesture deciding whether to fold into it.
    fn top_mut(&mut self) -> Option<&mut UndoStep> {
        self.undo.last_mut()
    }

    /// Drop the newest step without touching redo — for a sweep that came
    /// all the way back to its own starting point.
    fn drop_top(&mut self) {
        self.undo.pop();
    }

    /// Push a step to the redo stack (after undoing it).
    pub fn push_redo(&mut self, step: UndoStep) {
        self.redo.push(step);
    }

    /// Pop from redo stack (for redoing).
    pub fn pop_redo(&mut self) -> Option<UndoStep> {
        self.redo.pop()
    }

    /// Drop all history, both directions. For the moments the steps' world
    /// stops existing: a session load rebuilds every track, and a step that
    /// remembers the old ones would restore them into the new session.
    /// A save is not such a moment — undoing past a save point is normal.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }

    /// Whether the next undo would peel a recorded take.
    pub fn top_is_take(&self) -> bool {
        self.undo.last().is_some_and(|s| s.is_take)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::initial_tracks;

    fn nav_with_clip() -> NavState {
        let mut nav = NavState::new(initial_tracks());
        let mut track = TrackState::new(
            "synth", 0, true, phosphor_core::project::TrackKind::Instrument, vec![],
        );
        track.clips.push(Clip {
            number: 1,
            width: 4,
            has_content: true,
            start_tick: 0,
            length_ticks: 3840,
            notes: vec![phosphor_core::clip::NoteSnapshot {
                note: 60, velocity: 100, start_frac: 0.0, duration_frac: 0.25, muted: false
            }],
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        });
        nav.tracks.insert(0, track);
        nav
    }

    /// The capture/commit pair records exactly the states either side of
    /// the mutation, and both survive on the step.
    #[test]
    fn a_step_holds_both_sides_of_the_change() {
        let mut nav = nav_with_clip();
        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].notes.clear();
        nav.commit_undo(before, "delete notes");

        let step = nav.undo_stack.pop_undo().expect("a step was pushed");
        let StateSlice::TrackClips { clips: before_clips, .. } = &step.before else {
            panic!("wrong slice kind");
        };
        let StateSlice::TrackClips { clips: after_clips, .. } = &step.after else {
            panic!("wrong slice kind");
        };
        assert_eq!(before_clips[0].notes.len(), 1);
        assert_eq!(after_clips[0].notes.len(), 0);
        assert_eq!(step.label, "delete notes");
        assert!(!step.is_take);
    }

    /// A mutation that changed nothing pushes nothing — a gesture that hit
    /// a wall must not eat a press of `u`, and must not clear redo.
    #[test]
    fn an_unchanged_scope_pushes_no_step() {
        let mut nav = nav_with_clip();

        // Seed one real step, undo it, so the redo stack holds something.
        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].notes.clear();
        nav.commit_undo(before, "delete notes");
        let step = nav.undo_stack.pop_undo().unwrap();
        nav.undo_stack.push_redo(step);
        assert!(nav.undo_stack.can_redo());

        // A checkpoint committed with no mutation between.
        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.commit_undo(before, "nothing");

        assert!(!nav.undo_stack.can_undo(), "a no-op change was pushed");
        assert!(nav.undo_stack.can_redo(), "a no-op change cleared redo");
    }

    /// A real push is a new timeline branch: redo dies.
    #[test]
    fn a_real_change_clears_redo() {
        let mut nav = nav_with_clip();
        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].notes.clear();
        nav.commit_undo(before, "delete notes");
        let step = nav.undo_stack.pop_undo().unwrap();
        nav.undo_stack.push_redo(step);

        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].start_tick = 960;
        nav.commit_undo(before, "move clip");

        assert!(!nav.undo_stack.can_redo(), "new work left a stale redo branch");
    }

    /// Takes are marked, and the mark is visible from the top of the stack
    /// — that is what lets `u` refuse to reach past the recording.
    #[test]
    fn takes_are_marked_and_visible() {
        let mut nav = nav_with_clip();
        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].notes.push(phosphor_core::clip::NoteSnapshot {
            note: 62, velocity: 100, start_frac: 0.5, duration_frac: 0.25, muted: false
        });
        nav.commit_undo_take(before);
        assert!(nav.undo_stack.top_is_take());

        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].notes.clear();
        nav.commit_undo(before, "delete notes");
        assert!(!nav.undo_stack.top_is_take());
    }

    // ── Gestures ──

    fn nav_with_fx() -> NavState {
        let mut nav = nav_with_clip();
        nav.tracks[0].fx_chain.push(FxInstance {
            fx_type: crate::state::FxType::Eq,
            bypass: false,
            params: vec![0.0, 1.0, 2.0],
            gr: None,
        });
        nav
    }

    fn turn_knob(nav: &mut NavState, value: f32) {
        let before = nav.undo_checkpoint(UndoScope::TrackFx { track_idx: 0 });
        nav.tracks[0].fx_chain[0].params[0] = value;
        nav.commit_undo_coalesced(
            before,
            "adjust effect",
            UndoGesture::FxSlot { track_idx: 0, slot: 0 },
        );
    }

    /// A sweep is one step: the first commit's `before`, the last one's
    /// `after`, and one press of `u` between the player and where they
    /// started.
    #[test]
    fn a_sweep_folds_into_one_step() {
        let mut nav = nav_with_fx();
        turn_knob(&mut nav, 1.0);
        turn_knob(&mut nav, 2.0);
        turn_knob(&mut nav, 3.0);

        let step = nav.undo_stack.pop_undo().expect("the sweep left a step");
        assert!(nav.undo_stack.pop_undo().is_none(), "the sweep left more than one step");
        let StateSlice::TrackFx { chain: before, .. } = &step.before else { panic!() };
        let StateSlice::TrackFx { chain: after, .. } = &step.after else { panic!() };
        assert_eq!(before[0].params[0], 0.0, "the fold lost the sweep's starting point");
        assert_eq!(after[0].params[0], 3.0, "the fold lost the sweep's end");
    }

    /// A sweep that returns to its own starting point never happened.
    #[test]
    fn a_sweep_back_to_the_start_dissolves() {
        let mut nav = nav_with_fx();
        turn_knob(&mut nav, 5.0);
        turn_knob(&mut nav, 0.0);
        assert!(
            !nav.undo_stack.can_undo(),
            "a round trip left a step that changes nothing"
        );
    }

    /// Two different controls are two different steps, however fast the
    /// player moves between them.
    #[test]
    fn different_gestures_do_not_fold() {
        let mut nav = nav_with_fx();
        turn_knob(&mut nav, 1.0);
        let before = nav.undo_checkpoint(UndoScope::TrackFx { track_idx: 0 });
        nav.tracks[0].fx_chain[0].params[1] = 9.0;
        nav.commit_undo_coalesced(
            before,
            "adjust effect",
            UndoGesture::FxSlot { track_idx: 0, slot: 1 },
        );
        assert!(nav.undo_stack.pop_undo().is_some());
        assert!(nav.undo_stack.pop_undo().is_some(), "two gestures folded into one step");
    }

    /// A gesture goes stale: the same knob touched again after the window
    /// is a new thought and a new step.
    #[test]
    fn a_stale_gesture_starts_a_new_step() {
        let mut nav = nav_with_fx();
        turn_knob(&mut nav, 1.0);
        std::thread::sleep(GESTURE_WINDOW + std::time::Duration::from_millis(100));
        turn_knob(&mut nav, 2.0);
        assert!(nav.undo_stack.pop_undo().is_some());
        assert!(
            nav.undo_stack.pop_undo().is_some(),
            "a touch after the window folded into the old sweep"
        );
    }

    /// A discrete edit landing mid-sweep breaks the fold — the sweep's two
    /// halves must not merge across it, or undoing would reorder history.
    #[test]
    fn a_discrete_step_between_breaks_the_fold() {
        let mut nav = nav_with_fx();
        turn_knob(&mut nav, 1.0);

        let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
        nav.tracks[0].clips[0].notes.clear();
        nav.commit_undo(before, "delete notes");

        turn_knob(&mut nav, 2.0);
        let mut depth = 0;
        while nav.undo_stack.pop_undo().is_some() {
            depth += 1;
        }
        assert_eq!(depth, 3, "the sweep folded across a discrete edit");
    }

    /// History is bounded; the far end falls off first.
    #[test]
    fn the_stack_is_bounded() {
        let mut nav = nav_with_clip();
        for i in 0..(UndoStack::MAX + 10) {
            let before = nav.undo_checkpoint(UndoScope::TrackClips { track_idx: 0 });
            nav.tracks[0].clips[0].start_tick = i as i64 + 1;
            nav.commit_undo(before, "move clip");
        }
        let mut count = 0;
        while nav.undo_stack.pop_undo().is_some() {
            count += 1;
        }
        assert_eq!(count, UndoStack::MAX);
    }
}
