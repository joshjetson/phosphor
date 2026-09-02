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
        self.nav.ghost_dirty = true;
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
        self.nav.ghost_dirty = true;
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
        let table = instance.fx_type.params();
        // An instance loaded from an older session may hold fewer values
        // than the table now has; grow it to the canonical length so a new
        // knob's setting has somewhere to live in the mirror too.
        while instance.params.len() < table.len() {
            instance.params.push(table[instance.params.len()].default);
        }
        let clamped = table.get(param).map_or(value, |info| value.clamp(info.min, info.max));
        if let Some(stored) = instance.params.get_mut(param) {
            *stored = clamped;
        }
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetMidiFxParam {
            track_id,
            slot,
            param_index: param,
            value: clamped,
        });
        self.nav.ghost_dirty = true;
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
        self.nav.ghost_dirty = true;
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
            let mut progressions = Vec::new();
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
                    if current.custom_chords != wanted.custom_chords
                        || current.custom_name != wanted.custom_name
                    {
                        progressions.push((
                            slot,
                            wanted.custom_name.clone(),
                            wanted.custom_chords.clone(),
                        ));
                    }
                }
            }
            for (slot, param, value) in params {
                self.write_midi_fx_param(track_idx, slot, param, value);
            }
            for (slot, bypass) in switches {
                self.write_midi_fx_bypass(track_idx, slot, bypass);
            }
            for (slot, name, chords) in progressions {
                if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                    let track_id = track.mixer_id;
                    if let Some(instance) = track.midi_fx.get_mut(slot) {
                        instance.custom_chords = chords.clone();
                        instance.custom_name = name;
                        if let Some(track_id) = track_id {
                            let _ = self.engine.shared.mixer_command_tx.send(
                                MixerCommand::SetMidiFxProgression { track_id, slot, chords },
                            );
                        }
                    }
                }
                self.nav.ghost_dirty = true;
            }
            return;
        }

        self.clear_midi_fx(track_idx);
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            track.midi_fx = chain.to_vec();
        }
        self.install_midi_fx(track_idx);
        self.nav.ghost_dirty = true;
    }

    /// Print the rack into the viewed clip: the notes the devices would
    /// play become the clip's real, editable notes, and every device is
    /// bypassed so the sound does not transform twice. One undo step brings
    /// back both the played notes and the live rack.
    pub(crate) fn commit_midi_fx(&mut self) {
        let Some((ti, ci)) = self.nav.clip_view_target else {
            self.flash("open a clip first");
            return;
        };
        let Some(track) = self.nav.tracks.get(ti) else { return };
        if !track.midi_fx.iter().any(|s| !s.bypass) {
            self.flash("no active midi fx to commit");
            return;
        }
        let Some(clip) = track.clips.get(ci) else { return };
        if clip.notes.is_empty() && clip.controls.is_empty() {
            self.flash("the clip is empty");
            return;
        }
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::ClipsAndMidiFx { track_idx: ti },
        );
        let events = crate::state::render_clip_through_rack(
            clip,
            &track.midi_fx,
            clip.start_tick,
            self.engine.config.sample_rate as f32,
            self.engine.transport.tempo_bpm(),
        );
        let notes =
            crate::state::rendered_events_to_notes(events, clip.length_ticks.max(1));
        let count = notes.len();
        if let Some(clip) = self
            .nav
            .tracks
            .get_mut(ti)
            .and_then(|t| t.clips.get_mut(ci))
        {
            clip.notes = notes;
        }
        let slots = self.nav.tracks.get(ti).map(|t| t.midi_fx.len()).unwrap_or(0);
        for slot in 0..slots {
            self.write_midi_fx_bypass(ti, slot, true);
        }
        self.nav.commit_undo(before, "commit midi fx");
        self.send_clip_update();
        self.flash(format!(
            "committed \u{00b7} {count} notes \u{00b7} rack bypassed \u{00b7} u undoes"
        ));
    }

    /// Open the progression editor over a chord slot, seeded from what the
    /// slot already holds and the library on disk.
    pub(crate) fn open_prog_editor(&mut self, slot: usize) {
        let (chords, name) = self
            .nav
            .tracks
            .get(self.nav.track_cursor)
            .and_then(|t| t.midi_fx.get(slot))
            .map(|i| (i.custom_chords.clone(), i.custom_name.clone()))
            .unwrap_or_default();
        let library = phosphor_app::progressions::load_library();
        self.nav.prog_editor.open_for(slot, &chords, &name, library);
        self.flash("progression editor \u{00b7} enter uses it, s saves it");
    }

    /// The editor's keys. Everything stays in the working copy until Enter
    /// (load into the device) or s (save to the library).
    pub(crate) fn handle_prog_editor_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.nav.prog_editor.close(),
            KeyCode::Char('j') | KeyCode::Down => self.nav.prog_editor.move_row(1),
            KeyCode::Char('k') | KeyCode::Up => self.nav.prog_editor.move_row(-1),
            KeyCode::Tab => self.nav.prog_editor.move_col(1),
            KeyCode::Char('h') | KeyCode::Left => self.nav.prog_editor.adjust(-1),
            KeyCode::Char('l') | KeyCode::Right => self.nav.prog_editor.adjust(1),
            KeyCode::Char('a') => {
                if !self.nav.prog_editor.add_chord() {
                    self.flash("seven chords is the walk \u{2014} one per white key");
                }
            }
            KeyCode::Char('d') => {
                if !self.nav.prog_editor.remove_chord() {
                    self.flash("the last chord stays");
                }
            }
            KeyCode::Char('[') => {
                if let Some(name) = self.nav.prog_editor.cycle_library(-1) {
                    self.flash(format!("library: {name}"));
                } else {
                    self.flash("the library is empty \u{2014} s saves this one");
                }
            }
            KeyCode::Char(']') => {
                if let Some(name) = self.nav.prog_editor.cycle_library(1) {
                    self.flash(format!("library: {name}"));
                } else {
                    self.flash("the library is empty \u{2014} s saves this one");
                }
            }
            KeyCode::Char('n') => {
                let current = self.nav.prog_editor.name.clone();
                self.nav.input_modal.open_named(
                    crate::state::InputModalKind::ProgressionName,
                    &current,
                );
            }
            KeyCode::Char('s') => self.save_prog_editor_to_library(),
            KeyCode::Enter => {
                let slot = self.nav.prog_editor.slot;
                let name = self.nav.prog_editor.name.clone();
                let chords = self.nav.prog_editor.chords.clone();
                self.nav.prog_editor.close();
                self.set_user_progression(self.nav.track_cursor, slot, &name, chords);
            }
            _ => {}
        }
    }

    /// Save the working copy into the library file, replacing by name.
    fn save_prog_editor_to_library(&mut self) {
        let entry = self.nav.prog_editor.to_progression();
        let name = entry.name.clone();
        let mut library = phosphor_app::progressions::load_library();
        let replaced = phosphor_app::progressions::upsert(&mut library, entry);
        match phosphor_app::progressions::save_library(&library) {
            Ok(()) => {
                self.nav.prog_editor.library = library;
                self.flash(format!(
                    "{} \u{201c}{name}\u{201d} in the library",
                    if replaced { "updated" } else { "saved" }
                ));
            }
            Err(e) => self.flash(format!("could not save the library: {e}")),
        }
    }

    /// Keep the piano roll's ghost notes current: what the MIDI rack would
    /// make of the viewed clip, rendered offline through the same engine a
    /// commit prints with. Cheap when nothing changed; cleared when no
    /// device is active so the roll draws nothing stale.
    pub(crate) fn refresh_ghost_notes(&mut self) {
        let target = self.nav.clip_view_target;
        if !self.nav.ghost_dirty && self.nav.ghost_for == target {
            return;
        }
        self.nav.ghost_dirty = false;
        self.nav.ghost_for = target;
        self.nav.ghost_notes.clear();
        let Some((ti, ci)) = target else { return };
        let Some(track) = self.nav.tracks.get(ti) else { return };
        if !track.midi_fx.iter().any(|s| !s.bypass) {
            return;
        }
        let Some(clip) = track.clips.get(ci) else { return };
        if clip.notes.is_empty() && clip.controls.is_empty() {
            return;
        }
        let events = crate::state::render_clip_through_rack(
            clip,
            &track.midi_fx,
            clip.start_tick,
            self.engine.config.sample_rate as f32,
            self.engine.transport.tempo_bpm(),
        );
        self.nav.ghost_notes =
            crate::state::rendered_events_to_notes(events, clip.length_ticks.max(1));
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
        let chords = instance.custom_chords.clone();
        let tx = &self.engine.shared.mixer_command_tx;
        let _ = tx.send(MixerCommand::AddMidiFx { track_id, slot, fx });
        if !chords.is_empty() {
            let _ = tx.send(MixerCommand::SetMidiFxProgression { track_id, slot, chords });
        }
        if bypassed {
            let _ = tx.send(MixerCommand::SetMidiFxBypass { track_id, slot, bypassed: true });
        }
    }

    /// Load a user progression into a chord slot, on both sides, as one
    /// undo step. The mirror stores the resolved chords and the name, so
    /// the session owns its sound.
    pub(crate) fn set_user_progression(
        &mut self,
        track_index: usize,
        slot: usize,
        name: &str,
        chords: Vec<phosphor_core::midi_fx::UserChord>,
    ) {
        if chords.is_empty() {
            self.flash("the progression is empty");
            return;
        }
        let before = self.nav.undo_checkpoint(
            crate::state::undo::UndoScope::TrackMidiFx { track_idx: track_index },
        );
        let Some(track) = self.nav.tracks.get_mut(track_index) else { return };
        let Some(track_id) = track.mixer_id else { return };
        let Some(instance) = track.midi_fx.get_mut(slot) else { return };
        instance.custom_chords = chords.clone();
        instance.custom_name = name.to_string();
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetMidiFxProgression { track_id, slot, chords });
        // The knob follows: mode to prog, prog to the user slot.
        self.write_midi_fx_param(track_index, slot, 7, 1.0);
        self.write_midi_fx_param(track_index, slot, 8, 8.0);
        self.nav.ghost_dirty = true;
        self.nav.commit_undo(before, "load progression");
        self.flash(format!("progression: {name}"));
    }
}
