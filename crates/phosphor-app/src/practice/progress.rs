//! Saved practice progress: one JSON file, one record per exercise
//! variant, keyed by the exercise's stable id.
//!
//! The number that matters is `clean_bpm` — the fastest tempo the player
//! has passed the exercise at. It is the iReal insight made durable:
//! "keep going until you can no longer keep up; that tempo is your
//! current limit" — except here the limit is earned by clean reps, not
//! guessed at, and it is the progress bar.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExerciseRecord {
    /// The fastest BPM passed clean (3 reps in a row).
    pub clean_bpm: u32,
    /// Total clean reps ever, for the practice log's sense of history.
    pub clean_reps: u32,
    pub attempts: u32,
}

pub type Progress = HashMap<String, ExerciseRecord>;

fn path() -> Option<PathBuf> {
    crate::paths::app_dir().map(|d| d.join("practice.json"))
}

/// Load. Missing file = fresh start; corrupt file = left on disk, fresh
/// start reported, nothing destroyed.
#[must_use]
pub fn load() -> Progress {
    let Some(path) = path() else { return Progress::new() };
    let Ok(text) = std::fs::read_to_string(&path) else { return Progress::new() };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(progress: &Progress) -> std::io::Result<()> {
    let Some(path) = path() else {
        return Err(std::io::Error::other("no home directory"));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(progress)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let mut p = Progress::new();
        p.insert(
            "maj_scale:C:rh".into(),
            ExerciseRecord { clean_bpm: 84, clean_reps: 12, attempts: 30 },
        );
        let json = serde_json::to_string(&p).unwrap();
        let back: Progress = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
