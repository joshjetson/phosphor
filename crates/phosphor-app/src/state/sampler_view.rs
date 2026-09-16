//! The pad map's cursors.
//!
//! Cursors only. What a pad *is* lives in
//! [`SamplerState`](crate::sampler::SamplerState) and is edited through the
//! sampler's ops, which are also the only things that tell the engine — the
//! same separation the step grid keeps between
//! [`SequencerView`](super::SequencerView) and its patterns.
//!
//! The pad under the cursor is deliberately *not* here: it lives on the
//! sampler itself, because playing a key moves it. A controller and a
//! keypress have to move the same cursor, and a cursor that lives in the
//! view would be a second one that disagrees.

use crate::sampler::trim::NudgeUnit;

/// The trim strip, while it is open over the pad map.
///
/// Only what the strip itself owns. *Which* layer is being trimmed is not
/// here: that is the layer cursor below, and a second copy of it would be a
/// second cursor that disagrees the moment a layer is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimView {
    /// How far one press of `h`/`l` moves an edge — `j`/`k` walk it.
    pub unit: NudgeUnit,
    /// Pull a nudged edge onto the nearest zero crossing. On by default:
    /// it is what stops a trim clicking, and a player who wants the frame
    /// they asked for presses `z`.
    pub snap: bool,
    /// `t` again: the region plays round and round while it is trimmed.
    pub looping: bool,
}

impl Default for TrimView {
    fn default() -> Self {
        Self { unit: NudgeUnit::default(), snap: true, looping: false }
    }
}

/// Where the cursor is standing in the pad panel.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SamplerView {
    /// Which control the cursor is on, as an index into
    /// [`PadKnob::visible`](crate::sampler::knobs::PadKnob::visible).
    pub knob: usize,
    /// Which layer of the pad the layer controls address, and which one
    /// `m` and `d` act on.
    pub layer: usize,
    /// Enter was pressed on a control: `h`/`l` now adjust it and nothing
    /// else gets a look at the key. The fader's contract, applied to a pad
    /// knob — and what lets `h`/`l` walk the keyboard bed the rest of the
    /// time.
    pub locked: bool,
    /// `t` opened the trim strip over the map, and it has the keys until
    /// `esc`. `Some` is the whole answer to "is it open": a flag beside the
    /// settings would let the two disagree.
    pub trim: Option<TrimView>,
    /// `R` armed root-learn on the zone under the cursor: the next key
    /// played teaches it where it was recorded, and nothing else happens to
    /// that key.
    ///
    /// Here rather than on the sampler because it is a question the screen
    /// is asking, not a fact about the kit — it must not go into a session,
    /// and undoing an edit must not re-arm it.
    pub root_learn: bool,
}

impl SamplerView {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Opening the view: the cursor at the top of the pad's controls, with
    /// nothing held and no strip open.
    pub fn focus(&mut self) {
        self.knob = 0;
        self.layer = 0;
        self.locked = false;
        self.trim = None;
        self.root_learn = false;
    }

    /// Move between controls, stopping at both ends. Walking off the end of
    /// a knob list and reappearing at the other end is how a value gets
    /// changed by accident.
    pub fn move_knob(&mut self, delta: i32, count: usize) {
        if self.locked || count == 0 {
            self.knob = self.knob.min(count.saturating_sub(1));
            return;
        }
        self.knob = (self.knob as i32 + delta).clamp(0, count as i32 - 1) as usize;
    }

    /// Move between the pad's layers, stopping at both ends.
    pub fn move_layer(&mut self, delta: i32, count: usize) {
        if count == 0 {
            self.layer = 0;
            return;
        }
        self.layer = (self.layer as i32 + delta).clamp(0, count as i32 - 1) as usize;
    }

    /// Pull both cursors back inside what the pad actually has.
    ///
    /// Called on the way into every frame and every keypress, because the
    /// pad under the cursor changes without the view being asked: playing a
    /// key moves the pad cursor, and the new pad may have fewer layers — or
    /// none, which takes six controls off the bottom of the panel.
    pub fn clamp(&mut self, knobs: usize, layers: usize) {
        if knobs == 0 {
            self.knob = 0;
        } else if self.knob >= knobs {
            self.knob = knobs - 1;
            // A held control that no longer exists cannot stay held: the
            // next `h` would turn whatever slid into its place.
            self.locked = false;
        }
        if self.layer >= layers {
            self.layer = layers.saturating_sub(1);
        }
        // A strip open over a pad that has nothing on it is a waveform of
        // nothing, and every key in it would be a refusal. Playing a key
        // moves the pad cursor, so this happens without anything being
        // pressed.
        if layers == 0 {
            self.trim = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::knobs::PadKnob;

    #[test]
    fn a_held_control_blocks_the_cursor_but_never_the_layer_list() {
        let mut view = SamplerView::new();
        view.move_knob(3, PadKnob::ALL.len());
        assert_eq!(view.knob, 3);
        view.locked = true;
        view.move_knob(1, PadKnob::ALL.len());
        assert_eq!(view.knob, 3, "the cursor moved out from under a held knob");
        // The layer cursor is a different control and keeps working.
        view.move_layer(1, 4);
        assert_eq!(view.layer, 1);
    }

    /// The pad under the cursor changed to one with fewer layers: the
    /// cursors come back inside it, and a lock on a control that no longer
    /// exists is released rather than left pointing at its replacement.
    #[test]
    fn cursors_follow_a_pad_that_lost_its_layers() {
        let mut view = SamplerView::new();
        view.move_knob(30, PadKnob::ALL.len());
        view.move_layer(7, 8);
        view.locked = true;
        assert_eq!(view.knob, PadKnob::ALL.len() - 1);

        view.clamp(PadKnob::PAD_CONTROLS, 0);
        assert_eq!(view.knob, PadKnob::PAD_CONTROLS - 1);
        assert_eq!(view.layer, 0);
        assert!(!view.locked, "a control that stopped existing stayed held");

        // A pad with room for the cursor leaves it alone, lock and all.
        view.locked = true;
        view.clamp(PadKnob::ALL.len(), 3);
        assert_eq!(view.knob, PadKnob::PAD_CONTROLS - 1);
        assert!(view.locked);
    }

    #[test]
    fn neither_cursor_walks_off_either_end() {
        let mut view = SamplerView::new();
        for _ in 0..50 {
            view.move_knob(-1, 5);
            view.move_layer(-1, 3);
        }
        assert_eq!((view.knob, view.layer), (0, 0));
        for _ in 0..50 {
            view.move_knob(1, 5);
            view.move_layer(1, 3);
        }
        assert_eq!((view.knob, view.layer), (4, 2));
        // An empty list is not a panic and not an index.
        view.move_knob(1, 0);
        view.move_layer(1, 0);
        assert_eq!((view.knob, view.layer), (0, 0));
    }
}
