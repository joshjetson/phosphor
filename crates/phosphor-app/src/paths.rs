//! Where Phosphor keeps the files it owns.
//!
//! One module rather than a `HOME` lookup at each call site, because the three
//! places that needed a home directory — the theme preference, the preset
//! banks and the debug log — each hand-rolled it, and all three got the same
//! answer wrong in the same way. `HOME` is a Unix variable. Windows does not
//! set it, so every one of those lookups came back `None` there, and the call
//! sites treat `None` as "do nothing": a player saved a preset, was told
//! nothing, and the preset did not exist. Losing work quietly is the worst
//! failure this application has, so the rule lives in one place and is tested.
//!
//! The resolution rule, in order:
//!
//! 1. `PHOSPHOR_HOME`, if it is set to something non-blank. Names the
//!    directory itself, not a parent — that is what makes a portable install
//!    or an isolated test run possible.
//! 2. On Unix, `$HOME/.phosphor`. Exactly what every previous version wrote,
//!    and pinned by a test so it stays that way.
//! 3. On Windows, `%APPDATA%\phosphor`, falling back to
//!    `%USERPROFILE%\AppData\Roaming\phosphor`. `%APPDATA%` is where Windows
//!    keeps per-user application data and is what the platform's own file
//!    dialogs will show; a dotted directory in the profile root is a Unix
//!    habit that does not belong there. `HOME` is deliberately *not* consulted
//!    on Windows even though MSYS and Git Bash set it, because then the same
//!    installation would keep two sets of presets depending on which shell
//!    launched it.
//!
//! An empty or all-whitespace variable counts as unset. `HOME=""` used to
//! produce the relative path `.phosphor`, which scatters presets into whatever
//! directory the process happened to start in — the exact outcome the `None`
//! branch exists to prevent.
//!
//! [`Convention`] is a value rather than a `cfg` so the Windows rule can be
//! tested on a Unix machine. A `#[cfg(windows)]` function is a function nobody
//! here can run.
//!
//! The same argument settles where *sessions* go, and for the same reason:
//! [`sessions_home`] is the single folder every save and every list answers
//! from. When those two were separate answers, one of them consulted the
//! working directory, and a player's song was written somewhere the picker
//! never looked. See that function.

use std::path::{Path, PathBuf};

// ── Platform convention ──

/// Which platform's directory layout to follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Convention {
    /// A dotted directory in the home directory: `$HOME/.phosphor`.
    Unix,
    /// The roaming application-data directory: `%APPDATA%\phosphor`.
    Windows,
}

/// The convention this build follows.
#[cfg(windows)]
pub const NATIVE: Convention = Convention::Windows;

/// The convention this build follows.
#[cfg(not(windows))]
pub const NATIVE: Convention = Convention::Unix;

/// Environment variable that names the application directory outright.
pub const OVERRIDE_VAR: &str = "PHOSPHOR_HOME";

/// Directory name under `%APPDATA%`. Not dotted: Windows does not hide files
/// by name, and `AppData\Roaming\.phosphor` looks like a mistake.
const WINDOWS_DIR: &str = "phosphor";

/// Directory name under `$HOME`.
const UNIX_DIR: &str = ".phosphor";

/// The one folder sessions live in, under the application directory.
const SESSIONS_DIR: &str = "sessions";

/// The extension every session file carries.
///
/// One constant rather than a `"phos"` in the saver, another in the opener
/// and a third in the picker's filter: three spellings of the same fact are
/// three chances for a file to be written where nothing will list it.
pub const SESSION_EXT: &str = "phos";

/// The same extension with its dot, for the places that show it to the
/// player rather than compare it. Pinned to [`SESSION_EXT`] by a test,
/// because `concat!` will not take a constant.
pub const SESSION_DOT_EXT: &str = ".phos";

// ── Resolution ──

