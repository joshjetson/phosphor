//! The little file tree: choosing a project to open, or a sound for a pad.
//!
//! Both jobs used to be a text field. A player who had just saved
//! `myjam.phos` and wanted it back had to remember where the application
//! keeps sessions and type the whole path, which is a thing nobody can do
//! from memory and nothing on the screen would tell them. So this is a
//! list: the folder those files already live in, walked with the same
//! `j`/`k`/Enter/Esc the preset browser uses, because a second grammar for
//! a second list is a grammar nobody learns.
//!
//! # What it is not
//!
//! It does not search, it does not recurse, and selecting a row plays
//! nothing. It shows one directory at a time. That is a deliberate floor
//! rather than a first draft: a recursive search over a home directory is
//! a spinner in a terminal application, and a preview is an audition with
//! no key to stop it.
//!
//! # Saving is the same list
//!
//! A save is the same question with the answer written the other way round:
//! *which folder*, and *called what*. So saving is a third
//! [`PickerPurpose`] rather than a second widget — the same rows, the same
//! walk, the same `/` escape hatch — with one line added for the name and
//! one rule changed: typing edits the **name** instead of narrowing the
//! list. Everything else a player already learned still holds, and the
//! folder the save lands in is the folder they are looking at, which is the
//! whole reason this exists. See [`FilePicker::save_path`].
//!
//! Typing there is *all* typing, letters and walking keys alike, and the
//! arrows walk instead — see [`FilePicker::list_owns_letters`] for why a
//! name that could not begin with `j` is not a name line at all.
//!
//! # The rules of the list
//!
//! * Directories first, then files, each half alphabetical and
//!   case-insensitive — `Drums` and `drums` sit together where a player
//!   looks for them rather than in two blocks either side of `Zither`.
//! * Dot-prefixed names are skipped. They are the platform's business.
//! * Files are filtered by what the picker is *for* — see
//!   [`PickerPurpose::shows`] — but **directories are always shown**, so
//!   the tree can be walked out of the folder it opened on and into
//!   wherever the player actually keeps their samples.
//! * Typing narrows — on the pickers that are reading. Letters, digits,
//!   space and the three characters filenames are actually made of (`.`,
//!   `-`, `_`) go into a filter that is matched as a case-insensitive
//!   substring; Backspace takes one back. The filter belongs to the view it
//!   was typed in and is cleared by a change of directory, because a filter
//!   that survives a `cd` is a folder that looks empty for no visible
//!   reason. On the save picker the same keys are writing a name instead,
//!   and that *does* survive a change of directory — see above.
//!
//! # Reading the directory
//!
//! Every read goes through [`read_dir_sorted`], which is a free function
//! taking a path so that a test can aim it at a temporary directory. A
//! folder that cannot be read is not a panic and not an empty folder: the
//! list is empty and the body says why, because "no files here" and "I was
//! not allowed to look" are different answers and only one of them is the
//! player's fault.

use std::path::{Path, PathBuf};

// ── What the picker is for ──

/// What the picker's answer is for.
///
/// One picker, two jobs, and the difference is in the type rather than in a
/// second modal that would drift out of step with the first — the same
/// shape [`InstrumentPick`](super::InstrumentPick) uses for the instrument
/// menu's two answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerPurpose {
    /// Space+O: the answer is a session to open.
    #[default]
    OpenSession,
    /// `a` on the sampler: the answer is a sound for the pad under the
    /// caret.
    LoadSample,
    /// Space+S, and Ctrl+S on a session that has never been saved: the
    /// answer is a folder and a name to write into it.
    SaveSession,
}

