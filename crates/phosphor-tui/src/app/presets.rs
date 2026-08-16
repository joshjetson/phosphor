//! App methods: user presets.
//!
//! The browser is opened with Space+W on an instrument track. It reads and
//! writes `~/.phosphor/presets/<instrument>.json` from the UI thread; the
//! audio thread never sees a file. Applying a preset goes out as one
//! `SetParameter` per control down the same command channel a knob turn uses.

use super::*;
use phosphor_app::preset::{self, PresetError, StoreOutcome};

impl App {
    /// Where preset banks live. `None` when HOME is unset, in which case
    /// presets are unavailable rather than written somewhere arbitrary.
    fn preset_dir(&self) -> Option<&std::path::Path> {
        self.preset_dir.as_deref()
    }

    /// Open the preset browser for the track under the cursor.
    pub(crate) fn open_preset_browser(&mut self) {
        use crate::debug_log as dbg;

        let track_idx = self.nav.track_cursor;
        let Some(instrument) = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.instrument_type)
        else {
            self.status_message = Some((
                "presets: select an instrument track first".into(),
                std::time::Instant::now(),
            ));
            return;
        };

        let Some(dir) = self.preset_dir().map(std::path::Path::to_path_buf) else {
            self.status_message = Some((
                "presets: no HOME, nowhere to keep them".into(),
                std::time::Instant::now(),
            ));
            return;
        };

