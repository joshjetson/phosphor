//! `y`, `Y` and `p` on the pad map: copying a pad, or one sound off it.
//!
//! The rules are [`phosphor_app::sampler::clipboard`]'s; this is the part
//! that belongs to the application — the one clipboard, the undo step, the
//! engine told, and the words. The clipboard outlives the track it was
//! filled on, so a pad copied on one sampler pastes onto another.

use super::*;

use phosphor_app::sampler::ChildChange;

use crate::state::undo::UndoScope;

impl App {
    /// `y`: copy the pad under the caret, every sound and setting on it.
    pub(crate) fn yank_sampler_pad(&mut self) {
        let Some(sampler) = self.cursor_sampler() else { return };
        match sampler.yank_pad() {
            Ok(clip) => {
                let words = format!("{} copied \u{00b7} p pastes it on any key", clip.label());
                self.sampler_clip = Some(clip);
                self.flash(words);
            }
            Err(message) => self.flash(message),
        }
    }

    /// `Y`: copy just the sound under the layer cursor.
    pub(crate) fn yank_sampler_sound(&mut self) {
        let row = self.nav.clip_view.sampler.layer;
        let Some(sampler) = self.cursor_sampler() else { return };
        match sampler.yank_sound(row) {
            Ok(clip) => {
                let words = format!("{} copied \u{00b7} p stacks it on any key", clip.label());
                self.sampler_clip = Some(clip);
                self.flash(words);
            }
            Err(message) => self.flash(message),
        }
    }

    /// `p`: paste onto the pad under the caret, as one undo step.
    pub(crate) fn paste_sampler(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let Some(clip) = self.sampler_clip.clone() else {
            self.flash("nothing copied \u{00b7} y copies a pad, Y one sound");
            return;
        };
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let pasted = match sampler.paste(&clip) {
            Ok(pasted) => pasted,
            Err(message) => {
                self.flash(message);
                return;
            }
        };
        let title = sampler.edit_title();
        self.nav.commit_undo(before, "paste");
        self.nav.clip_view.sampler.layer = pasted.row;
        if pasted.child != ChildChange::Same {
            self.sync_sampler_child(track_idx);
        }
        self.sync_sampler_edit(track_idx);
        self.clamp_sampler_cursors();
        let child = match (&clip, pasted.child) {
            (_, ChildChange::Same) => String::new(),
            (phosphor_app::sampler::clipboard::SamplerClip::Pad { child: Some(c), .. }, _) => format!(
                " \u{00b7} child is now {} \u{00b7} every phrase plays through it",
                c.instrument.label(),
            ),
            _ => String::new(),
        };
        self.flash(format!("{} pasted on {title}{child} \u{00b7} u undoes", clip.label()));
    }
}