impl PickerPurpose {
    /// The extension a file has to carry to be worth showing.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::OpenSession | Self::SaveSession => crate::paths::SESSION_EXT,
            Self::LoadSample => "wav",
        }
    }

    /// Whether this purpose is about a session rather than a sound.
    ///
    /// Which of the two folders the picker is remembered in hangs off this:
    /// opening and saving share a folder — that is the point of them — and
    /// the sampler keeps its own.
    #[must_use]
    pub const fn is_session(self) -> bool {
        matches!(self, Self::OpenSession | Self::SaveSession)
    }

    /// Whether typing goes into a name rather than into the filter.
    #[must_use]
    pub const fn names_a_file(self) -> bool {
        matches!(self, Self::SaveSession)
    }

    /// Whether a file of this name is one of ours.
    ///
    /// Case-insensitive, because a WAV exported by another application is
    /// as likely to be `KICK.WAV` as `kick.wav`, and a kit that is invisible
    /// for that reason reads as a picker that cannot see the folder.
    #[must_use]
    pub fn shows(self, name: &str) -> bool {
        Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(self.extension()))
    }

    /// What the title bar of the picker says it is for.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::OpenSession => "open project",
            Self::LoadSample => "load sample",
            Self::SaveSession => "save project",
        }
    }

    /// What an empty folder says. Not "no files": a player who has arrived
    /// here has a next move, and this is the line that names it.
    #[must_use]
    pub const fn empty_words(self) -> &'static str {
        match self {
            Self::OpenSession => "no projects yet \u{00b7} ctrl+s saves your first one here",
            Self::LoadSample => "drop .wav files in this folder, or press / to type a path",
            // An empty folder is a perfectly good place to save into, so
            // this says that rather than reading as a dead end.
            Self::SaveSession => "nothing here yet \u{00b7} a name still saves into this folder",
        }
    }

    /// The footer of the box, for a picker with something typed into it or
    /// nothing.
    ///
    /// Two states and two lines, because the keys genuinely change: on the
    /// open pickers the letters stop walking the list once a filter exists,
    /// and on the save picker Enter stops meaning "go in" once a name does.
    /// Here rather than in the renderer because these are the same facts the
    /// key handler branches on — see [`FilePicker::list_owns_letters`] — and
    /// a footer that says one thing while the keys do another is worse than
    /// no footer at all.
    #[must_use]
    pub const fn footer(self, typed: bool) -> &'static [(&'static str, &'static str)] {
        match (self, typed) {
            // `\u{2190}` rather than `bksp` for the walk upwards: both do it,
            // and this line has sixty columns to say six things in. The
            // arrow is the one that costs three characters instead of six.
            (Self::SaveSession, false) => &[
                ("\u{2191}\u{2193}", " move  "),
                ("enter", " go in  "),
                ("\u{2190}", " up  "),
                ("type", " name  "),
                ("/", " path  "),
                ("esc", " cancel"),
            ],
            (Self::SaveSession, true) => &[
                ("\u{2191}\u{2193}", " move  "),
                ("enter", " save  "),
                ("bksp", " edit  "),
                ("/", " path  "),
                ("esc", " cancel"),
            ],
            (_, false) => &[
                ("j/k", " move  "),
                ("enter", " open  "),
                ("h", " up  "),
                ("type", " find  "),
                ("/", " path  "),
                ("esc", " close"),
            ],
            (_, true) => &[
                ("\u{2191}\u{2193}", " move  "),
                ("enter", " open  "),
                ("bksp", " widen  "),
                ("/", " path  "),
                ("esc", " close"),
            ],
        }
    }
}

// ── One row ──

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What it is called on disk, which is also what the filter matches.
    pub name: String,
    /// Where it is, whole — the picker's answer, and what Enter hands to
    /// the loader.
    pub path: PathBuf,
    pub is_dir: bool,
    /// A shortcut put at the top of the list rather than read out of the
    /// folder — the session's own takes directory. Drawn with a word beside
    /// it, because a row that is not where it appears to be needs one.
    pub pinned: bool,
}

impl Entry {
    /// A directory row for `path`, named for its last component.
    #[must_use]
    pub fn dir(path: PathBuf, pinned: bool) -> Self {
        let name = path
            .file_name()
            .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        Self { name, path, is_dir: true, pinned }
    }
}

/// Everything in `dir` worth showing `purpose`, directories first and then
/// files, each half alphabetical and case-insensitive.
///
/// The one door onto the filesystem, so that a test can point the whole
/// picker at a temporary directory by calling this. `Err` is a folder that
/// could not be read at all; one unreadable *entry* inside a readable
/// folder is skipped rather than failing the lot, which is what a mount
/// point or a dangling symlink in the middle of a directory deserves.
pub fn read_dir_sorted(dir: &Path, purpose: PickerPurpose) -> std::io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = item.path();
        // `path().is_dir()` rather than the entry's own file type, so that a
        // symlinked folder is a folder — which is how the player who made
        // the link meant it.
        let is_dir = path.is_dir();
        if !is_dir && !purpose.shows(&name) {
            continue;
        }
        entries.push(Entry { name, path, is_dir, pinned: false });
    }
    // Cached keys: a sort calls its comparator many times per element, and
    // lower-casing a name inside one would build the same string over and
    // over. `!is_dir` sorts false before true, which puts folders first.
    entries.sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    Ok(entries)
}

// ── The picker ──

/// The whole box: borders included, however tall the terminal is.
const BOX_MAX: u16 = 20;

/// The rows the box's list is drawn in, on a terminal of `rows` rows.
///
/// Shared by the renderer and by the loop that tells the picker how much of
/// the list is on the screen, so scrolling and drawing cannot come to
/// different answers about where the bottom is — the help card's rule, for
/// the help card's reason.
#[must_use]
pub fn picker_box_height(rows: u16) -> u16 {
    BOX_MAX.min(rows.saturating_sub(2))
}

/// The list rows inside that box: the two borders, the header, the filter
/// line and the footer are not list.
#[must_use]
pub fn picker_list_rows(rows: u16) -> usize {
    (picker_box_height(rows) as usize).saturating_sub(5)
}

/// The characters that go into the filter.
///
/// Letters, digits, space and the three punctuation marks filenames are
/// actually made of. Everything else is a key, not a letter: `/` is the
/// escape hatch to a typed path, and a filter that swallowed it would take
/// the one road out of the picker away.
#[must_use]
pub fn filter_accepts(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, ' ' | '.' | '-' | '_')
}

/// What a character typed at the picker did.
///
/// Three answers rather than a `bool`, because the caller has a different
/// job for each: a separator in a name is the only one worth a word on the
/// screen, and telling it apart from "that key is not a letter" is the
/// difference between a hint and a shrug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedKey {
    /// It went into the line.
    Took,
    /// Not a character this line takes, and nothing to say about it.
    Refused,
    /// A path separator typed into a name. Names are names here; a whole
    /// path has its own road, behind `/`.
    Separator,
}

