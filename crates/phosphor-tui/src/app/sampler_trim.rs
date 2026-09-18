//! The trim strip's operations: opening it, moving an edge, and turning
//! the four switches beside the waveform.
//!
//! Every edit here follows [`super::sampler_ops`]'s rule to the letter —
//! state first, then the undo step, then [`App::sync_sampler_edit`] ships
//! every key the sound can be heard on. It lives in its own file rather
//! than in that one because the strip is a mode: it has its own keys, its
//! own cursor, its own arithmetic ([`phosphor_app::sampler::trim`]) and its
//! own way out.
//!
//! # The grain of undo
//!
//! A nudge run is one gesture. Holding `l` for a second is a player deciding
//! where the start goes, not thirty decisions, so the presses fold into one
//! step the way a knob sweep does — one `u` puts the marker back where the
//! run began. `r` does not fold: reversing a layer is a decision, and
//! [`App::toggle_sampler_layer_mute`] settled that grain already. Neither
//! does `w`, for the same reason and one more: it moves both markers at
//! once, and a player who placed one by hand before pressing it should get
//! their own trim back rather than losing it to the same `u`.
//!
//! # Every nudge makes a sound
//!
//! A trim marker is a position in a waveform nobody can hear by looking at
//! it. So a start nudge auditions from the new start, which is the only way
//! to know whether the click is off the front yet, and the audition goes
//! through the one door in [`super::sampler_ops`] that can also switch it
//! off again.

use super::*;

use phosphor_app::sampler::trim::{Hug, TrimEdge};
use phosphor_plugin::sample::PreviewMode;

use crate::state::undo::{UndoGesture, UndoScope};

impl App {
    /// The layer the strip is on, when there is one with audio behind it.
    ///
    /// The track and the layer's place in the stack, never the pad: which
    /// sound the stack belongs to is the sampler's own answer, and a second
    /// copy of it here would be the one that went stale in keys mode.
    fn trim_layer(&self) -> Option<(usize, usize)> {
        let track_idx = self.cursor_sampler_track()?;
        let cursor = self.nav.clip_view.sampler.layer;
        let sampler = self.nav.tracks[track_idx].sampler.as_ref()?;
        sampler.edited()?.layers.get(cursor)?.pcm.as_ref()?;
        Some((track_idx, cursor))
    }

    /// `t` on the pad map: open the strip over the layer under the cursor.
    ///
    /// A layer with no audio behind it is refused in words rather than
    /// opened onto an empty pane: the strip's whole subject is a waveform,
    /// and a missing file has none to show.
    pub(crate) fn open_trim_strip(&mut self) {
        if self.cursor_sampler_track().is_none() {
            return;
        }
        if self.trim_layer().is_none() {
            // Three ways to have nothing to trim, and they want different
            // answers: an empty pad wants `a`, a phrase row wants to be
            // told a performance is not a waveform, and a layer whose file
            // has gone wants the path fixed.
            self.flash(match self.sampler_row() {
                None => "nothing on this pad to trim \u{00b7} a loads a sound",
                Some(phosphor_app::sampler::PadRow::Phrase(_)) => {
                    "a phrase is notes, not a waveform \u{00b7} there is nothing to trim"
                }
                Some(_) => "this layer has lost its file \u{00b7} nothing to trim",
            });
            return;
        }
        self.nav.clip_view.sampler.trim = Some(crate::state::TrimView::default());
        // The sound before the first nudge: the region as it stands, so the
        // player hears what they are about to change.
        self.preview_sampler_layer(PreviewMode::Once);
        self.flash(
            "trim \u{00b7} h/l start \u{00b7} H/L end \u{00b7} w hug \u{00b7} j/k unit \
             \u{00b7} z snap \u{00b7} t loop",
        );
    }

    /// `esc` in the strip: back to the pad map, and quiet.
    pub(crate) fn close_trim_strip(&mut self) {
        self.nav.clip_view.sampler.trim = None;
        self.stop_sampler_preview();
    }

    /// `h`/`l` and `H`/`L`: move one edge of the region by one unit.
    pub(crate) fn nudge_trim(&mut self, edge: TrimEdge, delta: i32) {
        let Some(view) = self.nav.clip_view.sampler.trim else { return };
        let Some((track_idx, cursor)) = self.trim_layer() else {
            self.flash("nothing here to trim");
            return;
        };
        let bpm = self.engine.transport.tempo_bpm();

        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some((pad, _)) = sampler.edit_span() else { return };
        let Some(layer) = sampler.edited_mut().and_then(|p| p.layers.get_mut(cursor)) else {
            return;
        };
        let Some(nudge) = layer.nudge_trim(edge, delta, view.unit, bpm, view.snap) else {
            return;
        };
        let seconds = layer.edge_seconds(edge);
        let length = layer.seconds();
        // One step per run, not one per press: holding `l` is a player
        // deciding where the marker goes, and `u` should take back the
        // decision rather than the last thirtieth of it.
        self.nav.commit_undo_coalesced(
            before,
            "trim layer",
            UndoGesture::SamplerPad { track_idx, pad },
        );
        self.sync_sampler_edit(track_idx);
        // The audition follows the edge that moved. Moving the start and
        // hearing the old start is worse than hearing nothing.
        self.preview_sampler_layer(self.sampler_preview_mode());
        if nudge.floored {
            self.flash(format!(
                "{} \u{00b7} {MIN} ms is the shortest region",
                edge.label(),
                MIN = phosphor_app::sampler::trim::MIN_REGION_MS as i32,
            ));
        } else {
            self.flash(format!(
                "{} {seconds:.3}s \u{00b7} region {length:.3}s",
                edge.label(),
            ));
        }
    }

