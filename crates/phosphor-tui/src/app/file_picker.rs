//! The file picker's doors and its keys.
//!
//! The picker itself is state — [`phosphor_app::state::FilePicker`] — and
//! knows nothing about sessions or samplers. This is the piece that opens
//! it on the right folder and hands its answer to the thing that was
//! waiting for one: [`App::do_load`] for a project,
//! [`App::do_load_sample`] for a sound. Neither of those is touched. The
//! picker resolves a path and calls them, which is the whole of its job —
//! everything a load *does* (finding the file, decoding it, the undo step,
//! telling the engine) already lives behind those two doors and is not
//! worth a second copy here.
//!
//! # The escape hatch
//!
//! `/` swaps the list for the typed field it replaced. A list can only
//! offer what is in a folder, and a player with a wav on a network share,
//! a path in their clipboard or a habit of typing faster than they can
//! read is entitled to the road they had. It is one keypress and it is in
//! the footer, because an escape hatch nobody can find is not one.
//!
//! The swap closes the picker rather than stacking the field on top of it:
//! one thing on the screen, one set of keys for it, and `esc` means the
//! same thing from either.
//!
//! # `j` is a letter and also the way down
//!
//! Both are true and the list cannot have it both ways: a filter that
//! reserved `j` and `k` could never find `jam` or `kick`, and a list whose
//! `j` typed instead of moving would break the one gesture every other pane
//! here shares. So the filter is empty until something is typed into it,
//! and while it is empty `j`, `k`, `h`, `g` and `G` are the list's. From
//! the first letter typed they are letters, and the arrows — with `ctrl+n`
//! and `ctrl+p` for hands that do not want to leave the home row — are what
//! moves the cursor. The footer says which of the two states it is in,
//! because a key that means two things has to be readable off the screen.

use super::*;

use phosphor_app::state::{FilePicker, PickerPurpose};

impl App {
    /// The folder projects are saved into and browsed from.
    ///
    /// One answer for both. "Where a bare name is saved" and "where the
    /// picker looks" being two answers that could drift apart is exactly
    /// the defect the picker exists to remove — a player would save
    /// `myjam` and then be shown a list without it in.
    ///
    /// Made if it is missing, because a save into a folder that is not
    /// there fails, and the failure is a sentence about a path the player
    /// never typed.
    pub(crate) fn projects_dir(&self) -> std::path::PathBuf {
        self.browse_sessions
            .clone()
            .unwrap_or_else(phosphor_app::paths::session_browse_dir)
    }

    /// Space+O: browse the projects folder.
    pub(crate) fn open_session_picker(&mut self) {
        let dir = self.projects_dir();
        crate::debug_log::user(&format!("file picker: sessions in {}", dir.display()));
        self.nav.file_picker.show(PickerPurpose::OpenSession, dir, None);
    }

    /// `a` on the sampler: browse the samples folder, with this session's
    /// own takes pinned at the top when it has any.
    ///
    /// A take is written into `<session>.samples/` beside the session file
    /// rather than into the shared samples folder, so a player wanting the
    /// thing they recorded ten minutes ago would otherwise have to walk out
    /// of the folder the picker opened on to find it.
    pub(crate) fn open_sample_picker(&mut self) {
        let takes = self
            .session_path
            .as_deref()
            .map(phosphor_app::sampler::sidecar::sidecar_dir);
        let dir = self
            .browse_samples
            .clone()
            .unwrap_or_else(phosphor_app::paths::sample_browse_dir);
        self.nav.file_picker.show(PickerPurpose::LoadSample, dir, takes);
    }