/// A directory of files, and a cursor in it.
#[derive(Debug)]
pub struct FilePicker {
    pub open: bool,
    pub purpose: PickerPurpose,
    /// The folder being shown, always absolute: the header has to name
    /// somewhere a player can find outside this application, and `h` has to
    /// be able to walk up from it.
    pub dir: PathBuf,
    /// Everything in `dir`, before the filter.
    pub entries: Vec<Entry>,
    /// Which *visible* row the cursor is on — an index into
    /// [`Self::visible`], not into `entries`, because the filter is what the
    /// player is looking at.
    pub cursor: usize,
    /// First visible row on the screen.
    pub scroll: usize,
    /// What has been typed to narrow the list.
    pub filter: String,
    /// What has been typed as the name to save under.
    ///
    /// Empty on every purpose but [`PickerPurpose::SaveSession`], which has
    /// no filter — one line is typed into, and which one it is belongs to
    /// the purpose. Unlike the filter, this survives a change of folder: it
    /// is the answer rather than a view of the list, and a player who names
    /// their song and then walks into `ideas/` means to save it there.
    pub name: String,
    /// The folder could not be read at all. Kept apart from "empty",
    /// because the two have different words and only one of them is
    /// something the player did.
    pub unreadable: bool,
    /// How many rows of the list fit, set from the terminal each frame.
    page_rows: usize,
}