/// The directory holding everything Phosphor owns, or `None` when the
/// environment names no home directory at all.
///
/// `None` is not a path to fall back on — there genuinely is nowhere to write
/// — so callers say so rather than writing relative to the working directory.
pub fn app_dir() -> Option<PathBuf> {
    app_dir_in(NATIVE, |key| std::env::var(key).ok())
}

/// [`app_dir`] with the platform and the environment supplied.
///
/// The whole rule is in here, as a function of its inputs, so both platforms'
/// answers can be asserted from any machine.
pub fn app_dir_in(convention: Convention, env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let var = |key: &str| {
        env(key)
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
    };

    if let Some(dir) = var(OVERRIDE_VAR) {
        return Some(dir);
    }

    match convention {
        Convention::Unix => Some(var("HOME")?.join(UNIX_DIR)),
        Convention::Windows => var("APPDATA")
            .or_else(|| var("USERPROFILE").map(|p| p.join("AppData").join("Roaming")))
            .map(|p| p.join(WINDOWS_DIR)),
    }
}

/// Where user preset banks live — `<app dir>/presets`.
pub fn preset_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("presets"))
}

/// Where sessions live when the player has not named somewhere else —
/// `<app dir>/sessions`.
pub fn session_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join(SESSIONS_DIR))
}

/// Where the sampler looks for sound files by default —
/// `<app dir>/samples`. A kit dropped in here loads by bare name from
/// any session.
pub fn samples_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("samples"))
}

/// Where to look for a sample the player named in the pad prompt.
///
/// The same shape as [`find_session`], with the sampler's own homes: as
/// typed, then against the working directory, then `<app dir>/samples`,
/// then the app dir itself — and a name with no extension tries `.wav`
/// at every step, so `kick` finds `kick.wav` wherever it lives.
pub fn find_sample(input: &Path) -> PathBuf {
    find_sample_in(input, app_dir().as_deref())
}

/// [`find_sample`] with the application directory supplied.
pub fn find_sample_in(input: &Path, app: Option<&Path>) -> PathBuf {
    let candidates = |p: &Path| -> Vec<PathBuf> {
        let mut v = vec![p.to_path_buf()];
        if p.extension().is_none() {
            v.push(p.with_extension("wav"));
        }
        v
    };
    for c in candidates(input) {
        if c.exists() {
            return c;
        }
    }
    if input.is_absolute() {
        return input.to_path_buf();
    }
    if let Some(app) = app {
        for base in [app.join("samples"), app.to_path_buf()] {
            for c in candidates(&base.join(input)) {
                if c.exists() {
                    return c;
                }
            }
        }
    }
    input.to_path_buf()
}

// ── The sessions home ──

/// The one folder sessions belong to: `<app dir>/sessions`, whatever
/// directory the process was started in.
///
/// Everything that has an opinion about where a session goes answers from
/// here — the save picker, the open picker, [`session_prompt_dir`],
/// [`session_browse_dir`] and [`save_target`] — so that they cannot come to
/// different answers. That is not tidiness; it is the defect this function
/// was written to remove.
///
/// **The defect.** This used to prefer a `sessions` directory in the working
/// directory when there was one, and fall back to `<app dir>/sessions`
/// otherwise. The save wrote one file into one folder, correctly; but launch
/// the application from somewhere else and the *picker* consulted the new
/// working directory, found no local `sessions`, and listed the application
/// directory instead. A player saved `911.phos`, reopened the picker from a
/// different terminal, and the file was not in the list. It had never moved.
/// The two answers had. A save and an open that disagree about "the" folder
/// lose work in the only way that matters: the file exists and nobody can
/// find it.
///
/// A path typed by hand is still honoured exactly as typed — see
/// [`save_target`] — and a session saved under the old rule still opens, by
/// way of [`find_session`], which searches all of the old places.
pub fn sessions_home() -> PathBuf {
    sessions_home_from(session_dir())
}

