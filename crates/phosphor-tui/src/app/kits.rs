//! App methods: the player's own drum kits.
//!
//! Made on a sampler track — its pads saved under a name from the preset
//! browser — and played on a Drum Rack, whose kit knob steps past the
//! machines into them. The files are `phosphor_app::kits`' business; this is
//! the browser's half of it, and the one pass that keeps each Drum Rack's
//! engine playing the kit its track says.
//!
//! # The reconcile pass
//!
//! A track's kit changes by more roads than the knob: undo and redo, a
//! session opening, a track coming back from the undo stack, a kit saved
//! over with a new version. Rather than teaching each of them to talk to
//! the engine, [`App::reconcile_user_kits`] runs after every input and asks
//! one question per Drum Rack — is the kit this track plays the kit its
//! engine was last given? — and ships the difference when it is not. A kit
//! carries an identity for exactly this, so the question costs a comparison
//! of two integers.

use super::*;

use phosphor_app::kits::{self, UserKit};
use phosphor_app::sampler::SamplerState;
use crate::state::undo::UndoScope;

/// How many kits the engine has been told to drop are held on to — see
/// `App::retired_kits`. A command is drained within a callback or two; this
/// is many times what a burst of keystrokes can retire in that time.
const RETIRED_KEPT: usize = 16;

impl App {
    // ── The library ──

    /// Read the kits folder again — at startup, and after a kit is saved
    /// or deleted.
    pub(crate) fn reload_kit_library(&mut self) {
        self.nav.kit_library = match self.kits_dir.as_deref() {
            Some(dir) => kits::load_all(dir),
            None => Vec::new(),
        };
    }

    /// The kits' names, in list order — what the browser's kit section shows.
    fn kit_names(&self) -> Vec<String> {
        self.nav.kit_library.iter().map(|k| k.name.clone()).collect()
    }

    /// Open the browser's drum kits section, when the track it was opened
    /// on is a sampler with pads.
    pub(crate) fn show_kit_section(&mut self) {
        let track_idx = self.nav.preset_modal.track_idx;
        let on_a_sampler = self.nav.tracks.get(track_idx).is_some_and(|t| {
            t.instrument_type == Some(InstrumentType::Sampler) && t.sampler.is_some()
        });
        if on_a_sampler {
            self.nav.preset_modal.kits = Some(self.kit_names());
        }
    }

    /// The sampler the browser was opened on.
    fn browser_sampler(&self) -> Option<&SamplerState> {
        self.nav.tracks.get(self.nav.preset_modal.track_idx)?.sampler.as_deref()
    }

    // ── Saving ──

    /// Enter on "save pads as a drum kit": ask for a name — unless there is
    /// nothing on the pads to save, which is said now rather than after the
    /// player has typed one.
    pub(crate) fn request_kit_name(&mut self) {
        if !self.browser_sampler().is_some_and(kits::has_sounds) {
            self.flash("nothing on the pads yet \u{00b7} put sounds on them, then save the kit");
            return;
        }
        if self.kits_dir.is_none() {
            self.flash("drum kits: no home directory, nowhere to keep them");
            return;
        }
        self.nav.input_modal.open_named(InputModalKind::KitName, "");
    }

    /// A name came back from the input modal. Ask first if a kit already
    /// has it.
    pub(crate) fn request_kit_save(&mut self, name: &str) {
        let name = match kits::check_name(name) {
            Ok(name) => name,
            Err(message) => {
                self.flash(message);
                return;
            }
        };
        let Some(dir) = self.kits_dir.clone() else { return };
        if kits::exists(&dir, &name) {
            let message = format!("Overwrite drum kit '{name}'?  y/n");
            self.nav.preset_modal.pending_name = name;
            self.nav.confirm_modal.show(ConfirmKind::OverwriteKit, &message);
        } else {
            self.do_save_kit(&name);
        }
    }