impl Default for FilePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl FilePicker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: false,
            purpose: PickerPurpose::OpenSession,
            dir: PathBuf::new(),
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            filter: String::new(),
            name: String::new(),
            unreadable: false,
            page_rows: 12,
        }
    }

    /// Open on `dir`, optionally with a shortcut pinned at the top of the
    /// list — the session's own takes folder, which is beside the session
    /// file rather than in here.
    ///
    /// The shortcut belongs to the view it is offered in: walking into
    /// another folder leaves it behind, because a row that means "somewhere
    /// else entirely" is only honest while the list around it is the one it
    /// was offered for.
    pub fn show(&mut self, purpose: PickerPurpose, dir: PathBuf, pinned: Option<PathBuf>) {
        self.open = true;
        self.purpose = purpose;
        self.dir = tidy(dir);
        // A name belongs to the save it was typed for, and the next save
        // starts from nothing rather than from a name somebody abandoned.
        self.name.clear();
        self.read();
        if let Some(pinned) = pinned.filter(|p| p.is_dir()) {
            self.entries.insert(0, Entry::dir(tidy(pinned), true));
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.entries.clear();
        self.filter.clear();
        self.name.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.unreadable = false;
    }

    /// Show another folder: the filter goes with the view it was typed in,
    /// and the cursor starts at the top of the new list.
    pub fn go(&mut self, dir: PathBuf) {
        self.dir = tidy(dir);
        self.read();
    }

    /// `h`: the folder above this one, answering whether there was one.
    ///
    /// The filesystem root has no parent and neither does a path that has
    /// run out of components, so this stops rather than walking into the
    /// empty path — which is not a directory and reads on the screen as the
    /// picker having lost its place.
    pub fn up(&mut self) -> bool {
        let Some(parent) = self.dir.parent().filter(|p| !p.as_os_str().is_empty()) else {
            return false;
        };
        let parent = parent.to_path_buf();
        self.go(parent);
        true
    }

    /// Fill the list from `dir`, and put the cursor at the top of it.
    fn read(&mut self) {
        self.filter.clear();
        self.cursor = 0;
        self.scroll = 0;
        match read_dir_sorted(&self.dir, self.purpose) {
            Ok(entries) => {
                self.entries = entries;
                self.unreadable = false;
            }
            Err(_) => {
                self.entries.clear();
                self.unreadable = true;
            }
        }
    }

    // ── The list ──

    /// The rows the filter leaves, which is what is on the screen.
    ///
    /// One iterator behind the list, the count and the cursor's row, so the
    /// three can never disagree about which rows are showing.
    fn matching(&self) -> impl Iterator<Item = &Entry> + '_ {
        self.entries.iter().filter(|entry| contains_fold(&entry.name, &self.filter))
    }

    /// The rows on the screen: everything, or the ones the filter matches.
    #[must_use]
    pub fn visible(&self) -> Vec<&Entry> {
        self.matching().collect()
    }

    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.matching().count()
    }

    /// The row the cursor is standing on.
    #[must_use]
    pub fn selected(&self) -> Option<&Entry> {
        self.matching().nth(self.cursor)
    }

    /// Walk the list, stopping at both ends: a cursor that wraps round is a
    /// cursor that opens the wrong file on a long list.
    pub fn move_cursor(&mut self, delta: i32) {
        let count = self.visible_count();
        if count == 0 {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        self.cursor = (self.cursor as i32 + delta).clamp(0, count as i32 - 1) as usize;
        self.follow_cursor();
    }

    /// `g` and `G`: the ends of the list.
    pub fn to_end(&mut self, bottom: bool) {
        self.cursor = if bottom { self.visible_count().saturating_sub(1) } else { 0 };
        self.follow_cursor();
    }

    /// Narrow the list, answering whether the character was one the filter
    /// takes.
    pub fn type_char(&mut self, ch: char) -> bool {
        if !filter_accepts(ch) {
            return false;
        }
        self.filter.push(ch);
        self.after_filter();
        true
    }

    /// Widen it again, answering whether there was anything to take back.
    pub fn backspace(&mut self) -> bool {
        let popped = self.filter.pop().is_some();
        if popped {
            self.after_filter();
        }
        popped
    }

    // ── The name being saved ──

    /// The line this purpose types into: the name on a save, the filter
    /// everywhere else.
    ///
    /// One accessor so the keys, the footer and the renderer cannot come to
    /// different conclusions about which of the picker's two states it is
    /// in.
    #[must_use]
    pub fn typed(&self) -> &str {
        if self.purpose.names_a_file() { &self.name } else { &self.filter }
    }

    /// Whether `j`, `k`, `h`, `g` and `G` are the list's rather than
    /// letters.
    ///
    /// On the open pickers they are until something has been typed, and from
    /// the first character they are letters — a filter is optional there,
    /// most players never type one, and the walk has to be the walk every
    /// other list here has.
    ///
    /// On the **save** picker they never are, and that is deliberate. A name
    /// is not optional, and it is typed from nothing: if the first character
    /// belonged to the list then no name could *begin* with `j`, `k`, `h`,
    /// `g` or `G` — and a player typing `ghost_take` would send `g` to the
    /// top of the list, `h` a folder upwards, and then save `ost_take` into
    /// a folder they never chose. That is the same defect this picker was
    /// built to end, wearing a different hat. The arrows, `ctrl+n`/`ctrl+p`
    /// and the page keys walk instead, Backspace on an empty name still goes
    /// up, and the footer says so from the first frame.
    #[must_use]
    pub fn list_owns_letters(&self) -> bool {
        !self.purpose.names_a_file() && self.typed().is_empty()
    }

    /// The footer for the state the picker is actually in.
    ///
    /// Keyed on whether anything has been typed rather than on who owns the
    /// letters, because on a save those are two different questions: the
    /// letters are never the list's there, but Enter still means "go in"
    /// until there is a name for it to write.
    #[must_use]
    pub fn footer(&self) -> &'static [(&'static str, &'static str)] {
        self.purpose.footer(!self.typed().is_empty())
    }

    /// A letter, wherever this purpose's letters go.
    ///
    /// The one door, so that the key handler never has to know which line it
    /// is feeding — the purpose knows, and it is the purpose that would be
    /// wrong.
    pub fn type_letter(&mut self, ch: char) -> TypedKey {
        if self.purpose.names_a_file() {
            return self.type_name(ch);
        }
        if self.type_char(ch) { TypedKey::Took } else { TypedKey::Refused }
    }

    /// A character into the name being saved under.
    ///
    /// Wider than the filter's set — a filter is matched against names that
    /// already exist, while a name is being invented, and a player who wants
    /// `take (2)!` is entitled to it. What it will not take is a path
    /// separator: this line names a file *in the folder on the screen*, and
    /// a name that quietly reached into another folder would put the file
    /// somewhere the picker was not showing, which is the whole defect this
    /// picker was built to end. `\` is refused alongside `/` on every
    /// platform, because a session named across a separator on one of them
    /// is a session that will not open on the other.
    pub fn type_name(&mut self, ch: char) -> TypedKey {
        if ch == '/' || ch == '\\' || std::path::is_separator(ch) {
            return TypedKey::Separator;
        }
        if ch.is_control() {
            return TypedKey::Refused;
        }
        self.name.push(ch);
        TypedKey::Took
    }

    /// Backspace, wherever this purpose's letters go — answering whether
    /// there was anything to take back, which is what makes an empty line's
    /// Backspace the way up a folder.
    pub fn backspace_typed(&mut self) -> bool {
        if self.purpose.names_a_file() {
            return self.name.pop().is_some();
        }
        self.backspace()
    }

    /// Enter on a file row with nothing typed: that file's name, in the name
    /// line.
    ///
    /// The nearest thing a list has to clicking a file in a save dialog, and
    /// it needs no key of its own: one press puts the name up where it can
    /// be read and edited, a second press commits it — and because the file
    /// is already there, that second press is the one the overwrite question
    /// answers. Nothing is overwritten by a single keystroke.
    ///
    /// The extension comes off, because the name line adds it back — a row
    /// adopted as `jam.phos` would read `jam.phos.phos`.
    pub fn adopt_selected_name(&mut self) -> bool {
        let Some(entry) = self.selected().filter(|e| !e.is_dir) else {
            return false;
        };
        let stem = Path::new(&entry.name)
            .file_stem()
            .map_or_else(|| entry.name.clone(), |s| s.to_string_lossy().into_owned());
        self.name = stem;
        true
    }

    /// Where Enter would write, or `None` when nothing has been named.
    ///
    /// The folder on the screen joined to the name on the screen, with the
    /// extension every session carries — one answer, used both to ask
    /// "overwrite?" and to do the writing, because a question asked about a
    /// different path than the one written is worse than no question.
    #[must_use]
    pub fn save_path(&self) -> Option<PathBuf> {
        let name = self.name.trim();
        if name.is_empty() {
            return None;
        }
        Some(crate::paths::with_session_extension(self.dir.join(name)))
    }

    /// The `.phos` drawn dim after the cursor on the name line, or nothing
    /// when what has been typed already carries it — so the line never reads
    /// `jam.phos.phos` while saving `jam.phos`.
    #[must_use]
    pub fn name_suffix(&self) -> &'static str {
        if crate::paths::is_session_file(Path::new(self.name.trim())) {
            return "";
        }
        crate::paths::SESSION_DOT_EXT
    }

    /// The cursor after the list under it changed length.
    fn after_filter(&mut self) {
        self.cursor = self.cursor.min(self.visible_count().saturating_sub(1));
        self.follow_cursor();
    }

    /// Told the terminal's height each frame, so that "the bottom" means the
    /// same thing to the keys and to the drawing.
    pub fn set_page_rows(&mut self, rows: usize) {
        self.page_rows = rows.max(1);
        self.follow_cursor();
    }

    #[must_use]
    pub fn page_rows(&self) -> usize {
        self.page_rows
    }

    /// Keep the cursor's row on the screen, and the screen full where the
    /// list is long enough to fill it.
    fn follow_cursor(&mut self) {
        let rows = self.page_rows.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
        self.scroll = self.scroll.min(self.visible_count().saturating_sub(rows));
    }

    // ── What the box says ──

    /// The folder being shown, cut to `width` characters.
    #[must_use]
    pub fn header(&self, width: usize) -> String {
        elide_left(&self.dir.display().to_string(), width)
    }

    /// The one line the body shows when there is nothing to list, or
    /// nothing when there is.
    #[must_use]
    pub fn empty_words(&self) -> Option<&'static str> {
        if self.visible_count() > 0 {
            return None;
        }
        Some(if self.unreadable {
            "this folder could not be read"
        } else if !self.filter.is_empty() {
            "nothing here matches \u{00b7} backspace widens it"
        } else {
            self.purpose.empty_words()
        })
    }
}