/// [`sessions_home`] with [`session_dir`]'s answer supplied.
///
/// `None` — an environment naming no home directory at all — is the one case
/// with nothing better to offer than a relative `sessions`, which is what
/// every version of this has fallen back to. It is the "there is nowhere to
/// write" branch, not a second rule: an environment that has a home
/// directory has exactly one sessions folder.
pub fn sessions_home_from(sessions: Option<PathBuf>) -> PathBuf {
    sessions.unwrap_or_else(|| PathBuf::from(SESSIONS_DIR))
}

/// The text the typed-path prompts start the field with — the sessions
/// home, with the separator a name goes after.
pub fn session_prompt_dir() -> String {
    format!("{}{}", sessions_home().display(), std::path::MAIN_SEPARATOR)
}

// ── The file picker's folders ──

/// The folder the session pickers open on, made if it is not there yet.
///
/// [`sessions_home`] as a folder that exists. Made rather than only named: a
/// picker that opens onto a folder the application has never written to
/// would list nothing and say the folder could not be read, which is a true
/// sentence and a useless one.
pub fn session_browse_dir() -> PathBuf {
    browse_dir(sessions_home())
}

/// The folder the sample picker opens on, made if it is not there yet —
/// `<app dir>/samples`, the one a bare name already resolves against.
///
/// No home directory means no folder of ours to make, and the picker opens
/// where the process was started instead: somewhere that exists, which is
/// all a list needs.
pub fn sample_browse_dir() -> PathBuf {
    match samples_dir() {
        Some(dir) => browse_dir(dir),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

/// `wanted`, made if it is missing, and somewhere that exists either way.
///
/// The working directory is the fallback rather than an error: a picker
/// has to open on *something*, and the directory the process was started
/// in is at least a place the player can walk out of.
fn browse_dir(wanted: PathBuf) -> PathBuf {
    if wanted.is_dir() || std::fs::create_dir_all(&wanted).is_ok() {
        return wanted;
    }
    std::env::current_dir().unwrap_or(wanted)
}

// ── Saving ──

/// Where a save lands, given what the player typed.
///
/// The save prompt asks for a *name* — `myjam` — and the name is written
/// into the sessions folder, which is where every other session already is
/// and the only place the open picker will think to look. A value with a
/// separator in it is a path and is taken exactly as typed, which is what
/// saving has always done and what a player who wants a file somewhere
/// specific is entitled to.
pub fn save_target(input: &str) -> PathBuf {
    save_target_in(input, &sessions_home())
}

/// `path` carrying the extension every session file has.
///
/// The rule in one place: a name typed without an extension gets one, and a
/// name typed with the wrong one is corrected — `mysong.txt` saves as
/// `mysong.phos`. The picker needs the same answer the saver reaches, because
/// it is the picker that asks "overwrite?" before the saver ever runs, and a
/// question asked about a different path than the one written is worse than
/// no question.
///
/// The extension already there is compared without case, the way the
/// picker's own listing compares it: `MYJAM.PHOS` is a session file to every
/// other part of this application, and renaming it out from under the player
/// on the way to disk is a surprise nobody asked for.
#[must_use]
pub fn with_session_extension(path: PathBuf) -> PathBuf {
    if is_session_file(&path) {
        return path;
    }
    path.with_extension(SESSION_EXT)
}

/// Whether `path` already ends in the session extension, in any case.
#[must_use]
pub fn is_session_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(SESSION_EXT))
}

/// [`save_target`] with the sessions folder supplied.
pub fn save_target_in(input: &str, dir: &Path) -> PathBuf {
    let typed = Path::new(input);
    // Anything with more than one component has a separator in it, on
    // every platform and including a drive prefix — `Path::components` is
    // what knows where a separator is, rather than this module guessing at
    // one. An empty field is handed back untouched so the caller's own
    // "nothing was typed" check still sees nothing.
    if input.trim().is_empty() || typed.is_absolute() || typed.components().count() > 1 {
        return typed.to_path_buf();
    }
    dir.join(typed)
}

