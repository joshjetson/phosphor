//! App methods: the MIDI-effect slots — install, remove, switch, adjust.
//!
//! The same discipline as the audio inserts: the front-end mirror is
//! written first, the command is sent from what the mirror then holds, and
//! every user-facing door takes an undo step while the `write_` half is the
//! one undo itself applies a captured chain through.

use super::*;
use crate::state::{MidiFxInstance, MidiFxType};
use phosphor_core::midi_fx::MAX_MIDI_FX_SLOTS;

impl App {
    /// Put a fresh effect of `fx_type` at the end of the track's MIDI rack.
    pub(crate) fn add_midi_fx(&mut self, track_index: usize, fx_type: MidiFxType) {
        let Some(track) = self.nav.tracks.get(track_index) else { return };
        if track.midi_fx.len() >= MAX_MIDI_FX_SLOTS {
            self.flash("the midi rack is full");
            return;
        }
        if track.midi_fx.iter().any(|s| s.fx_type == fx_type) {
            self.flash(format!("{} is already in the rack", fx_type.label()));
            return;
        }
        if track.mixer_id.is_none() {
            self.flash("midi fx live on instrument tracks");
            return;
        }
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMidiFx { track_idx: track_index },
        );
        // Canonical order: the chord device leads and the arp follows —
        // the arp needs a chord to arpeggiate. Adding in either order
        // lands them the right way round.
        let slot = {
            let track = &mut self.nav.tracks[track_index];
            let at = match fx_type {
                MidiFxType::Chord => 0,
                MidiFxType::Arp => track.midi_fx.len(),
            };
            track.midi_fx.insert(at, MidiFxInstance::new(fx_type));
            at
        };
        self.send_add_midi_fx(track_index, slot);
        self.nav.commit_undo(before, "add midi effect");
        self.flash(format!("{} added \u{00b7} plays live and on playback", fx_type.label()));
    }

    /// Take a slot out, on both sides. The audio thread flushes its
    /// note-offs before dropping it.
    pub(crate) fn remove_midi_fx(&mut self, track_index: usize, slot: usize) {
        let Some(track) = self.nav.tracks.get(track_index) else { return };
        if slot >= track.midi_fx.len() {
            return;
        }
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMidiFx { track_idx: track_index },
        );
        let track_id = track.mixer_id;
        self.nav.tracks[track_index].midi_fx.remove(slot);
        if let Some(track_id) = track_id {
            let _ = self
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::RemoveMidiFx { track_id, slot });
        }
        self.nav.commit_undo(before, "remove midi effect");
    }

    /// One knob movement, one coalesced undo step — the fx-slot contract.
    pub(crate) fn set_midi_fx_param(
        &mut self,
        track_index: usize,
        slot: usize,
        param: usize,
        value: f32,
    ) {
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMidiFx { track_idx: track_index },
        );
        self.write_midi_fx_param(track_index, slot, param, value);
        self.nav.commit_undo_coalesced(
            before,
            "adjust midi effect",
            crate::state::undo::UndoGesture::MidiFxSlot { track_idx: track_index, slot },
        );
    }

    /// [`Self::set_midi_fx_param`] without the undo step.
    pub(crate) fn write_midi_fx_param(
        &mut self,
        track_index: usize,
        slot: usize,
        param: usize,
        value: f32,
    ) {
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(track_id) = track.mixer_id else { return };
        let Some(instance) = track.midi_fx.get_mut(slot) else { return };
        let clamped = instance
            .fx_type
            .params()
            .get(param)
            .map_or(value, |info| value.clamp(info.min, info.max));
        if let Some(stored) = instance.params.get_mut(param) {
            *stored = clamped;
        }
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetMidiFxParam {
            track_id,
            slot,
            param_index: param,
            value: clamped,
        });
    }

    /// Throw a slot's switch. Discrete, never coalesced.
    pub(crate) fn set_midi_fx_bypass(&mut self, track_index: usize, slot: usize, bypass: bool) {
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMidiFx { track_idx: track_index },
        );
        self.write_midi_fx_bypass(track_index, slot, bypass);
        self.nav.commit_undo(before, "bypass midi effect");
    }

    /// [`Self::set_midi_fx_bypass`] without the undo step.
    pub(crate) fn write_midi_fx_bypass(&mut self, track_index: usize, slot: usize, bypass: bool) {
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(track_id) = track.mixer_id else { return };
        let Some(instance) = track.midi_fx.get_mut(slot) else { return };
        instance.bypass = bypass;
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetMidiFxBypass { track_id, slot, bypassed: bypass });
    }

    /// Install the mirror's whole rack onto the audio thread — session load
    /// and undo's reinstall path.
    pub(crate) fn install_midi_fx(&mut self, track_index: usize) {
        let count = self.nav.tracks.get(track_index).map(|t| t.midi_fx.len()).unwrap_or(0);
        for slot in 0..count {
            self.send_add_midi_fx(track_index, slot);
        }
    }

    /// Empty the rack on both sides.
    pub(crate) fn clear_midi_fx(&mut self, track_index: usize) {
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let count = track.midi_fx.len();
        track.midi_fx.clear();
        let Some(track_id) = track.mixer_id else { return };
        for slot in (0..count).rev() {
            let _ = self
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::RemoveMidiFx { track_id, slot });
        }
    }

    /// Undo's door: make the track's rack be exactly `chain`.
    pub(crate) fn apply_midi_fx_slice(
        &mut self,
        track_idx: usize,
        chain: &[MidiFxInstance],
    ) {
        let same_shape = self.nav.tracks.get(track_idx).is_some_and(|t| {
            t.midi_fx.len() == chain.len()
                && t.midi_fx.iter().zip(chain).all(|(a, b)| a.fx_type == b.fx_type)
        });
        if same_shape {
            let mut params = Vec::new();
            let mut switches = Vec::new();
            if let Some(track) = self.nav.tracks.get(track_idx) {
                for (slot, wanted) in chain.iter().enumerate() {
                    let current = &track.midi_fx[slot];
                    for (param, &value) in wanted.params.iter().enumerate() {
                        if current.params.get(param).copied() != Some(value) {
                            params.push((slot, param, value));
                        }
                    }
                    if current.bypass != wanted.bypass {
                        switches.push((slot, wanted.bypass));
                    }
                }
            }
            for (slot, param, value) in params {
                self.write_midi_fx_param(track_idx, slot, param, value);
            }
            for (slot, bypass) in switches {
                self.write_midi_fx_bypass(track_idx, slot, bypass);
            }
            return;
        }

        self.clear_midi_fx(track_idx);
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            track.midi_fx = chain.to_vec();
        }
        self.install_midi_fx(track_idx);
    }

    /// Build the effect from a mirror slot and send it to the audio thread.
    fn send_add_midi_fx(&mut self, track_index: usize, slot: usize) {
        let Some(track) = self.nav.tracks.get(track_index) else { return };
        let Some(track_id) = track.mixer_id else { return };
        let Some(instance) = track.midi_fx.get(slot) else { return };
        let Some(mut fx) = phosphor_core::midi_fx::build_midi_fx(instance.fx_type.key()) else {
            return;
        };
        for (index, &value) in instance.params.iter().enumerate() {
            fx.set_parameter(index, value);
        }
        let bypassed = instance.bypass;
        let tx = &self.engine.shared.mixer_command_tx;
        let _ = tx.send(MixerCommand::AddMidiFx { track_id, slot, fx });
        if bypassed {
            let _ = tx.send(MixerCommand::SetMidiFxBypass { track_id, slot, bypassed: true });
        }
    }
}