    /// Write the browser's sampler out as the kit called `name`.
    ///
    /// Every Drum Rack in the session playing a kit of that name moves to
    /// the new version: saving over a kit is how it is edited, and the
    /// edit is meant to be heard. Not an undo step — saving is a file
    /// operation, like a preset's.
    pub(crate) fn do_save_kit(&mut self, name: &str) {
        let Some(dir) = self.kits_dir.clone() else { return };
        let Some(state) = self.browser_sampler() else { return };
        let saved = match kits::save(&dir, name, state) {
            Ok(kit) => Arc::new(kit),
            Err(message) => {
                self.flash(format!("drum kit not saved: {message}"));
                return;
            }
        };
        self.nav.preset_modal.pending_name.clear();
        self.reload_kit_library();
        // The library's copy, so the knob finds the kit it is standing on.
        let fresh = kits::find(&self.nav.kit_library, &saved.name).cloned().unwrap_or(saved);
        let mut updated = 0usize;
        for track in &mut self.nav.tracks {
            if track.user_kit().is_some_and(|k| k.name == fresh.name) {
                track.kit = Some(Arc::clone(&fresh));
                updated += 1;
            }
        }
        self.reconcile_user_kits();

        let names = self.kit_names();
        self.nav.preset_modal.set_kits(names);
        if let Some(row) = self.nav.preset_modal.kit_row(&fresh.name) {
            self.nav.preset_modal.cursor = row;
        }
        let pads = match fresh.state.sounding_pads().len() {
            1 => "1 pad".to_string(),
            n => format!("{n} pads"),
        };
        let note = match updated {
            0 => String::new(),
            1 => " \u{00b7} the drum rack playing it has the new version".into(),
            n => format!(" \u{00b7} {n} drum racks playing it have the new version"),
        };
        crate::debug_log::system(&format!("drum kit saved: '{}' ({pads})", fresh.name));
        self.flash(format!(
            "drum kit saved: {} \u{00b7} {pads} \u{00b7} pick it on a drum rack's kit knob{note}",
            fresh.name,
        ));
    }

    // ── Editing ──

    /// Enter on a kit in the browser: put it on this sampler's pads, to
    /// edit with every sampler tool and save back under its own name.
    ///
    /// One undo step, the sampler's, and the whole bed — the pads the kit
    /// leaves empty are emptied, because what is being opened is the kit,
    /// not a few pads laid over whatever was there.
    pub(crate) fn do_load_kit_onto_sampler(&mut self, name: &str) {
        let track_idx = self.nav.preset_modal.track_idx;
        let Some(kit) = kits::find(&self.nav.kit_library, name).cloned() else {
            self.flash(format!("drum kit '{name}' is gone"));
            return;
        };
        let Some(current) = self.browser_sampler() else { return };
        let mut state = kit.state.clone();
        // The player's place on the bed is a cursor, not part of the kit.
        state.cursor = current.cursor;
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        self.apply_sampler_slice(track_idx, &Some(Box::new(state)));
        self.nav.commit_undo(before, "open drum kit");
        self.nav.preset_modal.close();
        self.flash(format!(
            "drum kit {} is on the pads \u{00b7} save it under the same name to update it \u{00b7} u undoes",
            kit.name,
        ));
    }

    // ── Deleting ──

    /// `d` on a kit in the browser: ask.
    pub(crate) fn request_kit_delete(&mut self) {
        let Some(name) = self.nav.preset_modal.selected_kit().map(str::to_string) else { return };
        let message = format!("Delete drum kit '{name}'?  y/n");
        self.nav.preset_modal.pending_name = name;
        self.nav.confirm_modal.show(ConfirmKind::DeleteKit, &message);
    }

    /// The `y` of that question. By name, for the preset's reason: the name
    /// is what the player was asked about.
    ///
    /// A Drum Rack playing the kit keeps playing it — the track holds its
    /// own copy, and a saved song keeps one in its own folder — so nothing
    /// that is already made goes quiet.
    pub(crate) fn do_delete_kit(&mut self) {
        let name = std::mem::take(&mut self.nav.preset_modal.pending_name);
        let Some(dir) = self.kits_dir.clone() else { return };
        match kits::delete(&dir, &name) {
            Ok(true) => {
                self.reload_kit_library();
                let names = self.kit_names();
                self.nav.preset_modal.set_kits(names);
                crate::debug_log::system(&format!("drum kit deleted: '{name}'"));
                self.flash(format!("drum kit deleted: {name}"));
            }
            Ok(false) => self.flash(format!("drum kit '{name}' is already gone")),
            Err(message) => self.flash(format!("drum kit not deleted: {message}")),
        }
    }

