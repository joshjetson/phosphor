//! The crates.io update check.
//!
//! At an interactive launch — and only there, never in a test or a headless
//! build — a background thread asks the crates.io sparse index what the newest
//! published version of `phosphor-studio` is. If it is newer than what is
//! running, a one-line notice appears in the bottom bar telling the player the
//! one command that updates it. Everything about it is designed to cost nothing
//! when it cannot help:
//!
//! * **Offline is the normal case.** Any failure at all — no network, DNS,
//!   timeout, a body that will not parse — is silent. There is no error, no
//!   notice, nothing in the log a player would see. Being offline must feel
//!   exactly like the check not existing.
//! * **At most once every six hours.** The last check's time and result are
//!   cached under the application directory; inside the window the cached answer
//!   is used and the network is not touched.
//! * **Off entirely** when `PHOSPHOR_NO_UPDATE_CHECK` is set to anything, and
//!   never spawned outside the real launch path.
//!
//! The TLS stack is pure Rust on purpose (`minreq` + rustls + ring, no
//! `openssl-sys`): a system-OpenSSL dependency would break the one-command
//! `cargo install phosphor-studio --locked` on a machine without it, which is a
//! property this project fought for. A test pins that the tree stays that way.
//!
//! The comparison and the index parse are pure and tested here; the fetch, the
//! cache file, and the thread are the IO seam around them and are never reached
//! by a test.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use phosphor_app::version;

/// The shared slot the background thread writes and the UI reads. `Some` holds
/// the newer version string; `None` means "no news", which is almost always.
///
/// A `Mutex` and not an atomic because the payload is a `String`; the UI takes
/// it with a non-blocking `try_lock` once a frame, so a contended lock costs a
/// frame's delay in showing a notice and never a stall.
pub type UpdateNotice = Arc<Mutex<Option<String>>>;

/// Setting this to anything turns the check off completely.
pub const DISABLE_VAR: &str = "PHOSPHOR_NO_UPDATE_CHECK";

/// The sparse-index path for `phosphor-studio`. The index shards by name
/// length: three-or-more-letter names live under `first-two/second-two/name`.
const INDEX_URL: &str = "https://index.crates.io/ph/os/phosphor-studio";

/// The whole check, connect and read, gets this many seconds and no more.
const TIMEOUT_SECS: u64 = 3;

/// The network is touched at most this often. Six hours: often enough that a
/// player learns of an update the same day, rare enough to be invisible.
const CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

/// The cache file under the application directory.
const CACHE_FILE: &str = "update_check.json";

/// The one command that updates the app — shown in the notice, and the reason
/// the pure-Rust TLS stack matters: this must work on a bare machine.
pub const INSTALL_COMMAND: &str = "cargo install phosphor-studio --locked";

/// The bottom-bar notice for a given newer version.
#[must_use]
pub fn notice_line(newer: &str) -> String {
    format!("v{newer} available \u{00b7} {INSTALL_COMMAND}")
}

// ── The pure seam ──

/// `latest` if it is strictly newer than `running`, else `None`.
///
/// Equal, older, and either side unparseable all answer `None`: there is only
/// news when crates.io is genuinely ahead of what is running.
#[must_use]
pub fn newer_version(latest: &str, running: &str) -> Option<String> {
    version::is_newer(latest, running).then(|| latest.trim().to_string())
}

/// The newest non-yanked version named in a sparse-index body.
///
/// The index is newline-delimited JSON, one object per published version in
/// ascending order, so the answer is the last line that parses, carries a
/// `vers`, and is not yanked. A line that will not parse is skipped rather than
/// fatal — one bad record must not throw away a good index — and an index with
/// no usable line answers `None`, which the caller treats as no news.
#[must_use]
pub fn parse_latest_from_index(body: &str) -> Option<String> {
    let mut latest = None;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("yanked").and_then(serde_json::Value::as_bool).unwrap_or(false) {
            continue;
        }
        if let Some(vers) = value.get("vers").and_then(serde_json::Value::as_str) {
            latest = Some(vers.to_string());
        }
    }
    latest
}

/// Whether a cache entry is still inside the six-hour window as of `now`.
///
/// A clock that has gone backwards since the check — `now` earlier than the
/// stamp — is treated as fresh rather than as a reason to hammer the network:
/// `saturating_sub` floors the age at zero.
#[must_use]
fn cache_is_fresh(cache: &Cache, now: u64) -> bool {
    now.saturating_sub(cache.checked_at) < CHECK_INTERVAL_SECS
}

// ── The IO seam (never reached by a test) ──

/// The cached result of the last check.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cache {
    /// Unix seconds when the check was made.
    checked_at: u64,
    /// The latest version the index reported then, or `None` if the check
    /// failed. Cached either way so a failed check does not retry for the whole
    /// window — an offline launch stays offline-cheap.
    latest: Option<String>,
}

/// Seconds since the Unix epoch, or 0 if the clock is before it.
fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Start the check on a background thread, if it is allowed to run at all.
///
/// Called once from the real launch path. Returns immediately; the audio and
/// MIDI threads are already up and the UI loop is about to start, and none of
/// them wait on this. A spawn that fails is swallowed like every other failure
/// here — a machine that cannot start a thread does not need to be told.
pub fn spawn(notice: UpdateNotice, running: String) {
    if std::env::var_os(DISABLE_VAR).is_some() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("phosphor-update-check".into())
        .spawn(move || {
            if let Some(latest) = resolve_latest() {
                if let Some(newer) = newer_version(&latest, &running) {
                    if let Ok(mut slot) = notice.lock() {
                        *slot = Some(newer);
                    }
                }
            }
        });
}