    /// `w`: pull both markers onto the audible part of the recording.
    ///
    /// The dead-air key. A take recorded with the player waiting for their
    /// own cue opens on a second of room tone, and a reversed one waits
    /// through the same second at the other end — which is a lot of `h` and
    /// `L` to find by eye. The rule is the one a fresh take is already
    /// trimmed by: [`LayerState::hug_audible`](phosphor_app::sampler::LayerState::hug_audible)
    /// calls the render's own thresholds rather than carrying a second copy.
    ///
    /// One step, and not a coalesced one: a hug is a decision the way a
    /// reverse is, and folding it into the nudge run before it would mean a
    /// player who trimmed by hand and then pressed `w` lost both to one `u`.
    pub(crate) fn hug_trim_region(&mut self) {
        let Some((track_idx, cursor)) = self.trim_layer() else {
            self.flash("nothing here to hug");
            return;
        };
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some(layer) = sampler.edited_mut().and_then(|p| p.layers.get_mut(cursor)) else {
            return;
        };
        let Some(hug) = layer.hug_audible() else { return };
        let (start, end) = match hug {
            Hug::Moved { start, end } => (start, end),
            // Nothing moved, so nothing is committed: a step that restores
            // the state it captured is a press of `u` that does nothing.
            Hug::Already => {
                self.flash("already hugged \u{00b7} both edges are on the sound");
                return;
            }
            Hug::Silent => {
                self.flash("this take is silent \u{00b7} there is nothing to hug");
                return;
            }
        };
        let length = layer.seconds();
        self.nav.commit_undo(before, "hug region");
        self.sync_sampler_edit(track_idx);
        // The region as it now stands, so the player hears what the key just
        // decided rather than taking the numbers on trust.
        self.preview_sampler_layer(self.sampler_preview_mode());
        self.flash(format!(
            "hugged \u{00b7} start {start:+.3}s \u{00b7} end {end:+.3}s \u{00b7} region {length:.3}s",
        ));
    }

    /// `j`/`k`: walk the nudge unit deeper or shallower.
    pub(crate) fn walk_trim_unit(&mut self, delta: i32) {
        let Some(view) = self.nav.clip_view.sampler.trim.as_mut() else { return };
        view.unit = view.unit.stepped(delta);
        let unit = view.unit;
        self.flash(format!("unit {} \u{00b7} j deeper \u{00b7} k wider", unit.label()));
    }

    /// `z`: zero-crossing snap on or off.
    pub(crate) fn toggle_trim_snap(&mut self) {
        let Some(view) = self.nav.clip_view.sampler.trim.as_mut() else { return };
        view.snap = !view.snap;
        let on = view.snap;
        self.flash(if on {
            "snap on \u{00b7} edges land on a zero crossing"
        } else {
            "snap off \u{00b7} edges land exactly where you put them"
        });
    }

    /// `t` inside the strip: play the region round and round, or stop.
    pub(crate) fn toggle_trim_loop(&mut self) {
        let Some(view) = self.nav.clip_view.sampler.trim.as_mut() else { return };
        view.looping = !view.looping;
        let looping = view.looping;
        if looping {
            self.preview_sampler_layer(PreviewMode::Loop);
            self.flash("loop on \u{00b7} the region plays while you trim it");
        } else {
            self.stop_sampler_preview();
            self.flash("loop off");
        }
    }

    /// `r`: play the region backwards, or forwards again.
    ///
    /// The region either way — reversing a layer never moves a marker, so a
    /// trim found forwards still means the same audio when it is flipped.
    pub(crate) fn toggle_trim_reverse(&mut self) {
        let Some((track_idx, cursor)) = self.trim_layer() else {
            self.flash("nothing here to reverse");
            return;
        };
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some(layer) = sampler.edited_mut().and_then(|p| p.layers.get_mut(cursor)) else {
            return;
        };
        layer.reverse = !layer.reverse;
        let reversed = layer.reverse;
        // Not folded: a direction is a decision, the way a mute is.
        self.nav.commit_undo(before, "reverse layer");
        self.sync_sampler_edit(track_idx);
        self.preview_sampler_layer(self.sampler_preview_mode());
        self.flash(if reversed { "reverse on" } else { "reverse off" });
    }

}