// ── Helpers ──

/// A path cut from the *left* to `width` characters, with the mark the
/// input field uses to say it did.
///
/// From the left because the end of a path is the part that says where you
/// are; the beginning is `/Users/somebody/` on every row of every machine.
/// One function for the picker's header and the save prompt's folder line,
/// so the two cut the same way.
#[must_use]
pub fn elide_left(text: &str, width: usize) -> String {
    let length = text.chars().count();
    if length <= width || width == 0 {
        return text.to_string();
    }
    let skip = length - (width - 1).max(1);
    format!("\u{2026}{}", text.chars().skip(skip).collect::<String>())
}

/// An absolute path with no trailing separator and no `.` in the middle of
/// it, so that the header reads as one place and `parent` walks properly.
fn tidy(dir: PathBuf) -> PathBuf {
    let joined = if dir.is_absolute() {
        dir
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(dir),
            Err(_) => dir,
        }
    };
    joined.components().collect()
}

/// Case-insensitive substring, without building a lower-cased copy of
/// either string: this runs over every row of the list on every keystroke
/// and on every frame.
fn contains_fold(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack
        .char_indices()
        .any(|(at, _)| starts_with_fold(&haystack[at..], needle))
}

fn starts_with_fold(haystack: &str, needle: &str) -> bool {
    let mut rest = haystack.chars();
    needle
        .chars()
        .all(|wanted| rest.next().is_some_and(|got| got.eq_ignore_ascii_case(&wanted)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own, with the files it names in it.
    fn scratch(tag: &str, files: &[&str], dirs: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("phosphor-picker-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in dirs {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        for name in files {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        dir
    }

    fn names(picker: &FilePicker) -> Vec<String> {
        picker.visible().iter().map(|e| e.name.clone()).collect()
    }

    /// Folders first, then files, each half alphabetical whatever case they
    /// were typed in — and the platform's dotted files are nobody's
    /// business but the platform's.
    #[test]
    fn the_list_puts_folders_first_and_ignores_dotted_names() {
        let dir = scratch(
            "sort",
            &["Zither.phos", "apple.phos", "Banjo.phos", ".hidden.phos", "notes.txt"],
            &["takes", "Archive", ".git"],
        );
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::OpenSession, dir.clone(), None);

        assert_eq!(
            names(&picker),
            vec!["Archive", "takes", "apple.phos", "Banjo.phos", "Zither.phos"],
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The purpose filters the files and never the folders: a player whose
    /// samples are three directories away has to be able to walk there.
    #[test]
    fn a_purpose_hides_other_files_but_never_a_folder() {
        let dir = scratch(
            "purpose",
            &["kick.wav", "SNARE.WAV", "jam.phos", "readme.txt", "loop.wave"],
            &["drums"],
        );
        let mut picker = FilePicker::new();

        picker.show(PickerPurpose::LoadSample, dir.clone(), None);
        assert_eq!(names(&picker), vec!["drums", "kick.wav", "SNARE.WAV"]);

        picker.show(PickerPurpose::OpenSession, dir.clone(), None);
        assert_eq!(names(&picker), vec!["drums", "jam.phos"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Typing narrows, Backspace widens, and the cursor is never left
    /// pointing past the end of what it narrowed to.
    #[test]
    fn typing_narrows_the_list_and_the_cursor_stays_inside_it() {
        let dir = scratch("filter", &["kick.wav", "kick_hard.wav", "snare.wav"], &[]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::LoadSample, dir.clone(), None);
        picker.move_cursor(2);
        assert_eq!(picker.selected().map(|e| e.name.clone()), Some("snare.wav".into()));

        assert!(picker.type_char('K'), "an ordinary letter was refused");
        assert_eq!(names(&picker), vec!["kick.wav", "kick_hard.wav"]);
        assert_eq!(picker.cursor, 1, "the cursor was left past the end of the list");
        assert!(picker.type_char('_'));
        assert_eq!(names(&picker), vec!["kick_hard.wav"]);
        assert_eq!(picker.selected().map(|e| e.name.clone()), Some("kick_hard.wav".into()));

        // `/` is the escape hatch, not a letter: a filter that swallowed it
        // would take the way out of the picker away.
        assert!(!picker.type_char('/'), "the filter swallowed the escape hatch");
        assert_eq!(picker.filter, "K_");

        assert!(picker.backspace());
        assert_eq!(names(&picker), vec!["kick.wav", "kick_hard.wav"]);
        assert!(picker.backspace());
        assert_eq!(names(&picker).len(), 3);
        assert!(!picker.backspace(), "an empty filter had something to take back");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A filter belongs to the view it was typed in. Carrying one into
    /// another folder shows an empty list for a reason nobody can see.
    #[test]
    fn a_filter_does_not_survive_a_change_of_folder() {
        let dir = scratch("cd", &["jam.phos"], &["ideas"]);
        std::fs::write(dir.join("ideas").join("sketch.phos"), b"{}").unwrap();
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::OpenSession, dir.clone(), None);
        picker.type_char('j');
        assert_eq!(names(&picker), vec!["jam.phos"]);

        let into = dir.join("ideas");
        picker.go(into.clone());
        assert!(picker.filter.is_empty(), "the filter followed the cursor into the folder");
        assert_eq!(names(&picker), vec!["sketch.phos"]);
        assert_eq!(picker.cursor, 0);

        assert!(picker.up(), "there was no way back out of the folder");
        assert_eq!(picker.dir, tidy(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `h` at the top of the filesystem does nothing at all — not a walk
    /// into the empty path, which is not a folder and lists as one that has
    /// vanished.
    #[test]
    fn the_walk_upwards_stops_at_the_root() {
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::OpenSession, PathBuf::from("/"), None);
        let root = picker.dir.clone();
        assert!(!picker.up(), "the picker walked above the root");
        assert_eq!(picker.dir, root);
    }

    /// A folder that is not there is not a panic, not an empty folder, and
    /// says which of the two it is.
    #[test]
    fn a_folder_that_cannot_be_read_says_so_rather_than_looking_empty() {
        let mut picker = FilePicker::new();
        picker.show(
            PickerPurpose::OpenSession,
            std::env::temp_dir().join("phosphor-picker-nowhere-at-all"),
            None,
        );
        assert!(picker.entries.is_empty());
        assert!(picker.selected().is_none());
        assert_eq!(picker.empty_words(), Some("this folder could not be read"));

        // An empty folder that *was* read says what to do next instead.
        let dir = scratch("emptywords", &[], &[]);
        picker.show(PickerPurpose::OpenSession, dir.clone(), None);
        assert_eq!(picker.empty_words(), Some(PickerPurpose::OpenSession.empty_words()));
        picker.show(PickerPurpose::LoadSample, dir.clone(), None);
        assert_eq!(picker.empty_words(), Some(PickerPurpose::LoadSample.empty_words()));

        // ...and a folder with something in it says nothing.
        std::fs::write(dir.join("kick.wav"), b"x").unwrap();
        picker.show(PickerPurpose::LoadSample, dir.clone(), None);
        assert_eq!(picker.empty_words(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The row the cursor is on resolves to a whole path, which is what the
    /// loader is handed — never a name that has to be resolved again
    /// against a folder the loader does not know about.
    #[test]
    fn the_selected_row_carries_the_whole_path() {
        let dir = scratch("resolve", &["jam.phos"], &["ideas"]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::OpenSession, dir.clone(), None);

        let folder = picker.selected().expect("the folder is not in the list");
        assert!(folder.is_dir);
        assert_eq!(folder.path, tidy(dir.clone()).join("ideas"));

        picker.move_cursor(1);
        let file = picker.selected().expect("the session is not in the list");
        assert!(!file.is_dir);
        assert_eq!(file.path, tidy(dir.clone()).join("jam.phos"));
        assert!(file.path.exists(), "the path the loader is handed does not exist");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The takes folder is pinned at the top of the list it was offered in,
    /// and does not follow the player into another folder.
    #[test]
    fn a_pinned_folder_leads_the_list_and_belongs_to_its_view() {
        let dir = scratch("pinned", &["kick.wav"], &["archive"]);
        let takes = dir.join("archive").join("jam.samples");
        std::fs::create_dir_all(&takes).unwrap();
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::LoadSample, dir.clone(), Some(takes.clone()));
        assert_eq!(names(&picker), vec!["jam.samples", "archive", "kick.wav"]);
        assert!(picker.entries[0].pinned, "the shortcut is not marked as one");

        picker.go(takes);
        assert!(!picker.entries.iter().any(|e| e.pinned), "the shortcut followed the cursor");

        // A shortcut that is not a folder is not offered at all.
        picker.show(PickerPurpose::LoadSample, dir.clone(), Some(dir.join("nowhere")));
        assert_eq!(names(&picker), vec!["archive", "kick.wav"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The cursor's row stays on the screen on a list longer than the box,
    /// and the window never scrolls past the end of the list.
    #[test]
    fn the_window_follows_the_cursor_and_stops_at_the_end() {
        let files: Vec<String> = (0..30).map(|i| format!("take{i:02}.phos")).collect();
        let names: Vec<&str> = files.iter().map(String::as_str).collect();
        let dir = scratch("scroll", &names, &[]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::OpenSession, dir.clone(), None);
        picker.set_page_rows(10);

        picker.to_end(true);
        assert_eq!(picker.cursor, 29);
        assert_eq!(picker.scroll, 20, "the last row is off the bottom of the box");
        picker.to_end(false);
        assert_eq!((picker.cursor, picker.scroll), (0, 0));

        // A filter that shortens the list brings the window back with it.
        picker.to_end(true);
        picker.type_char('0');
        assert!(picker.scroll + picker.page_rows() > picker.cursor);
        assert!(picker.cursor < picker.visible_count());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The header names the folder, cut from the left when it is too long —
    /// the end of a path is the part that says where you are.
    ///
    /// Asserted against the picker's own tidied directory rather than a
    /// spelled-out literal: `/tmp` is not even an absolute path on Windows
    /// (no drive), so `show` resolves it to `D:\tmp` there — which is the
    /// code doing its job and the old literal being a Unix habit.
    #[test]
    fn the_header_keeps_the_end_of_a_long_path() {
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::OpenSession, std::env::temp_dir(), None);
        assert_eq!(picker.header(400), picker.dir.display().to_string());

        let deep: PathBuf =
            ["somebody", "very", "deep", "tree", "sessions"].iter().collect();
        picker.dir = std::env::temp_dir().join(deep);
        let cut = picker.header(20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.starts_with('\u{2026}'), "nothing said the path was cut: {cut}");
        assert!(cut.ends_with("sessions"), "the cut took the part that matters: {cut}");
    }

    // ── Saving ──

    /// The save picker lists the same two things the open one does: folders
    /// to walk into, and the projects already here — which are context, not
    /// a menu. Anything else in the folder is still none of its business.
    #[test]
    fn the_save_picker_lists_folders_to_walk_and_projects_for_context() {
        let dir = scratch("savelist", &["jam.phos", "notes.txt", "kick.wav"], &["ideas"]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::SaveSession, dir.clone(), None);

        assert_eq!(names(&picker), vec!["ideas", "jam.phos"]);
        assert!(picker.name.is_empty(), "the name line did not start empty");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every letter goes into the name, the five that walk a list included.
    ///
    /// A name is typed from nothing and cannot be approximated: if `j` and
    /// `h` belonged to the list while the name was empty, `ghost_take` would
    /// walk to the top of the list, then up a folder, and save `ost_take`
    /// somewhere nobody chose. The open picker can lend those keys out
    /// because its filter is optional. This one cannot.
    #[test]
    fn every_letter_goes_into_the_name_including_the_ones_that_walk() {
        let dir = scratch("savetype", &["jam.phos"], &["ideas"]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::SaveSession, dir.clone(), None);
        assert!(!picker.list_owns_letters(), "the save picker lent the list its letters");

        for ch in "ghost_take (2)!".chars() {
            assert_eq!(picker.type_letter(ch), TypedKey::Took, "the name refused {ch:?}");
        }
        assert_eq!(picker.name, "ghost_take (2)!");
        assert_eq!(picker.dir, tidy(dir.clone()), "a letter walked the picker to another folder");
        assert_eq!(names(&picker).len(), 2, "the name narrowed the list like a filter");
        assert!(picker.filter.is_empty(), "the letters reached the filter");

        // Backspace edits the name, and says when there is nothing left —
        // which is what makes the next press the way up a folder.
        for _ in 0.."ghost_take (2)!".len() {
            assert!(picker.backspace_typed());
        }
        assert!(picker.name.is_empty());
        assert!(!picker.backspace_typed(), "an empty name had something to take back");
        assert!(
            !picker.list_owns_letters(),
            "an emptied name handed the letters back to the list",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A name names a file in the folder on the screen. A separator would
    /// reach into another folder — which is the one thing this picker exists
    /// to make impossible — so it is refused, and said out loud.
    #[test]
    fn a_separator_typed_into_a_name_is_refused() {
        let dir = scratch("savesep", &[], &[]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::SaveSession, dir.clone(), None);
        picker.type_letter('a');

        for ch in ['/', '\\', std::path::MAIN_SEPARATOR] {
            assert_eq!(
                picker.type_letter(ch),
                TypedKey::Separator,
                "{ch:?} went into a name",
            );
        }
        assert_eq!(picker.name, "a", "a separator reached the name anyway");
        assert_eq!(picker.type_letter('\u{7}'), TypedKey::Refused, "a control character typed");
        assert_eq!(picker.name, "a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Enter on a project row takes its name, extension and all, and leaves
    /// it where it can be read and edited. Nothing is written by that press.
    #[test]
    fn enter_on_a_project_adopts_its_name() {
        let dir = scratch("saveadopt", &["neon_causeway.phos"], &["ideas"]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::SaveSession, dir.clone(), None);

        // The folder is the first row, and a folder is not a name.
        assert!(!picker.adopt_selected_name(), "a folder was adopted as a name");
        picker.move_cursor(1);
        assert!(picker.adopt_selected_name(), "the project row was not adopted");
        assert_eq!(picker.name, "neon_causeway", "the extension came along and would double");
        assert_eq!(
            picker.save_path(),
            Some(tidy(dir.clone()).join("neon_causeway.phos")),
            "the adopted name does not point back at the file it came from",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Where Enter writes: the folder on the screen, the name on the screen,
    /// and the extension every session carries — added once, however the
    /// player spelled it.
    #[test]
    fn the_save_path_is_the_folder_on_the_screen_and_the_name_on_it() {
        let dir = scratch("savepath", &[], &["ideas"]);
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::SaveSession, dir.clone(), None);
        assert_eq!(picker.save_path(), None, "an empty name named a file");
        assert_eq!(picker.name_suffix(), ".phos", "the extension is not offered");

        for ch in "  myjam  ".chars() {
            picker.type_letter(ch);
        }
        assert_eq!(
            picker.save_path(),
            Some(tidy(dir.clone()).join("myjam.phos")),
            "the name was not trimmed into the folder on the screen",
        );

        // Spelled with the extension: it is not added twice, and the line
        // stops offering it.
        picker.name = "myjam.phos".into();
        assert_eq!(picker.save_path(), Some(tidy(dir.clone()).join("myjam.phos")));
        assert_eq!(picker.name_suffix(), "");

        // A name survives a walk into another folder — it is the answer, not
        // a view of the list — and follows it into the path.
        picker.name = "myjam".into();
        picker.go(dir.join("ideas"));
        assert_eq!(picker.name, "myjam", "the name was lost walking into a folder");
        assert_eq!(
            picker.save_path(),
            Some(tidy(dir.join("ideas")).join("myjam.phos")),
            "the save did not follow the picker into the folder",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The walk upwards stops at the root on a save too — with a name typed,
    /// which is the state that has the most to lose.
    #[test]
    fn the_save_pickers_walk_upwards_stops_at_the_root() {
        let mut picker = FilePicker::new();
        picker.show(PickerPurpose::SaveSession, PathBuf::from("/"), None);
        picker.name = "myjam".into();
        let root = picker.dir.clone();
        assert!(!picker.up(), "the save picker walked above the root");
        assert_eq!(picker.dir, root);
        assert_eq!(picker.name, "myjam", "the refused walk took the name with it");
    }

    /// The footer is the one line that says which of the two states the keys
    /// are in, so it changes with them — on every purpose.
    #[test]
    fn the_footer_names_the_keys_that_are_actually_moving() {
        let dir = scratch("footer", &["jam.phos"], &[]);
        let mut picker = FilePicker::new();

        let words = |p: &FilePicker| -> String {
            p.footer().iter().map(|(k, w)| format!("{k}{w}")).collect()
        };

        picker.show(PickerPurpose::OpenSession, dir.clone(), None);
        assert!(words(&picker).contains("j/k move"), "the list does not say it walks");
        picker.type_letter('j');
        assert!(words(&picker).contains("bksp widen"), "the filter does not say it widens");

        picker.show(PickerPurpose::SaveSession, dir.clone(), None);
        let empty = words(&picker);
        assert!(
            empty.contains("\u{2191}\u{2193} move") && !empty.contains("j/k"),
            "the save footer offers letters the save picker does not answer: {empty}",
        );
        assert!(empty.contains("type name"), "nothing says typing names the file");
        assert!(empty.contains("esc cancel"), "esc does not say it cancels the save");
        assert!(empty.contains("\u{2190} up"), "nothing says how to leave the folder");
        // Sixty columns is what the box has, borders and margins taken off.
        assert!(
            empty.chars().count() + 2 <= 60,
            "the save footer does not fit the box it is drawn in: {} columns",
            empty.chars().count() + 2,
        );
        picker.type_letter('j');
        let typed = words(&picker);
        assert!(typed.contains("enter save"), "the named state does not say enter saves");
        assert!(typed.contains("bksp edit"), "the named state does not say bksp edits");
        assert!(typed.contains("/ path"), "the typed-path road left the footer");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The box and the list it holds are measured once, so the keys and the
    /// drawing agree about where the bottom is.
    #[test]
    fn the_box_never_outgrows_the_terminal() {
        for rows in [0u16, 1, 3, 8, 24, 60] {
            let height = picker_box_height(rows);
            assert!(height <= rows.saturating_sub(2), "the box is taller than the terminal");
            assert!(height <= BOX_MAX);
            assert_eq!(picker_list_rows(rows), (height as usize).saturating_sub(5));
        }
    }
}
