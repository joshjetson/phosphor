//! The sampler's operations: putting sounds on pads, turning what is on
//! them, and keeping the engine's copy current.
//!
//! The state of record is [`phosphor_app::sampler::SamplerState`] on the
//! track; the engine holds a real-time copy. Every edit goes state first,
//! then [`App::sync_sampler_pad`] ships that one pad whole — config plus
//! layers — so the two can never disagree about anything but time. Nothing
//! outside this file writes a pad, for the reason the sequencer's ops
//! exist: an edit that changes the screen and not the signal is the hardest
//! kind of bug to see, because the screen agrees with you.
//!
//! There is a second door in this file and it makes a sound rather than
//! changing one: [`App::preview_sampler_layer`] auditions a single layer,
//! which is what the trim strip does on every nudge and what the layer list
//! does when its cursor moves. It is here because an audition that can be
//! started from three files is an audition nobody can switch off — see
//! [`App::reconcile_sampler_preview`] for the switch.
//!
//! Undo goes with it, and here it carries a second job. A deleted layer's
//! audio must stay referenced by something on the UI side for as long as
//! the audio thread might still hold it, or the engine's own drop becomes
//! the last one and a free happens on the real-time thread. The undo step
//! *is* that reference — which is why no path removes a layer without
//! pushing one.

use super::*;

use std::path::{Path, PathBuf};

use phosphor_app::sampler::knobs::PadKnob;
use phosphor_app::sampler::{LayerState, MapMode, SamplerState, ZoneEdge};
use phosphor_plugin::sample::{PreviewLayer, PreviewMode};
use crate::state::undo::{UndoGesture, UndoScope};

impl App {
    /// The track under the cursor, when it is a sampler carrying state.
    pub(crate) fn cursor_sampler_track(&self) -> Option<usize> {
        let track = self.nav.tracks.get(self.nav.track_cursor)?;
        (track.instrument_type == Some(InstrumentType::Sampler) && track.sampler.is_some())
            .then_some(self.nav.track_cursor)
    }

    /// The sampler under the cursor, when the track has one.
    pub(crate) fn cursor_sampler(&self) -> Option<&SamplerState> {
        self.cursor_sampler_track().and_then(|idx| self.nav.tracks[idx].sampler.as_deref())
    }

    /// Whether the pad map is the thing the keys are typing into.
    ///
    /// Asked by everything that has to stop when the player walks away: the
    /// audition, and root-learn. One answer, because two of them would
    /// drift and the one that drifted would be the one nobody notices.
    pub(crate) fn sampler_keys_have_focus(&self) -> bool {
        self.nav.focused_pane == Pane::ClipView
            && self.nav.clip_view.clip_tab == ClipTab::Pads
            && self.nav.clip_view.focus == ClipViewFocus::PianoRoll
    }

    /// `a` on the sampler's panel: ask for a file for the pad or zone
    /// under the cursor.
    pub(crate) fn open_sample_prompt(&mut self) {
        let Some(sampler) = self.cursor_sampler() else { return };
        if let Err(message) = sampler.room_here() {
            self.flash(message);
            return;
        }
        let title = sampler.edit_title();
        self.nav.input_modal.open_named(InputModalKind::SamplePath, "");
        self.status_message = Some((
            format!("{title} \u{00b7} a bare name looks in samples/ and tries .wav"),
            std::time::Instant::now(),
        ));
    }

