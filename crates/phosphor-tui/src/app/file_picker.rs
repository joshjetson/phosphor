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
//!
//! The save picker types into a name rather than a filter, and there the
//! letters are never lent out: a name is typed from nothing and has to be
//! able to *start* with any letter, `j` and `h` included, or `jam` is not a
//! filename a player can reach. The arrows walk instead and the footer says
//! so from the first frame. Both pickers ask the same accessor which state
//! they are in — `FilePicker::list_owns_letters` — so the keys and the
//! footer cannot come to different answers.
//!
//! # Where the picker opens
//!
//! On the folder it was last in, for the life of the run, and on
//! [`phosphor_app::paths::sessions_home`] the first time. Two folders are
//! remembered, not one: projects and samples are different places and a
//! player moving between them is not asking for either to follow the other.
//! Save As after an Open therefore starts where the player just was, which
//! is the only folder they have any reason to expect.

use super::*;

use phosphor_app::state::{FilePicker, PickerPurpose, TypedKey};

impl App {
    /// The folder projects are saved into and browsed from.
    ///
    /// One answer for all of it: the open picker, the save picker, the
    /// folder a bare name is written into, and the folder the typed prompts
    /// name. "Where a bare name is saved" and "where the picker looks"
    /// being two answers that could drift apart is exactly the defect that
    /// lost a player's file — see [`phosphor_app::paths::sessions_home`].
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

    /// Space+S, and Ctrl+S on a session that has never been saved: the same
    /// list, with a name line.
    ///
    /// The list rather than a bare field because a save has two halves —
    /// which folder, and called what — and the field only ever asked the
    /// second one. A player could read the folder off the prompt but not
    /// change it, and could not see what was already in there to avoid
    /// naming over it.
    pub(crate) fn open_save_picker(&mut self) {
        let dir = self.projects_dir();
        crate::debug_log::user(&format!("file picker: saving into {}", dir.display()));
        self.nav.file_picker.show(PickerPurpose::SaveSession, dir, None);
        // When a session is already open, its name is offered in the line:
        // Enter saves straight over it (no folder to walk to, no name to
        // retype), and typing replaces it for a spin-off. This is the
        // common case — save what I just changed — and it should cost the
        // one key the player reached for.
        if let Some(stem) = self
            .session_path
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
        {
            self.nav.file_picker.suggest_name(&stem);
        }
    }

    /// Remember where the picker is, so that the next one opens there.
    ///
    /// Called on every change of folder rather than when the picker closes:
    /// the picker can be left by a road that closes it — `/`, or a save —
    /// and the folder the player walked to is theirs either way.
    fn remember_picker_dir(&mut self) {
        let dir = self.nav.file_picker.dir.clone();
        if self.nav.file_picker.purpose.is_session() {
            self.browse_sessions = Some(dir);
        } else {
            self.browse_samples = Some(dir);
        }
    }

    /// Walk the picker into `dir`, and remember it.
    fn picker_go(&mut self, dir: std::path::PathBuf) {
        self.nav.file_picker.go(dir);
        self.remember_picker_dir();
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
                // Backspace widens the filter — or takes back a character of
                // the name — and once there is nothing left it is the other
                // way out of a folder, which is what a lifetime of file
                // dialogs has taught the hand.
                if !self.nav.file_picker.backspace_typed() {
                    self.walk_picker_up();
                }
            }
            // The typed-path road, kept open for the paths a list cannot
            // reach. The picker closes: one thing on the screen at a time.
            // A name half typed goes with it rather than being dropped.
            KeyCode::Char('/') => {
                let purpose = self.nav.file_picker.purpose;
                let folder = self.nav.file_picker.dir.display().to_string();
                let name = self.nav.file_picker.name.clone();
                self.remember_picker_dir();
                self.nav.file_picker.close();
                dbg::user("file picker: / \u{2192} type a path");
                match purpose {
                    PickerPurpose::OpenSession => self.nav.input_modal.open_load_in(&folder),
                    PickerPurpose::SaveSession => {
                        self.nav.input_modal.open_save_typed(&name, &folder);
                    }
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
            // filename. While nothing has been typed into a *filter* they
            // are the list's; from the first letter on they are letters and
            // the arrows move the cursor. On a save they are letters from
            // the start. See the note at the top of this file.
            KeyCode::Char(ch) => {
                if self.nav.file_picker.list_owns_letters() {
                    match ch {
                        'j' => return self.nav.file_picker.move_cursor(1),
                        'k' => return self.nav.file_picker.move_cursor(-1),
                        'h' => return self.walk_picker_up(),
                        'g' => return self.nav.file_picker.to_end(false),
                        'G' => return self.nav.file_picker.to_end(true),
                        _ => {}
                    }
                }
                // A separator is the one refusal worth a word: the player
                // meant a folder, and there is a key for that.
                if self.nav.file_picker.type_letter(ch) == TypedKey::Separator {
                    self.flash("a name cannot hold a folder \u{00b7} / types a whole path");
                }
            }
            _ => {}
        }
    }

