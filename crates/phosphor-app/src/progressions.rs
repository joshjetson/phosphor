//! The user's progression library: named chord progressions, saved in one
//! JSON file in the app directory and loaded whenever the editor opens.
//!
//! The library is a palette, not a dependency — loading a progression into
//! a chord device copies its resolved chords into the track's mirror, and
//! the session stores that copy, so editing the library later never
//! changes a saved song.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use phosphor_core::midi_fx::UserChord;

/// One saved progression: a name and its chords, in wire form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserProgression {
    pub name: String,
    /// (root above song root, quality index, bass pitch class or -1).
    pub chords: Vec<(i8, u8, i8)>,
}

impl UserProgression {
    #[must_use]
    pub fn wire_chords(&self) -> Vec<UserChord> {
        self.chords
            .iter()
            .map(|&(root, quality, bass)| UserChord { root, quality, bass })
            .collect()
    }
}

fn library_path() -> Option<PathBuf> {
    crate::paths::app_dir().map(|d| d.join("progressions.json"))
}

/// Load the library. A missing file is an empty library; a corrupt one is
/// kept on disk untouched and reported as empty, so a bad edit never
/// destroys what was there.
#[must_use]
pub fn load_library() -> Vec<UserProgression> {
    let Some(path) = library_path() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Save the library, creating the app directory if needed.
pub fn save_library(library: &[UserProgression]) -> std::io::Result<()> {
    let Some(path) = library_path() else {
        return Err(std::io::Error::other("no home directory"));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(library)?;
    std::fs::write(path, text)
}

/// Insert or replace by name, returning whether an entry was replaced.
pub fn upsert(library: &mut Vec<UserProgression>, entry: UserProgression) -> bool {
    if let Some(existing) = library.iter_mut().find(|p| p.name == entry.name) {
        *existing = entry;
        true
    } else {
        library.push(entry);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round trip through the serialized form, and upsert-by-name.
    #[test]
    fn the_library_round_trips_and_upserts() {
        let entry = UserProgression { name: "mine".into(), chords: vec![(9, 5, -1), (5, 1, 9)] };
        let json = serde_json::to_string(&[entry.clone()]).unwrap();
        let back: Vec<UserProgression> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0], entry);
        assert_eq!(back[0].wire_chords()[1].bass, 9);

        let mut lib = vec![entry.clone()];
        let replaced = upsert(
            &mut lib,
            UserProgression { name: "mine".into(), chords: vec![(0, 0, -1)] },
        );
        assert!(replaced);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].chords, vec![(0, 0, -1)]);
        let replaced = upsert(
            &mut lib,
            UserProgression { name: "other".into(), chords: vec![(2, 5, -1)] },
        );
        assert!(!replaced);
        assert_eq!(lib.len(), 2);
    }
}