    // ── The engine ──

    /// Make every Drum Rack's engine play the kit its track says — see the
    /// module notes. Cheap enough to run after every input: one comparison
    /// per track unless something actually changed.
    pub(crate) fn reconcile_user_kits(&mut self) {
        // A track that has gone took its engine with it; what it was
        // playing is retired rather than dropped here.
        let live: Vec<usize> = self.nav.tracks.iter().filter_map(|t| t.mixer_id).collect();
        let dead: Vec<usize> =
            self.kits_in_engine.keys().copied().filter(|id| !live.contains(id)).collect();
        for id in dead {
            if let Some(kit) = self.kits_in_engine.remove(&id) {
                self.retire_kit(kit);
            }
        }

        for track_idx in 0..self.nav.tracks.len() {
            let track = &self.nav.tracks[track_idx];
            let Some(mixer_id) = track.mixer_id else { continue };
            let want = track.user_kit().cloned();
            let have = self.kits_in_engine.get(&mixer_id);
            if want.as_ref().map(|k| k.id) == have.map(|k| k.id) {
                continue;
            }
            let old = self.kits_in_engine.remove(&mixer_id);
            self.ship_kit(mixer_id, old.as_deref(), want.as_deref());
            if let Some(kit) = want {
                self.kits_in_engine.insert(mixer_id, kit);
            }
            if let Some(kit) = old {
                self.retire_kit(kit);
            }
        }
    }

    /// Move one engine from `old` to `new`, either of which may be no kit.
    ///
    /// Every pad either kit sounds is sent — the new kit's truth on it, or
    /// an empty pad where the new kit has nothing — because a pad only the
    /// old kit used is a pad the engine is still holding a sound on. The
    /// undo of a sampler's pads does the same, for the same reason. The
    /// child goes only when it changed: a new one cuts what it was playing.
    fn ship_kit(&self, mixer_id: usize, old: Option<&UserKit>, new: Option<&UserKit>) {
        let empty = SamplerState::new();
        let target = new.map_or(&empty, |k| &k.state);
        let mut pads = old.map(|k| k.state.sounding_pads()).unwrap_or_default();
        for pad in target.sounding_pads() {
            if !pads.contains(&pad) {
                pads.push(pad);
            }
        }
        pads.sort_unstable();
        let old_child = old.and_then(|k| k.state.child.as_ref());
        if old_child != target.child.as_ref() {
            self.send_sampler_child(mixer_id, target.child.as_ref());
        }
        self.send_sampler_pads(mixer_id, target, &pads);
    }

    /// Record that a rebuild has just handed a track's engine its whole
    /// kit, so the reconcile pass does not send it twice.
    pub(crate) fn note_kit_in_engine(&mut self, track_idx: usize) {
        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        let (Some(mixer_id), Some(kit)) = (track.mixer_id, track.user_kit().cloned()) else {
            return;
        };
        if let Some(old) = self.kits_in_engine.insert(mixer_id, kit) {
            self.retire_kit(old);
        }
    }

    /// A track's plugin has been replaced by a fresh one, which holds no kit
    /// at all — the next reconcile pass sends the whole of it again.
    pub(crate) fn forget_engine_kit(&mut self, mixer_id: usize) {
        if let Some(old) = self.kits_in_engine.remove(&mixer_id) {
            self.retire_kit(old);
        }
    }

    fn retire_kit(&mut self, kit: Arc<UserKit>) {
        self.retired_kits.push(kit);
        let over = self.retired_kits.len().saturating_sub(RETIRED_KEPT);
        self.retired_kits.drain(..over);
    }
}