/// The latest version, from the cache when it is fresh and from the network
/// otherwise. Every path is silent on failure.
fn resolve_latest() -> Option<String> {
    let now = unix_now();
    if let Some(cache) = read_cache() {
        if cache_is_fresh(&cache, now) {
            return cache.latest;
        }
    }
    let latest = fetch_latest();
    // Record the attempt whether or not it found anything, so a failed check
    // does not retry until the window is up.
    write_cache(&Cache { checked_at: now, latest: latest.clone() });
    latest
}

/// GET the index with the timeout and parse the newest version out of it.
/// `None` on any failure — a non-200, a read error, an unparseable body.
fn fetch_latest() -> Option<String> {
    let response = minreq::get(INDEX_URL)
        .with_timeout(TIMEOUT_SECS)
        .send()
        .ok()?;
    if response.status_code != 200 {
        return None;
    }
    parse_latest_from_index(response.as_str().ok()?)
}

/// Read the cache, or `None` when it is absent, unreadable, or malformed.
fn read_cache() -> Option<Cache> {
    let dir = phosphor_app::paths::app_dir()?;
    let text = std::fs::read_to_string(dir.join(CACHE_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write the cache, best-effort. A home that cannot be written just means the
/// next launch checks again — no worse than not caching at all.
fn write_cache(cache: &Cache) {
    if let Some(dir) = phosphor_app::paths::app_dir() {
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = serde_json::to_string_pretty(cache) {
            let _ = std::fs::write(dir.join(CACHE_FILE), text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_remote_is_news() {
        assert_eq!(newer_version("0.3.86", "0.3.85"), Some("0.3.86".to_string()));
        assert_eq!(newer_version("0.4.0", "0.3.85"), Some("0.4.0".to_string()));
    }

    #[test]
    fn an_equal_or_older_remote_is_not_news() {
        assert_eq!(newer_version("0.3.85", "0.3.85"), None);
        assert_eq!(newer_version("0.3.84", "0.3.85"), None);
    }

    #[test]
    fn a_malformed_version_is_never_news() {
        assert_eq!(newer_version("garbage", "0.3.85"), None);
        assert_eq!(newer_version("0.3.86", "garbage"), None);
        assert_eq!(newer_version("", "0.3.85"), None);
    }

    #[test]
    fn the_index_parse_takes_the_last_non_yanked_version() {
        // A real-shaped sparse index: ascending versions, the newest last.
        let body = "\
{\"name\":\"phosphor-studio\",\"vers\":\"0.3.83\",\"yanked\":false}
{\"name\":\"phosphor-studio\",\"vers\":\"0.3.84\",\"yanked\":false}
{\"name\":\"phosphor-studio\",\"vers\":\"0.3.85\",\"yanked\":false}
";
        assert_eq!(parse_latest_from_index(body).as_deref(), Some("0.3.85"));
    }

    #[test]
    fn a_yanked_newest_version_is_skipped() {
        // The newest line is yanked, so the answer is the newest that is not.
        let body = "\
{\"vers\":\"0.3.84\",\"yanked\":false}
{\"vers\":\"0.3.85\",\"yanked\":false}
{\"vers\":\"0.3.86\",\"yanked\":true}
";
        assert_eq!(parse_latest_from_index(body).as_deref(), Some("0.3.85"));
    }

    #[test]
    fn a_malformed_line_is_skipped_not_fatal() {
        let body = "\
{\"vers\":\"0.3.84\",\"yanked\":false}
this is not json
{\"vers\":\"0.3.85\",\"yanked\":false}
";
        assert_eq!(parse_latest_from_index(body).as_deref(), Some("0.3.85"));
    }

    #[test]
    fn an_empty_or_all_yanked_index_is_no_news() {
        assert_eq!(parse_latest_from_index(""), None);
        assert_eq!(
            parse_latest_from_index("{\"vers\":\"0.3.85\",\"yanked\":true}\n"),
            None
        );
    }

    #[test]
    fn the_notice_names_the_version_and_the_one_command() {
        let line = notice_line("0.3.86");
        assert!(line.contains("v0.3.86"));
        assert!(line.contains("cargo install phosphor-studio --locked"));
    }

    #[test]
    fn a_check_stays_fresh_for_six_hours_then_expires() {
        let cache = Cache { checked_at: 1_000, latest: Some("0.3.86".into()) };
        assert!(cache_is_fresh(&cache, 1_000), "the same instant is fresh");
        assert!(cache_is_fresh(&cache, 1_000 + CHECK_INTERVAL_SECS - 1), "still inside the window");
        assert!(!cache_is_fresh(&cache, 1_000 + CHECK_INTERVAL_SECS), "the window has closed");
    }

    #[test]
    fn a_clock_that_went_backwards_counts_as_fresh() {
        // now < checked_at must not read as an enormous age and hammer the net.
        let cache = Cache { checked_at: 10_000, latest: None };
        assert!(cache_is_fresh(&cache, 5_000));
    }
}