    /// `h`: up one folder, saying so when there is nowhere above.
    fn walk_picker_up(&mut self) {
        if self.nav.file_picker.up() {
            self.remember_picker_dir();
        } else {
            self.flash("this is the top of the filesystem");
        }
    }

    /// Enter: descend into a folder, or answer with a file.
    fn choose_in_picker(&mut self) {
        if self.nav.file_picker.purpose == PickerPurpose::SaveSession {
            return self.enter_in_save_picker();
        }
        let Some(entry) = self.nav.file_picker.selected() else { return };
        let path = entry.path.clone();
        if entry.is_dir {
            self.picker_go(path);
            return;
        }
        let purpose = self.nav.file_picker.purpose;
        self.nav.file_picker.close();
        let path = path.display().to_string();
        crate::debug_log::user(&format!("file picker: chose {path}"));
        match purpose {
            PickerPurpose::OpenSession => self.do_load(&path),
            PickerPurpose::LoadSample => self.do_load_sample(&path),
            // Answered at the top of this function: a file row in the save
            // picker is a name to take, not a file to open.
            PickerPurpose::SaveSession => {}
        }
    }

    /// Enter in the save picker: write it, walk into it, or take its name.
    ///
    /// Three meanings for one key and no ambiguity between them, because
    /// they are decided in order by what the player has already done. A name
    /// has been typed: that is the answer, and Enter writes it. Nothing
    /// typed, on a folder: Enter goes in, the way it does in every list
    /// here. Nothing typed, on a project: Enter takes its *name*, which is
    /// the closest a list gets to clicking a file in a save dialog — and it
    /// writes nothing, because a second Enter (and the question it raises)
    /// is what writing over somebody's song should cost.
    fn enter_in_save_picker(&mut self) {
        if self.nav.file_picker.save_path().is_some() {
            return self.commit_picker_save();
        }
        let Some(entry) = self.nav.file_picker.selected() else {
            return self.flash("type a name \u{00b7} or enter on a folder to go in");
        };
        if entry.is_dir {
            let path = entry.path.clone();
            return self.picker_go(path);
        }
        if self.nav.file_picker.adopt_selected_name() {
            let name = self.nav.file_picker.name.clone();
            self.flash(format!("name: {name} \u{00b7} enter again to save over it"));
        }
    }

    /// Write what the name line and the folder add up to, asking first when
    /// something of that name is already there.
    fn commit_picker_save(&mut self) {
        let Some(path) = self.nav.file_picker.save_path() else {
            return self.flash("type a name \u{00b7} or enter on a folder to go in");
        };
        // Saving over the session you already have open is not clobbering
        // someone's work — it is the point of saving. The overwrite question
        // is for a *different* existing file, so a name that resolves to the
        // current session's own path writes straight through.
        let is_current = self
            .session_path
            .as_deref()
            .is_some_and(|open| open == path);
        if path.exists() && !is_current {
            let name = path
                .file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            crate::debug_log::user(&format!("save picker: {} exists \u{2192} asking", path.display()));
            self.pending_save = Some(path);
            self.nav
                .confirm_modal
                .show(crate::state::ConfirmKind::OverwriteSession, &format!("Overwrite {name}?  y/n"));
            return;
        }
        self.write_from_save_picker(&path);
    }

    /// The overwrite question, answered yes.
    pub(crate) fn overwrite_from_save_picker(&mut self) {
        let Some(path) = self.pending_save.take() else { return };
        self.write_from_save_picker(&path);
    }

    /// Do the save the picker asked for, and keep the picker if it would not
    /// go.
    ///
    /// A folder that refuses the write — a read-only disk, somebody else's
    /// directory — is one the player can walk out of, and closing the box
    /// over the failure would leave them holding a sentence with nothing to
    /// press. The name stays in the line, so the second attempt is one
    /// Enter away.
    fn write_from_save_picker(&mut self, path: &std::path::Path) {
        if self.do_save(&path.display().to_string()) {
            self.nav.file_picker.close();
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