        match preset::load_bank(&dir, instrument) {
            Ok(bank) => {
                let names: Vec<String> = bank.presets.iter().map(|p| p.name.clone()).collect();
                dbg::user(&format!(
                    "preset browser: {:?}, {} saved",
                    instrument,
                    names.len()
                ));
                self.nav.preset_modal.show(instrument, track_idx, names);
            }
            Err(e) => {
                // The bank exists but does not parse. Open anyway so the
                // player can see which file is at fault, but with no entries
                // and saving disabled: writing would replace a file we could
                // not read, which is the one way to lose a whole bank.
                let path = preset::bank_path(&dir, instrument);
                tracing::warn!("preset bank unreadable: {} — {e}", path.display());
                let file = path
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.nav.preset_modal.show(instrument, track_idx, Vec::new());
                self.nav.preset_modal.error = Some(format!("{file} is unreadable"));
            }
        }
    }

    /// Enter on the save row: ask for a name.
    pub(crate) fn request_preset_name(&mut self) {
        if self.nav.preset_modal.error.is_some() {
            self.status_message = Some((
                "presets: fix the bank file before saving over it".into(),
                std::time::Instant::now(),
            ));
            return;
        }
        self.nav.input_modal.open_preset_name();
    }

    /// A name came back from the input modal. Confirm first if it is taken.
    pub(crate) fn request_preset_save(&mut self, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.status_message =
                Some(("presets: a preset needs a name".into(), std::time::Instant::now()));
            return;
        }

        let Some(instrument) = self.nav.preset_modal.instrument else { return };
        let Some(dir) = self.preset_dir().map(std::path::Path::to_path_buf) else { return };

        let taken = preset::load_bank(&dir, instrument)
            .map(|bank| bank.find(&name).is_some())
            .unwrap_or(false);

        if taken {
            // Overwriting is how a name behaves like a slot rather than a
            // label, but it destroys a sound the player kept, so it asks.
            let msg = format!("Overwrite preset '{name}'?  y/n");
            self.nav.preset_modal.pending_name = name;
            self.nav.confirm_modal.show(ConfirmKind::OverwritePreset, &msg);
        } else {
            self.do_save_preset(&name);
        }
    }

    /// Write the track's current panel into the bank under `name`.
    pub(crate) fn do_save_preset(&mut self, name: &str) {
        use crate::debug_log as dbg;

        let Some(instrument) = self.nav.preset_modal.instrument else { return };
        let Some(dir) = self.preset_dir().map(std::path::Path::to_path_buf) else { return };
        let track_idx = self.nav.preset_modal.track_idx;
        let Some(params) = self
            .nav
            .tracks
            .get(track_idx)
            .map(|t| t.synth_params.clone())
        else {
            return;
        };

        // Re-read rather than trusting the list the modal is showing: the
        // modal only kept names, and the bank is a file another instance of
        // the application could have written since it was opened.
        let mut bank = match preset::load_bank(&dir, instrument) {
            Ok(b) => b,
            Err(e) => {
                self.status_message =
                    Some((format!("preset save failed: {e}"), std::time::Instant::now()));
                return;
            }
        };

        let outcome = match bank.store(name, instrument, &params) {
            Ok(o) => o,
            Err(e) => {
                self.status_message =
                    Some((format!("preset not saved: {e}"), std::time::Instant::now()));
                return;
            }
        };

        if let Err(e) = preset::save_bank(&dir, instrument, &bank) {
            self.status_message =
                Some((format!("preset save failed: {e}"), std::time::Instant::now()));
            return;
        }

        let names: Vec<String> = bank.presets.iter().map(|p| p.name.clone()).collect();
        let row = bank.find(name).map_or(0, |i| i + 1);
        self.nav.preset_modal.set_entries(names);
        self.nav.preset_modal.cursor = row;
        self.nav.preset_modal.pending_name.clear();

        let verb = match outcome {
            StoreOutcome::Added => "saved",
            StoreOutcome::Replaced => "overwrote",
        };
        dbg::system(&format!("preset {verb}: '{name}' ({} controls)", params.len()));
        self.status_message = Some((
            format!("preset {verb}: {}", name.trim()),
            std::time::Instant::now(),
        ));
    }

    /// Apply the preset at `index` to the track the browser was opened on.
    pub(crate) fn do_load_preset(&mut self, index: usize) {
        use crate::debug_log as dbg;

        let Some(instrument) = self.nav.preset_modal.instrument else { return };
        let Some(dir) = self.preset_dir().map(std::path::Path::to_path_buf) else { return };
        let track_idx = self.nav.preset_modal.track_idx;
        let Some(want_count) = self.nav.tracks.get(track_idx).map(|t| t.synth_params.len()) else {
            return;
        };

        let bank = match preset::load_bank(&dir, instrument) {
            Ok(b) => b,
            Err(e) => {
                self.status_message =
                    Some((format!("preset load failed: {e}"), std::time::Instant::now()));
                return;
            }
        };

        let Some(result) = bank.params_at(index, instrument, want_count) else { return };
        let loaded = match result {
            Ok(p) => p,
            Err(e) => {
                // Same shape as a session whose parameter block does not fit
                // the instrument: say so and change nothing. Loading a block
                // of the wrong length or the wrong order produces a plausible
                // sound that is not the one that was saved, with nothing on
                // screen to say it happened.
                let name = bank.presets[index].name.clone();
                tracing::warn!("preset '{name}': {e} — not loaded");
                let detail = match &e {
                    PresetError::LayoutMismatch { .. } => {
                        "saved against an older panel layout".to_string()
                    }
                    other => other.to_string(),
                };
                // "not loaded" leads, because the bottom bar is narrow and a
                // message whose first half is a preset name reads, when it is
                // cut off, as though the preset loaded.
                self.status_message = Some((
                    format!("not loaded: '{name}' \u{2014} {detail}"),
                    std::time::Instant::now(),
                ));
                return;
            }
        };

        let name = bank.presets[index].name.clone();
        if let Some(track) = self.nav.tracks.get_mut(track_idx) {
            track.synth_params.copy_from_slice(&loaded.params);
        }
        self.push_params_to_audio(track_idx);

        // Both notes are about the same thing — a patch that may not be the one
        // the preset named — and the bottom bar is the only place the player
        // would ever find that out. Same wording as a session load, because it
        // is the same defect: a selector stored as a fraction of a bank that
        // has since changed size.
        for (param, wanted, given) in &loaded.clamped {
            tracing::warn!(
                "preset '{name}': control {param} was at position {wanted}, which this \
                 build no longer has — loading {given}"
            );
        }
        let note = if loaded.legacy_selectors {
            " (older format: check the patch)"
        } else if !loaded.clamped.is_empty() {
            " (a patch it names is no longer in the bank)"
        } else {
            ""
        };

        dbg::system(&format!(
            "preset loaded: '{name}' ({} controls){note}",
            loaded.params.len()
        ));
        self.status_message =
            Some((format!("preset loaded: {name}{note}"), std::time::Instant::now()));
    }

    /// `d` on a preset row: confirm, then delete.
    pub(crate) fn request_preset_delete(&mut self) {
        let Some(name) = self.nav.preset_modal.selected_name().map(str::to_string) else { return };
        let msg = format!("Delete preset '{name}'?  y/n");
        self.nav.preset_modal.pending_name = name;
        self.nav.confirm_modal.show(ConfirmKind::DeletePreset, &msg);
    }

    /// Remove the preset the confirmation was raised for.
    ///
    /// By name rather than by row: the row is a position in a list read when
    /// the modal opened, and the bank is a file. The name is what the player
    /// was asked about.
    pub(crate) fn do_delete_preset(&mut self) {
        use crate::debug_log as dbg;

        let name = std::mem::take(&mut self.nav.preset_modal.pending_name);
        let Some(instrument) = self.nav.preset_modal.instrument else { return };
        let Some(dir) = self.preset_dir().map(std::path::Path::to_path_buf) else { return };

        let mut bank = match preset::load_bank(&dir, instrument) {
            Ok(b) => b,
            Err(e) => {
                self.status_message =
                    Some((format!("preset delete failed: {e}"), std::time::Instant::now()));
                return;
            }
        };

        let Some(index) = bank.find(&name) else {
            self.status_message = Some((
                format!("preset '{name}' is already gone"),
                std::time::Instant::now(),
            ));
            return;
        };
        bank.remove(index);

        if let Err(e) = preset::save_bank(&dir, instrument, &bank) {
            self.status_message =
                Some((format!("preset delete failed: {e}"), std::time::Instant::now()));
            return;
        }

        let names: Vec<String> = bank.presets.iter().map(|p| p.name.clone()).collect();
        self.nav.preset_modal.set_entries(names);
        dbg::system(&format!("preset deleted: '{name}'"));
        self.status_message = Some((format!("preset deleted: {name}"), std::time::Instant::now()));
    }

    /// Push a track's whole parameter block to the audio thread.
    ///
    /// One `SetParameter` per control down the mixer command channel — the
    /// same path a knob turn and a session load take. Nothing here allocates
    /// or blocks on the audio side: the mixer drains the queue at the top of
    /// its callback and calls `set_parameter` on the plugin.
    pub(crate) fn push_params_to_audio(&self, track_idx: usize) {
        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        let Some(mixer_id) = track.mixer_id else { return };
        for (i, &value) in track.synth_params.iter().enumerate() {
            let _ = self
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::SetParameter {
                    track_id: mixer_id,
                    param_index: i,
                    value,
                });
        }
    }
}
