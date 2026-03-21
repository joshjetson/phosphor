//! Debug event logger for test-driven development.
//!
//! Logs every user action and system response to a file
//! so we can trace exactly what happened vs what should have happened.
//!
//! Enable with: PHOSPHOR_DEBUG=1 cargo run
//! Logs to: phosphor_debug.log (in current directory)

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

static LOGGER: Mutex<Option<DebugLogger>> = Mutex::new(None);

struct DebugLogger {
    file: File,
    start: Instant,
}

/// Initialize the debug logger. Call once at startup.
/// Only creates the log file if PHOSPHOR_DEBUG=1 is set.
pub fn init() {
    if std::env::var("PHOSPHOR_DEBUG").unwrap_or_default() != "1" {
        return;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open("phosphor_debug.log")
        .expect("Failed to create phosphor_debug.log");

    let mut logger = LOGGER.lock().unwrap();
    *logger = Some(DebugLogger {
        file,
        start: Instant::now(),
    });

    drop(logger);
    log("INIT", "Debug logging started");
}

/// Log an event with a category and message.
pub fn log(category: &str, msg: &str) {
    let mut guard = LOGGER.lock().unwrap();
    if let Some(ref mut logger) = *guard {
        let elapsed = logger.start.elapsed();
        let secs = elapsed.as_secs_f64();
        let _ = writeln!(logger.file, "[{secs:>10.3}] {category:<12} {msg}");
        let _ = logger.file.flush();
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
