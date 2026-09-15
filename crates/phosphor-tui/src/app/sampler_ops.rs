//! The sampler's operations: putting sounds on pads and keeping the
//! engine's copy of a pad current.
//!
//! The state of record is [`phosphor_app::sampler::SamplerState`] on the
//! track; the engine holds a real-time copy. Every edit goes state first,
//! then [`App::sync_sampler_pad`] ships that one pad whole — config plus
//! layers — so the two can never disagree about anything but time.

use super::*;

use std::path::{Path, PathBuf};

use phosphor_app::sampler::SamplerState;

impl App {
    /// The track under the cursor, when it is a sampler carrying state.
    pub(crate) fn cursor_sampler_track(&self) -> Option<usize> {
        let track = self.nav.tracks.get(self.nav.track_cursor)?;
        (track.instrument_type == Some(InstrumentType::Sampler) && track.sampler.is_some())
            .then_some(self.nav.track_cursor)
    }

    /// `a` on the sampler's panel: ask for a file for the current pad.
    pub(crate) fn open_sample_prompt(&mut self) {
        let Some(idx) = self.cursor_sampler_track() else { return };
        let pad = self.nav.tracks[idx].sampler.as_ref().map(|s| s.cursor).unwrap_or(0);
        let label = SamplerState::pad_label(pad);
        self.nav.input_modal.open_named(InputModalKind::SamplePath, "");
        self.status_message = Some((
            format!("pad {label} \u{00b7} a bare name looks in samples/ and tries .wav"),
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
        let Some(sampler) = self.nav.tracks[idx].sampler.as_mut() else { return };
        let pad = sampler.cursor;
        // The session keeps the path as typed: a bare name stays a bare
        // name, and keeps resolving against samples/ on any machine.
        if let Err(message) = sampler.add_wav_layer(pad, PathBuf::from(typed), pcm) {
            self.flash(&message);
            return;
        }
        let count = sampler.pads[pad].layers.len();
        let name = sampler.pads[pad].layers[count - 1].name.clone();
        self.sync_sampler_pad(idx, pad);
        self.flash(&format!(
            "pad {} \u{00b7} {name} \u{00b7} {seconds:.2}s \u{00b7} layer {count}/{}",
            SamplerState::pad_label(pad),
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

    /// Replay every occupied pad to the engine — a session load, or any
    /// rebuild that made a fresh instrument.
    pub(crate) fn restore_sampler_pads(&mut self, track_idx: usize) {
        let pads: Vec<usize> = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_ref())
            .map(|s| s.occupied_pads().collect())
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
    }
}
