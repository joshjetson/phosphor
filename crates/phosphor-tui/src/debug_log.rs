//! Debug event logger for test-driven development.
//!
//! Logs every user action and system response to a file
//! so we can trace exactly what happened vs what should have happened.
//!
//! Enable with: PHOSPHOR_DEBUG=1 cargo run
//! Logs to: phosphor_debug.log — see [`log_candidates`] for where.

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

static LOGGER: Mutex<Option<DebugLogger>> = Mutex::new(None);

/// The log file's name, wherever it ends up.
const LOG_NAME: &str = "phosphor_debug.log";

/// How large the log is allowed to get before it starts over.
///
/// It had no ceiling at all and reached a gigabyte. A trace is read from its
/// recent end, so past this the file is emptied and written again from the top
/// rather than rotated: one file, bounded, always holding the last few minutes
/// of whatever went wrong. Not a logging design — a ceiling, until there is
/// one.
const MAX_BYTES: u64 = 8 * 1024 * 1024;

struct DebugLogger {
    file: File,
    start: Instant,
    /// Bytes in the file, so the cap does not need a `metadata` call per line.
    written: u64,
}

impl DebugLogger {
    fn write_line(&mut self, line: &str) {
        let len = line.len() as u64;
        if self.written + len > MAX_BYTES {
            // Truncate *and* seek: `set_len` alone leaves the cursor past the
            // new end, and the next write would leave a hole of that many
            // zero bytes in front of it.
            if self.file.set_len(0).is_err() || self.file.seek(SeekFrom::Start(0)).is_err() {
                return;
            }
            self.written = 0;
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.written += len;
        }
        let _ = self.file.flush();
    }
}

/// Where the log is tried, in order.
///
/// The working directory first, because that is where it has always been
/// written and where anyone debugging goes looking for it. Then the
/// application directory, then the system temp directory — an installed
/// binary is normally launched with a working directory it cannot write to,
/// which is the ordinary case on Windows and a possible one anywhere.
fn log_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(LOG_NAME)];
    if let Some(dir) = phosphor_app::paths::app_dir() {
        candidates.push(dir.join(LOG_NAME));
    }
    candidates.push(std::env::temp_dir().join(LOG_NAME));
    candidates
}

/// Open the first candidate that will take a write.
fn open_log() -> Option<(PathBuf, File)> {
    for path in log_candidates() {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        if let Ok(file) = OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
            return Some((path, file));
        }
    }
    None
}

/// Initialize the debug logger. Call once at startup.
/// Only creates the log file if PHOSPHOR_DEBUG=1 is set.
///
/// A log file is a debugging convenience, so nothing here is fatal: a
/// read-only working directory used to panic on this line before the
/// application had drawn anything, which is a debug flag taking down the
/// program it was set to debug.
pub fn init() {
    if std::env::var("PHOSPHOR_DEBUG").unwrap_or_default() != "1" {
        return;
    }
    let Some((path, file)) = open_log() else {
        // No terminal has been taken over yet, so this is still readable.
        eprintln!("phosphor: PHOSPHOR_DEBUG=1 but no writable place for {LOG_NAME} — not logging");
        return;
    };

    let mut logger = LOGGER.lock().unwrap_or_else(|e| e.into_inner());
    *logger = Some(DebugLogger {
        file,
        start: Instant::now(),
        written: 0,
    });

    drop(logger);
    log("INIT", &format!("Debug logging started: {}", path.display()));
}

/// Log an event with a category and message.
pub fn log(category: &str, msg: &str) {
    let mut guard = LOGGER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut logger) = *guard {
        let secs = logger.start.elapsed().as_secs_f64();
        logger.write_line(&format!("[{secs:>10.3}] {category:<12} {msg}\n"));
    }
}

/// Log a user input action.
pub fn user(action: &str) {
    log("USER", action);
}

/// Log a system response/state change.
pub fn system(response: &str) {
    log("SYSTEM", response);
}

/// Log transport state.
pub fn transport(playing: bool, recording: bool, looping: bool, position: i64, loop_start: i64, loop_end: i64) {
    log("TRANSPORT", &format!(
        "playing={playing} recording={recording} looping={looping} pos={position} loop={loop_start}..{loop_end}"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The log stops at its ceiling instead of growing without bound. It
    /// reached a gigabyte before there was one.
    #[test]
    fn the_log_starts_over_rather_than_growing_forever() {
        let path = std::env::temp_dir().join(format!("phosphor-log-cap-{}", std::process::id()));
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let mut logger = DebugLogger { file, start: Instant::now(), written: 0 };

        // A line big enough that a few hundred of them pass the ceiling.
        let line = format!("{}\n", "x".repeat(64 * 1024));
        let lines = (MAX_BYTES / line.len() as u64) + 8;
        for _ in 0..lines {
            logger.write_line(&line);
        }

        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size <= MAX_BYTES, "the log grew to {size} bytes, past its {MAX_BYTES} ceiling");
        assert!(size > 0, "the log stopped recording instead of starting over");
        assert_eq!(size, logger.written, "the byte count drifted from the file");

        // The bytes past the restart are real content, not a hole of zeroes
        // left by truncating without seeking.
        let contents = std::fs::read(&path).unwrap();
        assert!(!contents.contains(&0), "truncation left a sparse hole in the log");

        let _ = std::fs::remove_file(&path);
    }

    /// The working directory comes first — that is where anyone debugging
    /// looks — with somewhere writable behind it for an installed binary
    /// whose working directory is read-only.
    #[test]
    fn the_log_has_somewhere_to_fall_back_to() {
        let candidates = log_candidates();
        assert_eq!(candidates[0], PathBuf::from(LOG_NAME), "the log left the working directory");
        assert!(candidates.len() >= 2, "the log has nowhere to fall back to");
        assert_eq!(
            candidates.last().unwrap(),
            &std::env::temp_dir().join(LOG_NAME),
            "the last resort is not the temp directory"
        );
    }
}