/// Where to look for a session the player named in the open prompt.
///
/// Deliberately the forgiving end of the pair. Saving has one home —
/// [`sessions_home`] — but opening has to find files written by every rule
/// this application has ever had, and by the player's own hand: an absolute
/// path is taken as given, a relative one that exists against the working
/// directory is taken next (that is where it has always resolved, and a
/// checkout full of `sessions/*.phos` must keep opening), and only when
/// neither finds a file does this try the application directory. So
/// `sessions/take3.phos` opens from anywhere, and a session saved before the
/// one-home rule is not stranded by it.
///
/// Saving does not go through this. A save resolves the path exactly as typed,
/// as it always has; it is the *prompt* that starts somewhere deterministic.
/// Making a write depend on which files happen to exist is how a save lands
/// somewhere the player did not ask for.
pub fn find_session(input: &Path) -> PathBuf {
    find_session_in(input, app_dir().as_deref())
}

/// [`find_session`] with the application directory supplied.
pub fn find_session_in(input: &Path, app: Option<&Path>) -> PathBuf {
    // Saving appends `.phos` to whatever was typed; opening owes the
    // player the same forgiveness. A name without the extension tries
    // `.phos` at every step, so `open mysong` finds `mysong.phos` exactly
    // as `save mysong` wrote it.
    let with_ext: Option<PathBuf> =
        (input.extension().is_none()).then(|| input.with_extension(SESSION_EXT));
    let candidates = |p: &Path| -> Vec<PathBuf> {
        let mut v = vec![p.to_path_buf()];
        if let Some(e) = &with_ext {
            v.push(if p == input { e.clone() } else { p.with_extension(SESSION_EXT) });
        }
        v
    };
    for c in candidates(input) {
        if c.is_absolute() || c.exists() {
            return c;
        }
    }
    if input.is_absolute() {
        return input.to_path_buf();
    }
    let Some(app) = app else {
        return input.to_path_buf();
    };
    // `sessions/take3.phos` first, then a bare `take3.phos`.
    for base in [app.to_path_buf(), app.join(SESSIONS_DIR)] {
        for c in candidates(&base.join(input)) {
            if c.exists() {
                return c;
            }
        }
    }
    input.to_path_buf()
}

/// These run on every platform, including the one whose rule they are mostly
/// about, and the assertions are written to mean the same thing on all of
/// them. Two habits make that work:
///
/// * A `PathBuf` expectation is built with the same `join` calls the code
///   uses, never spelled out with a separator in it. `join` inserts the host's
///   separator, so a literal would only match on the host it was typed for.
/// * Where a literal does appear, it is safe because `Path`'s `PartialEq`
///   compares `components()`, and Windows counts `/` and `\` as separators
///   alike — so `x.join("y")` and `"x/y"` are equal there as well as here.
#[cfg(test)]
mod tests {
    use super::*;