    /// Enter in the sample prompt: decode the file and stack it on the
    /// current pad. Every failure is a sentence in the status bar; the
    /// pad is untouched unless the whole path worked.
    pub(crate) fn do_load_sample(&mut self, typed: &str) {
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let Some(idx) = self.cursor_sampler_track() else {
            self.flash("no sampler under the cursor");
            return;
        };
        let resolved = phosphor_app::paths::find_sample(Path::new(typed));
        let pcm = match phosphor_app::sampler::wav::load_wav(&resolved) {
            Ok(pcm) => pcm,
            Err(message) => {
                self.flash(&message);
                return;
            }
        };
        let seconds = pcm.frames() as f32 / pcm.sample_rate.max(1.0);
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx: idx });
        let Some(sampler) = self.nav.tracks[idx].sampler.as_mut() else { return };
        // The session keeps the path as typed: a bare name stays a bare
        // name, and keeps resolving against samples/ on any machine.
        let landed = match sampler.add_wav_here(PathBuf::from(typed), pcm) {
            Ok(index) => index,
            Err(message) => {
                self.flash(&message);
                return;
            }
        };
        let title = sampler.edit_title();
        let (name, count) = match sampler.edited() {
            Some(state) => (state.layers[landed].name.clone(), state.layers.len()),
            None => return,
        };
        // A name that says its own pitch teaches the root — but only where
        // a root is audible: a sound that does not keytrack plays the same
        // on every key, and retuning it from its file name would be a
        // change the player did not ask for and cannot hear.
        let sniffed = sampler
            .edit_keytracks()
            .then(|| phosphor_app::sampler::root::from_name(&name))
            .flatten();
        let learned = match sniffed {
            Some(root) if sampler.set_edit_root(root) => {
                format!(" \u{00b7} root {}", phosphor_app::format::note_name(root))
            }
            _ => String::new(),
        };
        // A sound landing on a pad is one step back off it, and the step
        // holds the decode: `u` after a load is free, and a `u` that
        // emptied the pad still has the audio if redo asks for it.
        self.nav.commit_undo(before, "load sample");
        // The panel points at the sound that just arrived, which is the one
        // a player is about to trim, turn down or take off again.
        self.nav.clip_view.sampler.layer = landed;
        self.sync_sampler_edit(idx);
        self.flash(format!(
            "{title} \u{00b7} {name} \u{00b7} {seconds:.2}s \u{00b7} layer {count}/{}{learned}",
            phosphor_app::sampler::MAX_LAYERS,
        ));
    }

    /// Ship one pad's current truth to the engine, whole.
    pub(crate) fn sync_sampler_pad(&mut self, track_idx: usize, pad: usize) {
        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        let (Some(mixer_id), Some(sampler)) = (track.mixer_id, track.sampler.as_ref()) else {
            return;
        };
        let Some((config, layers)) = sampler.engine_pad(pad) else { return };
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetSamplerPad {
            track_id: mixer_id,
            pad: pad as u8,
            config,
            layers,
        });
    }

    /// Ship every key an edit under the cursor can be heard on: the one
    /// pad in pads mode, the whole zone in keys mode.
    ///
    /// The door every edit leaves by, so that no caller has to remember
    /// which mode it is in — the same reason [`App::sync_sampler_pad`] is
    /// the only thing that writes to the engine at all.
    pub(crate) fn sync_sampler_edit(&mut self, track_idx: usize) {
        let span = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_ref())
            .and_then(|s| s.edit_span());
        if let Some(span) = span {
            self.sync_sampler_span(track_idx, span);
        }
    }

    /// Ship a stretch of keys, both ends included.
    pub(crate) fn sync_sampler_span(&mut self, track_idx: usize, (lo, hi): (usize, usize)) {
        for pad in lo..=hi.min(phosphor_app::sampler::NUM_PADS - 1) {
            self.sync_sampler_pad(track_idx, pad);
        }
    }

    /// Replay everything the engine could be sounding to a fresh instance —
    /// a session load, or any rebuild that made a new one.
    ///
    /// The union of both modes, not just the one that is on: a player who
    /// saved in pads mode and switched to keys before the rebuild would
    /// otherwise get a kit that is silent in the mode they are not in.
    pub(crate) fn restore_sampler_pads(&mut self, track_idx: usize) {
        let pads: Vec<usize> = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_ref())
            .map(|s| s.sounding_pads())
            .unwrap_or_default();
        for pad in pads {
            self.sync_sampler_pad(track_idx, pad);
        }
    }

    /// A note-on seen by the UI's MIDI tap: the pad cursor follows the
    /// keys. On an 88-key controller this is the fastest pad selector
    /// there is, and it costs one array index.
    pub(crate) fn sampler_follow_note(&mut self, note: u8) {
        let Some(idx) = self.cursor_sampler_track() else { return };
        if let Some(pad) = SamplerState::pad_of_note(note) {
            if let Some(sampler) = self.nav.tracks[idx].sampler.as_mut() {
                sampler.cursor = pad;
            }
        }
        // The pad that just arrived under the cursor may hold fewer layers
        // than the one the panel was showing — and if it has none at all the
        // trim strip closes, which is one of the ways a loop preview can be
        // left playing with nothing on the screen to explain it.
        self.clamp_sampler_cursors();
        self.reconcile_sampler_preview();
    }

    // ── The pad panel ──

    /// The controls the thing under the cursor offers, and where the cursor
    /// is standing in them.
    pub(crate) fn sampler_knobs(&self) -> &'static [PadKnob] {
        let Some(sampler) = self.cursor_sampler() else {
            return PadKnob::visible(false, MapMode::Pads);
        };
        let has_layer = sampler.edited().is_some_and(|p| !p.layers.is_empty());
        PadKnob::visible(has_layer, sampler.mode)
    }

    /// How many layers the thing under the cursor holds.
    pub(crate) fn sampler_layer_count(&self) -> usize {
        self.cursor_sampler()
            .and_then(SamplerState::edited)
            .map_or(0, |state| state.layers.len())
    }

    /// Pull both panel cursors inside what the current pad holds.
    ///
    /// Called before every edit and after anything that can change which
    /// pad is current, because the pad cursor moves on its own: playing a
    /// key takes it to a pad that may have no layers at all, and six
    /// controls go with them.
    pub(crate) fn clamp_sampler_cursors(&mut self) {
        let (knobs, layers) = (self.sampler_knobs().len(), self.sampler_layer_count());
        self.nav.clip_view.sampler.clamp(knobs, layers);
    }

    /// Walk the cursor along the bed.
    pub(crate) fn move_sampler_pad(&mut self, delta: i32) {
        let Some(idx) = self.cursor_sampler_track() else { return };
        let Some(sampler) = self.nav.tracks[idx].sampler.as_mut() else { return };
        sampler.move_cursor(delta);
        self.clamp_sampler_cursors();
        // Not an undo step and not a command: where the cursor is standing
        // is a cursor, and the engine has never needed to know.
        let Some(sampler) = self.cursor_sampler() else { return };
        let (title, key) = (sampler.edit_title(), SamplerState::pad_label(sampler.cursor));
        let layers = self.sampler_layer_count();
        self.flash(match self.sampler_mode() {
            // In keys mode the key and the zone are different things and
            // the player needs both: which key the cursor is on, and which
            // zone that puts the panel in.
            MapMode::Keys if self.cursor_sampler_zone().is_none() => {
                format!("{key} \u{00b7} {}", SamplerState::no_zone_message())
            }
            MapMode::Keys => format!("{key} \u{00b7} {title} \u{00b7} {layers} layers"),
            MapMode::Pads => format!("{title} \u{00b7} {layers} layers"),
        });
    }

    /// Walk the layer cursor inside the current pad.
    ///
    /// And sound what it lands on. A stack of eight layers is eight names in
    /// a list until you can hear which is which, and the audition costs one
    /// command — the deferral M3 made rather than bolt a one-off note-on
    /// onto the mixer.
    pub(crate) fn move_sampler_layer(&mut self, delta: i32) {
        let count = self.sampler_layer_count();
        let before = self.nav.clip_view.sampler.layer;
        self.nav.clip_view.sampler.move_layer(delta, count);
        if self.nav.clip_view.sampler.layer != before {
            self.preview_sampler_layer(PreviewMode::Once);
        }
    }

    /// Put the layer cursor on a numbered layer, when the pad has one.
    ///
    /// This one sounds the layer even when the cursor was already on it,
    /// where `[`/`]` do not: naming a number is a player asking for that
    /// sound, and walking into the end of the list is not asking for
    /// anything.
    pub(crate) fn select_sampler_layer(&mut self, index: usize) {
        if index < self.sampler_layer_count() {
            self.nav.clip_view.sampler.layer = index;
            self.preview_sampler_layer(PreviewMode::Once);
        }
    }

    // ── The audition ──

    /// Sound the layer under the layer cursor, on its own.
    ///
    /// The engine's second door, beside [`App::sync_sampler_pad`]: this one
    /// makes a sound rather than changing one. Both are here because
    /// everything that touches a pad's audio should be findable in one file
    /// — an audition that fires from three places is an audition nobody can
    /// switch off.
    ///
    /// The layer travels whole rather than as an index, because the engine's
    /// layer slots hold only the ones with audio behind them: with a missing
    /// file in the stack, row three and slot three are different sounds.
    pub(crate) fn preview_sampler_layer(&mut self, mode: PreviewMode) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let cursor = self.nav.clip_view.sampler.layer;
        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        let (Some(mixer_id), Some(sampler)) = (track.mixer_id, track.sampler.as_ref()) else {
            return;
        };
        let Some(pad) = sampler.edited() else {
            self.stop_sampler_preview();
            return;
        };
        // A missing file makes no sound, and neither does a muted layer: an
        // audition that ignored the mute would be the one place in the box
        // where a muted sound plays.
        let Some(layer) = pad
            .layers
            .get(cursor)
            .filter(|l| !l.mute)
            .and_then(LayerState::engine_layer)
        else {
            self.stop_sampler_preview();
            return;
        };
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetSamplerPreview {
            track_id: mixer_id,
            preview: Some(PreviewLayer { config: pad.config, layer, mode }),
        });
        self.sampler_preview = Some((mixer_id, mode));
    }

    /// Silence the audition, wherever it is running.
    pub(crate) fn stop_sampler_preview(&mut self) {
        let Some((track_id, _)) = self.sampler_preview.take() else { return };
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetSamplerPreview { track_id, preview: None });
    }

    /// How an audition started now should behave: round and round while the
    /// trim strip is looping, a single pass otherwise. One answer, because
    /// every nudge and every toggle in the strip has to give the same one.
    pub(crate) fn sampler_preview_mode(&self) -> PreviewMode {
        match self.nav.clip_view.sampler.trim {
            Some(view) if view.looping => PreviewMode::Loop,
            _ => PreviewMode::Once,
        }
    }

    /// Stop an audition the player has walked away from.
    ///
    /// A preview is a mode the engine holds until it is told otherwise, and
    /// there are more ways out of the pad map than there are keys in it —
    /// Tab to another view, `esc` to the track list, the track deleted from
    /// under the cursor, a key played that moves the pad cursor onto an
    /// empty pad. Rather than remembering to stop it at each of them, the
    /// question is asked once per keystroke and once per note: are the keys
    /// still on the pads of the track that is sounding, and is the strip
    /// still open if what is sounding is a loop? If not, silence.
    ///
    /// The second half of that is the part worth stating. A single pass ends
    /// by itself, so leaving one running costs nothing; a loop ends only
    /// when something asks it to, and the strip is the only thing with a key
    /// for asking.
    pub(crate) fn reconcile_sampler_preview(&mut self) {
        let Some((track_id, mode)) = self.sampler_preview else { return };
        let on_the_pads = self.sampler_keys_have_focus()
            && self
                .cursor_sampler_track()
                .and_then(|idx| self.nav.tracks.get(idx))
                .and_then(|t| t.mixer_id)
                == Some(track_id);
        let kept = on_the_pads
            && (mode == PreviewMode::Once || self.nav.clip_view.sampler.trim.is_some());
        if !kept {
            self.stop_sampler_preview();
        }
    }

    /// Turn the control under the cursor.
    ///
    /// One undo step per gesture: a player who nudges the decay and then
    /// the level has made one adjustment to one pad, and one `u` puts both
    /// back — the effect panel's grain, applied to a pad.
    pub(crate) fn adjust_sampler_knob(&mut self, delta: i32, stride: bool) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        self.clamp_sampler_cursors();
        let view = &self.nav.clip_view.sampler;
        let (cursor, layer) = (view.knob, view.layer);
        let Some(&knob) = self.sampler_knobs().get(cursor) else { return };
        // The span is the brace, not a control on the sound: its edges are
        // clamped against the zones either side, and `H`/`L` are the other
        // edge rather than a stride. Everything else about it — the hold,
        // the readout, the one undo step per run — is this door's.
        if knob == PadKnob::Span {
            self.nudge_zone_edge(if stride { ZoneEdge::High } else { ZoneEdge::Low }, delta);
            return;
        }

        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some((lo, hi)) = sampler.edit_span() else {
            self.flash(SamplerState::no_zone_message());
            return;
        };
        let title = sampler.edit_title();
        let Some(state) = sampler.edited_mut() else { return };
        knob.adjust(state, layer, delta, stride);
        let shown = knob.value(state, state.layers.get(layer), None);
        // The gesture is named for the first key of what is being edited,
        // so a sweep on one zone never folds into a sweep on the next.
        self.nav.commit_undo_coalesced(
            before,
            "adjust pad",
            UndoGesture::SamplerPad { track_idx, pad: lo },
        );
        self.sync_sampler_span(track_idx, (lo, hi));
        self.flash(format!("{title} {}: {shown}", knob.label()));
    }

    // ── The layer list ──

    /// `m`: take the layer under the cursor out of the pad's sound without
    /// taking it off the pad. One step, never folded — a mute is a decision,
    /// not a sweep.
    pub(crate) fn toggle_sampler_layer_mute(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        self.clamp_sampler_cursors();
        let layer = self.nav.clip_view.sampler.layer;
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some(state) = sampler.edited_mut().and_then(|p| p.layers.get_mut(layer)) else {
            return;
        };
        state.mute = !state.mute;
        let (muted, name) = (state.mute, state.name.clone());
        self.nav.commit_undo(before, "mute layer");
        self.sync_sampler_edit(track_idx);
        // An audition holds its own copy of the layer and would otherwise
        // play on regardless — which would make this the one place in the
        // box where a muted sound is audible.
        self.stop_sampler_preview();
        self.flash(format!(
            "{name}: {}",
            if muted { "muted" } else { "in the pad" },
        ));
    }

    /// `d`: ask before taking a sound off a pad.
    ///
    /// The effect chain's modal, for the effect chain's reason and one of
    /// its own: a layer can be a take that took a performance to make, and
    /// `d` is one key away from the ones that walk the list.
    pub(crate) fn request_sampler_layer_delete(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        self.clamp_sampler_cursors();
        let layer = self.nav.clip_view.sampler.layer;
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_ref() else { return };
        let Some(state) = sampler.edited().and_then(|p| p.layers.get(layer)) else {
            // `d` removes a *sound*, in both modes. A player pressing it on
            // a zone that has none probably wants the zone gone, and the
            // key for that is one shift away — so say so rather than just
            // refusing.
            self.flash(match sampler.mode {
                MapMode::Keys => format!(
                    "nothing on {} to remove \u{00b7} D takes the zone off the bed",
                    sampler.edit_title(),
                ),
                MapMode::Pads => format!("nothing on {} to remove", sampler.edit_title()),
            });
            return;
        };
        let message = format!("remove {} from {}?", state.name, sampler.edit_title());
        self.nav.confirm_modal.show(ConfirmKind::DeleteSamplerLayer, &message);
    }

    /// The `y` of that modal.
    ///
    /// The undo step is not only for the player: the slice it captured
    /// holds the last UI-side `Arc` to this layer's audio, so the buffer
    /// stays alive until history lets go of it. The engine hears about the
    /// pad on the next line and drops its own copy whenever it is finished
    /// with it, which is then never the final drop.
    pub(crate) fn delete_sampler_layer(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let layer = self.nav.clip_view.sampler.layer;
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some(state) = sampler.edited_mut() else { return };
        if layer >= state.layers.len() {
            return;
        }
        let name = state.layers.remove(layer).name;
        self.nav.commit_undo(before, "remove layer");
        self.sync_sampler_edit(track_idx);
        self.clamp_sampler_cursors();
        // A sound taken off a pad stops. The audition holds its own copy and
        // would otherwise play the removed layer to its end — and starting
        // the neighbour the cursor fell onto would be a sound nobody asked
        // for at the one moment a player is removing one.
        self.stop_sampler_preview();
        self.flash(format!("{name} removed \u{00b7} u brings it back"));
    }

    // ── Undo ──

    /// Put a captured sampler back, and tell the engine everything it has
    /// to forget as well as everything it has to learn.
    ///
    /// The union of the two sounding sets, not just the new one: a key the
    /// undo emptied is a key the engine is still holding a sound for, and
    /// shipping only what sounds now would leave that sound on the key with
    /// nothing on the screen to explain it. Both modes are in each set, so
    /// an undo that crosses a mode switch is covered by the same rule.
    pub(crate) fn apply_sampler_slice(
        &mut self,
        track_idx: usize,
        sampler: &Option<Box<SamplerState>>,
    ) {
        let mut pads: Vec<usize> = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_ref())
            .map(|s| s.sounding_pads())
            .unwrap_or_default();
        for pad in sampler.as_ref().map(|s| s.sounding_pads()).unwrap_or_default() {
            if !pads.contains(&pad) {
                pads.push(pad);
            }
        }
        let Some(track) = self.nav.tracks.get_mut(track_idx) else { return };
        track.sampler = sampler.clone();
        for pad in pads {
            self.sync_sampler_pad(track_idx, pad);
        }
        self.clamp_sampler_cursors();
    }
}
