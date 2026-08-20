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

/// What a save or open prompt has always started with, and still does when
/// the working directory has one.
const LOCAL_SESSIONS: &str = "sessions";

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
    app_dir().map(|dir| dir.join(LOCAL_SESSIONS))
}

// ── Session prompts ──

/// The text a save or open prompt starts the field with.
///
/// `sessions/` when the working directory already has a `sessions` directory.
/// That is a checkout being run from its own root, which is how this has
/// always behaved and what the sessions already on disk are relative to.
///
/// Otherwise the absolute `<app dir>/sessions/`. A bare `sessions/` resolves
/// against wherever the process was started, and a Start Menu shortcut, a
/// Finder alias or a desktop launcher starts it somewhere the player has never
/// looked — so the file is written successfully to a directory nobody will
/// find again.
pub fn session_prompt_dir() -> String {
    session_prompt_dir_from(Path::new(LOCAL_SESSIONS).is_dir(), session_dir())
}

/// [`session_prompt_dir`] with the filesystem answers supplied: whether the
/// working directory has a `sessions` directory, and what [`session_dir`] says.
pub fn session_prompt_dir_from(local_exists: bool, sessions: Option<PathBuf>) -> String {
    // A forward slash even on Windows, which accepts it everywhere a
    // backslash goes, so the string a checkout sees is one string.
    let local = format!("{LOCAL_SESSIONS}/");
    if local_exists {
        return local;
    }
    match sessions {
        Some(dir) => format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR),
        None => local,
    }
}

/// Where to look for a session the player named in the open prompt.
///
/// An absolute path is taken as given, and so is a relative one that exists
/// against the working directory — that is where it has always resolved and a
/// checkout must keep working. Only when neither finds a file does this try
/// the application directory, so `sessions/take3.phos` still opens after the
/// player stops launching from the checkout.
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
    if input.is_absolute() || input.exists() {
        return input.to_path_buf();
    }
    let Some(app) = app else {
        return input.to_path_buf();
    };
    // `sessions/take3.phos` first, then a bare `take3.phos`.
    for base in [app.to_path_buf(), app.join(LOCAL_SESSIONS)] {
        let candidate = base.join(input);
        if candidate.exists() {
            return candidate;
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

    /// A checkout keeps the prompt it has always had. The absolute form only
    /// appears where the relative one would have resolved somewhere arbitrary.
    #[test]
    fn the_prompt_prefers_a_checkouts_own_sessions_directory() {
        let sessions = PathBuf::from("/home/player/.phosphor").join("sessions");
        assert_eq!(session_prompt_dir_from(true, Some(sessions.clone())), "sessions/");
        assert_eq!(
            session_prompt_dir_from(false, Some(sessions.clone())),
            format!("{}{}", sessions.display(), std::path::MAIN_SEPARATOR)
        );
        // No home directory at all: the relative path is still better than an
        // empty prompt, and it is what this did before.
        assert_eq!(session_prompt_dir_from(false, None), "sessions/");
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

        // Cargo runs a test with the package root as the working directory.
        assert!(Path::new("Cargo.toml").exists(), "this test needs a file in the working directory");
        // `Cargo.toml` exists relative to this crate's working directory, and
        // the same name exists in the application directory. The local one is
        // the answer.
        assert_eq!(
            find_session_in(Path::new("Cargo.toml"), Some(&app)),
            PathBuf::from("Cargo.toml")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ── The wiring, against the real process environment ──
    //
    // The tests above prove the rule. These prove `app_dir` is actually
    // wired to it: a correct rule reached through the wrong variable is the
    // defect this module was written to fix.
    //
    // Every reader and writer of the process environment in this crate goes
    // through `std::env`, which serialises them against each other, so the
    // only thing to guard is these tests overwriting each other's setup.

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `body` with `vars` applied to the real environment and everything
    /// else this module reads removed, then put the environment back.
    fn with_env(vars: &[(&str, &str)], body: impl FnOnce()) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        const KEYS: [&str; 4] = [OVERRIDE_VAR, "HOME", "APPDATA", "USERPROFILE"];
        let saved: Vec<(&str, Option<String>)> =
            KEYS.iter().map(|k| (*k, std::env::var(k).ok())).collect();

        for key in KEYS {
            std::env::remove_var(key);
        }
        for (key, value) in vars {
            std::env::set_var(key, value);
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));

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
}