    /// An environment with exactly these variables in it and nothing else.
    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            vars.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        }
    }

    /// The paths every macOS and Linux build has written since the beginning.
    /// If this test changes, somebody's presets moved.
    #[test]
    fn unix_resolves_to_the_dot_directory_it_always_has() {
        let vars = env(&[("HOME", "/home/player")]);
        assert_eq!(
            app_dir_in(Convention::Unix, &vars),
            Some(PathBuf::from("/home/player/.phosphor"))
        );
        assert_eq!(
            app_dir_in(Convention::Unix, &vars).map(|d| d.join("presets")),
            Some(PathBuf::from("/home/player/.phosphor/presets"))
        );
    }

    /// `APPDATA` is the first thing Windows offers and the first thing taken.
    ///
    /// Asserted as a `join` rather than against a literal, because `PathBuf`
    /// is host-flavoured: run on Unix, `join` inserts `/`. What this can prove
    /// from any machine is which variable was read and which segments were
    /// appended to it, which is the whole of the rule. Which separator ends up
    /// between them is `std::path`'s business and correct by construction.
    #[test]
    fn windows_resolves_under_appdata() {
        let vars = env(&[
            ("APPDATA", r"C:\Users\player\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\player"),
        ]);
        assert_eq!(
            app_dir_in(Convention::Windows, &vars),
            Some(PathBuf::from(r"C:\Users\player\AppData\Roaming").join("phosphor"))
        );
    }

    /// Service accounts and stripped environments can be missing `APPDATA`
    /// while still having a profile. Rebuilding the roaming path from the
    /// profile lands in the same place `APPDATA` would have named.
    #[test]
    fn windows_falls_back_to_the_user_profile() {
        let vars = env(&[("USERPROFILE", r"C:\Users\player")]);
        assert_eq!(
            app_dir_in(Convention::Windows, &vars),
            Some(
                PathBuf::from(r"C:\Users\player")
                    .join("AppData")
                    .join("Roaming")
                    .join("phosphor")
            )
        );
    }

    /// The defect this module exists for, stated as a test: a Windows
    /// environment has no `HOME`, and the old lookup answered `None` — which
    /// every call site read as "quietly do nothing".
    #[test]
    fn a_windows_environment_without_home_still_resolves() {
        let vars = env(&[
            ("APPDATA", r"C:\Users\player\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\player"),
        ]);
        assert_eq!(vars("HOME"), None, "this test is about HOME being absent");
        assert!(
            app_dir_in(Convention::Windows, &vars).is_some(),
            "presets would be silently discarded"
        );
    }

    /// `HOME` on Windows is a shell's habit, not the platform's, and honouring
    /// it would give one installation two sets of presets depending on how it
    /// was launched.
    #[test]
    fn windows_ignores_home() {
        let vars = env(&[("HOME", "/c/Users/player")]);
        assert_eq!(app_dir_in(Convention::Windows, &vars), None);
    }

    /// The override names the directory itself — no suffix appended — which is
    /// what a portable install and an isolated test both need.
    #[test]
    fn the_override_wins_on_both_platforms() {
        let vars = env(&[
            (OVERRIDE_VAR, "/tmp/scratch-phosphor"),
            ("HOME", "/home/player"),
            ("APPDATA", r"C:\Users\player\AppData\Roaming"),
        ]);
        for convention in [Convention::Unix, Convention::Windows] {
            assert_eq!(
                app_dir_in(convention, &vars),
                Some(PathBuf::from("/tmp/scratch-phosphor")),
                "{convention:?} did not honour {OVERRIDE_VAR}"
            );
        }
    }

    /// A variable set to nothing is not a home directory. `HOME=""` used to
    /// produce the relative path `.phosphor`, which puts presets in whatever
    /// directory the process was launched from.
    #[test]
    fn a_blank_variable_is_not_a_home_directory() {
        for blank in ["", "   "] {
            assert_eq!(app_dir_in(Convention::Unix, env(&[("HOME", blank)])), None);
            assert_eq!(
                app_dir_in(Convention::Windows, env(&[("APPDATA", blank)])),
                None
            );
            assert_eq!(
                app_dir_in(Convention::Unix, env(&[(OVERRIDE_VAR, blank), ("HOME", "/h")])),
                Some(PathBuf::from("/h/.phosphor")),
                "a blank override swallowed the real home directory"
            );
        }
    }

    /// Nothing to go on means nowhere to write, and the caller has to say so.
    #[test]
    fn an_empty_environment_resolves_to_nothing() {
        for convention in [Convention::Unix, Convention::Windows] {
            assert_eq!(app_dir_in(convention, env(&[])), None);
        }
    }

    /// The sessions home is the application's, never the working
    /// directory's.
    #[test]
    fn the_sessions_home_is_the_application_folder() {
        let sessions = PathBuf::from("/home/player/.phosphor").join("sessions");
        assert_eq!(sessions_home_from(Some(sessions.clone())), sessions);
        // No home directory at all: a relative folder is still better than no
        // folder, and it is what this has always fallen back to.
        assert_eq!(sessions_home_from(None), PathBuf::from("sessions"));
    }

    /// A name typed without an extension gets one; a name typed with the
    /// wrong one is corrected.
    #[test]
    fn a_session_file_always_ends_up_with_the_extension() {
        for (typed, wanted) in [
            ("myjam", "myjam.phos"),
            ("myjam.phos", "myjam.phos"),
            ("mysong.txt", "mysong.phos"),
        ] {
            assert_eq!(
                with_session_extension(PathBuf::from(typed)),
                PathBuf::from(wanted),
                "{typed} did not come out as a session file",
            );
        }
        // A dot inside a name is not an extension anybody typed: `.phos` goes
        // on the end of the last component, which is what `Path` calls one.
        assert_eq!(
            with_session_extension(PathBuf::from("take 2.1")),
            PathBuf::from("take 2.phos"),
        );
        // A shouted extension is still the extension — the picker lists it,
        // so the saver must not quietly rename it.
        assert_eq!(
            with_session_extension(PathBuf::from("MYJAM.PHOS")),
            PathBuf::from("MYJAM.PHOS"),
        );
        assert!(is_session_file(Path::new("a.PhOs")));
        assert!(!is_session_file(Path::new("a.wav")));
        assert_eq!(SESSION_DOT_EXT, format!(".{SESSION_EXT}"), "the two spellings drifted");
    }

    /// A bare name is a name, and goes where the sessions go. Anything with
    /// a separator in it is a path and is written exactly there — the
    /// contract every save has had since the beginning.
    #[test]
    fn a_bare_name_saves_into_the_sessions_folder() {
        let sessions = PathBuf::from("/home/player/.phosphor").join("sessions");
        assert_eq!(save_target_in("myjam", &sessions), sessions.join("myjam"));
        assert_eq!(save_target_in("my jam.phos", &sessions), sessions.join("my jam.phos"));

        // A path, however it is spelled, is taken as typed.
        for typed in ["ideas/jam.phos", "./jam.phos", "/tmp/jam.phos"] {
            assert_eq!(
                save_target_in(typed, &sessions),
                PathBuf::from(typed),
                "a typed path was moved into the sessions folder"
            );
        }
        // Nothing typed stays nothing, so the caller's own check still sees
        // an empty field rather than the folder itself.
        assert_eq!(save_target_in("", &sessions), PathBuf::new());
    }

    /// Opening finds the file in the working directory first, then in the
    /// application directory, and hands back what was typed when neither has
    /// it so the failure message names the path the player entered.
    #[test]
    fn opening_falls_back_to_the_application_directory() {
        let root = std::env::temp_dir().join(format!("phosphor-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = root.join("app");
        std::fs::create_dir_all(app.join("sessions")).unwrap();
        std::fs::write(app.join("sessions").join("take3.phos"), "{}").unwrap();

        // `sessions/take3.phos`, typed from a directory that has no `sessions`.
        assert_eq!(
            find_session_in(Path::new("sessions/take3.phos"), Some(&app)),
            app.join("sessions").join("take3.phos")
        );
        // A bare name finds it too.
        assert_eq!(
            find_session_in(Path::new("take3.phos"), Some(&app)),
            app.join("sessions").join("take3.phos")
        );
        // Nothing anywhere: unchanged, so the error names what was typed.
        assert_eq!(
            find_session_in(Path::new("nowhere.phos"), Some(&app)),
            PathBuf::from("nowhere.phos")
        );
        // An absolute path is never rewritten, even when it does not exist.
        let absolute = root.join("elsewhere.phos");
        assert_eq!(find_session_in(&absolute, Some(&app)), absolute);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file in the working directory wins over one of the same name in the
    /// application directory — the relative path a checkout types has to keep
    /// meaning the checkout's own file.
    #[test]
    fn the_working_directory_wins_over_the_application_directory() {
        let root = std::env::temp_dir().join(format!("phosphor-paths-cwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = root.join("app");
        std::fs::create_dir_all(app.join("sessions")).unwrap();
        std::fs::write(app.join("sessions").join("Cargo.toml"), "{}").unwrap();

        // Under the lock, because this reads a relative path and another test
        // in this module walks the process's working directory about.
        with_process(&[], None, || {
            // Cargo runs a test with the package root as the working directory.
            assert!(
                Path::new("Cargo.toml").exists(),
                "this test needs a file in the working directory"
            );
            // `Cargo.toml` exists relative to this crate's working directory,
            // and the same name exists in the application directory. The local
            // one is the answer.
            assert_eq!(
                find_session_in(Path::new("Cargo.toml"), Some(&app)),
                PathBuf::from("Cargo.toml")
            );
        });

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── The wiring, against the real process environment ──
    //
    // The tests above prove the rule. These prove `app_dir` is actually
    // wired to it: a correct rule reached through the wrong variable is the
    // defect this module was written to fix.
    //
    // The environment and the working directory are both process-wide, and
    // the test binary runs its tests in threads of one process — so a test
    // that changes either has to have the process to itself for as long as it
    // is looking. That is what the lock below is for, and why a test that
    // only *reads* a relative path takes it too.

    static PROCESS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `body` with `vars` applied to the real environment and everything
    /// else this module reads removed, then put the environment back.
    fn with_env(vars: &[(&str, &str)], body: impl FnOnce()) {
        with_process(vars, None, body);
    }

    /// Run `body` with the whole process to itself: `vars` in the
    /// environment, everything else this module reads removed, and the
    /// working directory at `cwd` when one is named. Both are put back
    /// afterwards, panic or not.
    fn with_process(vars: &[(&str, &str)], cwd: Option<&Path>, body: impl FnOnce()) {
        let guard = PROCESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        const KEYS: [&str; 4] = [OVERRIDE_VAR, "HOME", "APPDATA", "USERPROFILE"];
        let saved: Vec<(&str, Option<String>)> =
            KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        let here = std::env::current_dir().ok();

        for key in KEYS {
            std::env::remove_var(key);
        }
        for (key, value) in vars {
            std::env::set_var(key, value);
        }
        if let Some(cwd) = cwd {
            std::env::set_current_dir(cwd).expect("the test's own working directory");
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));

        if let Some(here) = here {
            let _ = std::env::set_current_dir(here);
        }
        for (key, value) in saved {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        drop(guard);
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    /// **The defect that lost `911.phos`, as a test.**
    ///
    /// The sessions home used to prefer a `sessions` directory in the working
    /// directory. Launch from a checkout and it answered one folder; launch
    /// from anywhere else and it answered another — and the save and the
    /// picker asked at different moments, from different directories, and got
    /// different answers. The file was written exactly where the save put it
    /// and was not in the list the player was shown.
    ///
    /// So: the same environment, two working directories, one of which has a
    /// `sessions` folder sitting in it, and every answer must be identical.
    /// All four are asserted together, because it is their *agreement* that
    /// broke rather than any one of them.
    #[test]
    fn the_sessions_home_does_not_move_with_the_working_directory() {
        let root = std::env::temp_dir().join(format!("phosphor-onehome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("app");
        let checkout = root.join("checkout");
        let elsewhere = root.join("elsewhere");
        // A working directory with its own `sessions` folder — a checkout —
        // and one without. This is the whole of the old rule's input.
        std::fs::create_dir_all(checkout.join("sessions")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(&home).unwrap();

        let answers = |from: &Path| -> (PathBuf, String, PathBuf, PathBuf) {
            let mut out = None;
            with_process(&[(OVERRIDE_VAR, &home.to_string_lossy())], Some(from), || {
                out = Some((
                    sessions_home(),
                    session_prompt_dir(),
                    session_browse_dir(),
                    save_target("911"),
                ));
            });
            out.expect("the answers were never taken")
        };

        let (from_checkout, from_elsewhere) = (answers(&checkout), answers(&elsewhere));
        assert_eq!(
            from_checkout, from_elsewhere,
            "the sessions folder moved with the working directory",
        );
        // ...and it is the application's folder, not either of theirs.
        assert_eq!(from_checkout.0, home.join("sessions"));
        assert_eq!(from_checkout.3, home.join("sessions").join("911"));
        assert!(
            !from_checkout.2.starts_with(&checkout),
            "the picker would open on the checkout: {}",
            from_checkout.2.display(),
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real `app_dir` reads the real variables, and the derived
    /// directories hang off it where they always have.
    #[test]
    #[cfg(unix)]
    fn the_process_environment_reaches_app_dir() {
        with_env(&[("HOME", "/home/pinned")], || {
            assert_eq!(app_dir(), Some(PathBuf::from("/home/pinned/.phosphor")));
            assert_eq!(
                preset_dir(),
                Some(PathBuf::from("/home/pinned/.phosphor/presets"))
            );
            assert_eq!(
                session_dir(),
                Some(PathBuf::from("/home/pinned/.phosphor/sessions"))
            );
            assert_eq!(
                crate::preset::default_dir(),
                Some(PathBuf::from("/home/pinned/.phosphor/presets")),
                "the preset bank moved out of ~/.phosphor/presets"
            );
        });
    }

    /// Unset means unset, however this build was compiled: with nothing in the
    /// environment there is nowhere to write, and callers are told so.
    #[test]
    fn an_unset_process_environment_gives_no_directory() {
        with_env(&[], || {
            assert_eq!(app_dir(), None);
            assert_eq!(preset_dir(), None);
            assert_eq!(crate::preset::default_dir(), None);
        });
    }

    /// The override reaches the real lookup too, which is what lets a test or
    /// a portable install point the whole application somewhere else.
    #[test]
    fn the_process_environment_honours_the_override() {
        with_env(&[(OVERRIDE_VAR, "/tmp/pinned-phosphor"), ("HOME", "/home/pinned")], || {
            assert_eq!(app_dir(), Some(PathBuf::from("/tmp/pinned-phosphor")));
        });
    }

    /// This build follows the host's convention.
    #[test]
    fn the_native_convention_matches_the_host() {
        #[cfg(windows)]
        assert_eq!(NATIVE, Convention::Windows);
        #[cfg(not(windows))]
        assert_eq!(NATIVE, Convention::Unix);
    }

    /// Opening owes the same forgiveness saving gives: a name typed
    /// without `.phos` finds the file save wrote with it.
    #[test]
    fn open_forgives_a_missing_extension() {
        let dir = std::env::temp_dir().join(format!("phos_ext_test_{}", std::process::id()));
        let sessions = dir.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("jam.phos"), "{}").unwrap();

        let found = find_session_in(Path::new("sessions/jam"), Some(&dir));
        assert_eq!(found, sessions.join("jam.phos"), "the bare name missed the file");
        let found = find_session_in(Path::new("jam"), Some(&dir));
        assert_eq!(found, sessions.join("jam.phos"), "the bare basename missed it too");
        // An exact name still wins over the extension guess.
        std::fs::write(sessions.join("take"), "{}").unwrap();
        let found = find_session_in(Path::new("sessions/take"), Some(&dir));
        assert_eq!(found, sessions.join("take"), "the literal file lost to the guess");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A sample typed by bare name is found in `<app dir>/samples`, with
    /// the `.wav` guess the session prompt's `.phos` guess taught.
    #[test]
    fn a_bare_sample_name_finds_the_samples_directory() {
        let dir = std::env::temp_dir().join(format!("phos_smp_test_{}", std::process::id()));
        let samples = dir.join("samples");
        std::fs::create_dir_all(&samples).unwrap();
        std::fs::write(samples.join("kick.wav"), b"riff").unwrap();

        let found = find_sample_in(Path::new("kick"), Some(&dir));
        assert_eq!(found, samples.join("kick.wav"), "the bare name missed the kit");
        let found = find_sample_in(Path::new("kick.wav"), Some(&dir));
        assert_eq!(found, samples.join("kick.wav"));
        // A path that resolves nowhere comes back as typed, so the error
        // the loader shows names what the player wrote.
        let found = find_sample_in(Path::new("ghost.wav"), Some(&dir));
        assert_eq!(found, Path::new("ghost.wav"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
