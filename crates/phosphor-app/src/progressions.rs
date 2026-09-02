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

/// One chord as it is stored — in the library file and in sessions.
///
/// Untagged: the `Tuple` form is what v0.3.59 wrote, three numbers; the
/// `Full` form carries a learned voicing's own intervals. Old files load
/// as tuples; every new save writes the full form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StoredChord {
    Full {
        root: i8,
        quality: u8,
        bass: i8,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        custom: Vec<i8>,
    },
    Tuple((i8, u8, i8)),
}

impl StoredChord {
    #[must_use]
    pub fn to_wire(&self) -> UserChord {
        match self {
            Self::Tuple((root, quality, bass)) => UserChord::pick(*root, *quality, *bass),
            Self::Full { root, quality, bass, custom } => {
                if *quality == phosphor_core::midi_fx::LEARNED_QUALITY {
                    UserChord::learned(*root, custom, *bass)
                } else {
                    UserChord::pick(*root, *quality, *bass)
                }
            }
        }
    }

    #[must_use]
    pub fn from_wire(chord: &UserChord) -> Self {
        Self::Full {
            root: chord.root,
            quality: chord.quality,
            bass: chord.bass,
            custom: chord.custom[..usize::from(chord.custom_len)].to_vec(),
        }
    }
}

/// One saved progression: a name and its chords, in stored form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserProgression {
    pub name: String,
    pub chords: Vec<StoredChord>,
}

impl UserProgression {
    #[must_use]
    pub fn wire_chords(&self) -> Vec<UserChord> {
        self.chords.iter().map(StoredChord::to_wire).collect()
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
        let entry = UserProgression {
            name: "mine".into(),
            chords: vec![
                StoredChord::from_wire(&UserChord::pick(9, 5, -1)),
                StoredChord::from_wire(&UserChord::learned(5, &[0, 11, 16], 9)),
            ],
        };
        let json = serde_json::to_string(&[entry.clone()]).unwrap();
        let back: Vec<UserProgression> = serde_json::from_str(&json).unwrap();
        assert_eq!(back[0], entry);
        let wires = back[0].wire_chords();
        assert_eq!(wires[1].bass, 9);
        assert_eq!(wires[1].custom_len, 3, "the learned shape was lost in the file");

        // A v0.3.59 library file — bare triples — still loads.
        let old_json = r#"[{"name":"old","chords":[[9,5,-1],[5,1,9]]}]"#;
        let old: Vec<UserProgression> = serde_json::from_str(old_json).unwrap();
        assert_eq!(old[0].wire_chords().len(), 2);
        assert_eq!(old[0].wire_chords()[0].root, 9);

        let mut lib = vec![entry.clone()];
        let replaced = upsert(
            &mut lib,
            UserProgression { name: "mine".into(), chords: vec![StoredChord::Tuple((0, 0, -1))] },
        );
        assert!(replaced);
        assert_eq!(lib.len(), 1);
        let replaced = upsert(
            &mut lib,
            UserProgression { name: "other".into(), chords: vec![StoredChord::Tuple((2, 5, -1))] },
        );
        assert!(!replaced);
        assert_eq!(lib.len(), 2);
    }
}