    /// One key, in the picker.
    ///
    /// Every key is answered here and nothing falls through, which is the
    /// rule a modal exists to keep: the letters are a filter, and a `d`
    /// that reached the pad map from inside a filename would delete a
    /// sound. The keys that are not letters are the list's — `j`/`k`,
    /// Enter, `h`, Esc — and they are the same ones the preset browser
    /// uses.
    pub(crate) fn handle_file_picker_keys(&mut self, key: crossterm::event::KeyEvent) {
        use crate::debug_log as dbg;

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                dbg::user("file picker: esc \u{2192} close");
                self.nav.file_picker.close();
            }
            KeyCode::Down => self.nav.file_picker.move_cursor(1),
            KeyCode::Up => self.nav.file_picker.move_cursor(-1),
            KeyCode::PageDown => self.nav.file_picker.move_cursor(8),
            KeyCode::PageUp => self.nav.file_picker.move_cursor(-8),
            KeyCode::Left => {
                self.walk_picker_up();
            }
            KeyCode::Enter => self.choose_in_picker(),
            KeyCode::Backspace => {
                // Backspace widens the filter, and once there is nothing
                // left to widen it is the other way out of a folder — which
                // is what a lifetime of file dialogs has taught the hand.
                if !self.nav.file_picker.backspace() {
                    self.walk_picker_up();
                }
            }
            // The typed-path road, kept open for the paths a list cannot
            // reach. The picker closes: one thing on the screen at a time.
            KeyCode::Char('/') => {
                let purpose = self.nav.file_picker.purpose;
                self.nav.file_picker.close();
                dbg::user("file picker: / \u{2192} type a path");
                match purpose {
                    PickerPurpose::OpenSession => self.nav.input_modal.open_load(),
                    PickerPurpose::LoadSample => self.open_sample_typed_prompt(),
                }
            }
            // The way down a list that is being filtered, for hands that do
            // not want to leave the home row.
            KeyCode::Char('n') if ctrl => self.nav.file_picker.move_cursor(1),
            KeyCode::Char('p') if ctrl => self.nav.file_picker.move_cursor(-1),
            // Held control is a shortcut somebody meant, not a letter. A
            // `ctrl+w` that put a `w` in the filter would be a keystroke
            // arriving as text.
            KeyCode::Char(_) if ctrl => {}
            // `j` and `k` are the way down a list and also letters in a
            // filename. While nothing has been typed they are the list's;
            // from the first letter on they are letters and the arrows move
            // the cursor. See the note at the top of this file.
            KeyCode::Char(ch) => {
                if self.nav.file_picker.filter.is_empty() {
                    match ch {
                        'j' => return self.nav.file_picker.move_cursor(1),
                        'k' => return self.nav.file_picker.move_cursor(-1),
                        'h' => return self.walk_picker_up(),
                        'g' => return self.nav.file_picker.to_end(false),
                        'G' => return self.nav.file_picker.to_end(true),
                        _ => {}
                    }
                }
                self.nav.file_picker.type_char(ch);
            }
            _ => {}
        }
    }

    /// `h`: up one folder, saying so when there is nowhere above.
    fn walk_picker_up(&mut self) {
        if !self.nav.file_picker.up() {
            self.flash("this is the top of the filesystem");
        }
    }

    /// Enter: descend into a folder, or answer with a file.
    fn choose_in_picker(&mut self) {
        let Some(entry) = self.nav.file_picker.selected() else { return };
        let path = entry.path.clone();
        if entry.is_dir {
            self.nav.file_picker.go(path);
            return;
        }
        let purpose = self.nav.file_picker.purpose;
        self.nav.file_picker.close();
        let path = path.display().to_string();
        crate::debug_log::user(&format!("file picker: chose {path}"));
        match purpose {
            PickerPurpose::OpenSession => self.do_load(&path),
            PickerPurpose::LoadSample => self.do_load_sample(&path),
        }
    }
}

/// How much of the picker's list is on the screen, for a terminal of this
/// height.
///
/// Fed to the picker once a frame from the main loop, the way the help
/// card's height is, so that the keys and the drawing agree about where the
/// bottom of the list is.
pub(crate) fn follow_terminal(picker: &mut FilePicker, rows: u16) {
    if picker.open {
        picker.set_page_rows(phosphor_app::state::picker_list_rows(rows));
    }
}
