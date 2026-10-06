//! The player's own drum kits: a sampler's pads, saved under a name, that a
//! Drum Rack track picks on its kit knob after the eighteen machines.
//!
//! # On disk
//!
//! `<app dir>/kits/<name>.json` holds the pads the way a session holds a
//! sampler track's ([`SessionSampler`]), and `<name>.samples/` beside it
//! holds every sound the pads play. *Every* sound, not only the recorded
//! takes: a kit is something a player keeps for months, and one that named
//! `~/Downloads/kick.wav` would go quiet the day that folder was tidied. So a
//! save treats each layer with audio behind it as a take and writes it out
//! through the sidecar machinery sessions already use ([`sidecar::write_takes`]
//! and [`sidecar::prune_takes`]) — the same file names, the same "written
//! once", the same caution about which files are ours to sweep.
//!
//! # In memory
//!
//! A loaded kit is a [`UserKit`]: its name, the pads with every layer's path
//! made absolute into the kit's folder and marked as a take, and an `id`.
//! Marked as takes because that is what makes a session that uses the kit
//! copy the audio into its *own* sidecar when it is saved — a song has to keep
//! sounding after the kit it was made with is edited or deleted. The `id` is
//! identity for the front end: a new one is handed out every time a kit's
//! content is loaded or saved, so "is the engine playing this kit?" is one
//! integer comparison rather than a walk over eighty-eight pads.
//!
//! # Not undoable
//!
//! Saving and deleting a kit are file operations, like saving and deleting a
//! preset, and stay off the undo stack. Choosing one on a Drum Rack is an
//! edit, and is undoable like any other turn of the kit knob.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use phosphor_dsp::drum_rack::KIT_COUNT;

use crate::sampler::session::SessionSampler;
use crate::sampler::{sidecar, wav::WavCache, LayerSource, SamplerState};

/// Current kit file format version. Written into every file so that a later
/// format can tell an old kit from a damaged one.
pub const KIT_VERSION: u32 = 1;

/// The longest name a kit can have — a preset's limit, for a preset's
/// reason: it has to fit where the kit knob prints it.
pub const MAX_NAME_LEN: usize = crate::preset::MAX_NAME_LEN;

/// One kit, ready to play: what the kit knob lists and a Drum Rack track
/// holds.
#[derive(Debug, Clone)]
pub struct UserKit {
    pub name: String,
    /// Which loading or saving of this kit this is — see the module notes.
    pub id: u64,
    pub state: SamplerState,
}

impl UserKit {
    /// A kit with a fresh identity.
    #[must_use]
    pub fn new(name: impl Into<String>, state: SamplerState) -> Self {
        Self { name: name.into(), id: next_id(), state }
    }

    /// The same kit whose sounds now live somewhere else — a session's
    /// sidecar after a save. The identity is kept, because nothing the engine
    /// plays has changed and resending eighty-eight pads to say so would cut
    /// whatever is ringing.
    #[must_use]
    pub fn moved(&self, state: SamplerState) -> Self {
        Self { name: self.name.clone(), id: self.id, state }
    }
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, AtomicOrdering::Relaxed)
}

/// A kit as a session carries it: its name, and its pads in the session's
/// own sampler format. The session keeps its own copy so that editing or
/// deleting the kit later never changes a song that was made with it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionKit {
    pub name: String,
    pub sampler: SessionSampler,
}

impl SessionKit {
    #[must_use]
    pub fn from_kit(kit: &UserKit) -> Self {
        Self { name: kit.name.clone(), sampler: SessionSampler::from_state(&kit.state) }
    }
}

/// The file a kit is written as.
#[derive(Serialize, Deserialize)]
struct KitFile {
    name: String,
    version: u32,
    sampler: SessionSampler,
}

/// Enough of a kit file to say whose it is, without decoding its pads.
#[derive(Deserialize)]
struct KitHeader {
    name: String,
}

// ── Names ──

/// A name the player typed, made ready to store — or the sentence that says
/// why it cannot be.
pub fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a drum kit needs a name".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!("a drum kit name is {MAX_NAME_LEN} characters at most"));
    }
    Ok(name.to_string())
}

/// The order kits are listed in: alphabetical regardless of case, and by
/// exact spelling where two names differ only in case. One order for the
/// list, the knob and the search, so the three never disagree.
fn order(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// A file name made from a kit name. Letters, digits, `-` and `_` are kept;
/// a space becomes `-` and anything else `_`, so a name can never reach out
/// of the kits folder or trip a shell. The name itself is kept whole inside
/// the file — this is only where it is kept.
fn safe_stem(name: &str) -> String {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => c,
            ' ' => '-',
            _ => '_',
        })
        .collect();
    if stem.is_empty() { "kit".into() } else { stem }
}

/// Every kit file in `dir`, with the name it says it holds.
fn kit_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json") && p.is_file())
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let header: KitHeader = serde_json::from_str(&text).ok()?;
            Some((p, header.name))
        })
        .collect()
}

/// The file holding the kit called `name`, if there is one.
///
/// Found by the name written inside rather than by working the file name
/// out again: two names can make one file name, and a filesystem that
/// ignores case makes two more of them the same.
fn file_of(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    kit_files(dir).into_iter().find(|(_, n)| n == name).map(|(p, _)| p)
}

/// A file name for a new kit that nothing in `dir` is using — neither a kit
/// file nor its samples folder.
fn free_file(dir: &Path, name: &str) -> PathBuf {
    let stem = safe_stem(name);
    let taken = |path: &Path| path.exists() || sidecar::sidecar_dir(path).exists();
    let mut path = dir.join(format!("{stem}.json"));
    let mut n = 2;
    while taken(&path) && n < 1000 {
        path = dir.join(format!("{stem}-{n}.json"));
        n += 1;
    }
    path
}

/// Whether a kit called `name` is saved in `dir` — what the save asks
/// before it overwrites one.
#[must_use]
pub fn exists(dir: &Path, name: &str) -> bool {
    file_of(dir, name).is_some()
}

/// The kit called `name` in a loaded library.
#[must_use]
pub fn find<'a>(library: &'a [Arc<UserKit>], name: &str) -> Option<&'a Arc<UserKit>> {
    let name = name.trim();
    library.iter().find(|k| k.name == name)
}

/// Whether a sampler has anything on it a kit could keep: a sound or a
/// phrase on any pad or zone. Settings alone are not a kit.
#[must_use]
pub fn has_sounds(state: &SamplerState) -> bool {
    state
        .pads
        .iter()
        .chain(state.zones.iter().map(|z| &z.pad))
        .any(|p| !p.layers.is_empty() || !p.phrases.is_empty())
}

// ── Loading ──

/// Every kit in `dir`, in list order. A kit that will not read is left out
/// with a warning in the log — one damaged file must not take the whole
/// library, or the application, with it.
#[must_use]
pub fn load_all(dir: &Path) -> Vec<Arc<UserKit>> {
    let mut kits: Vec<Arc<UserKit>> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return kits };
    for path in entries.flatten().map(|e| e.path()) {
        if !(path.extension().is_some_and(|x| x == "json") && path.is_file()) {
            continue;
        }
        match load(&path) {
            Ok(kit) => kits.push(Arc::new(kit)),
            Err(message) => tracing::warn!("drum kit not loaded: {message}"),
        }
    }
    kits.sort_by(|a, b| order(&a.name, &b.name));
    kits
}

/// One kit file, decoded.
///
/// A sound whose file has gone keeps its seat with no audio behind it, the
/// way a session's does. Every layer comes back absolute and marked as a
/// take — see the module notes for why.
pub fn load(path: &Path) -> Result<UserKit, String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    let file: KitFile = serde_json::from_str(&text).map_err(|e| format!("{shown}: {e}"))?;
    let name = check_name(&file.name).map_err(|e| format!("{shown}: {e}"))?;
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let mut wavs = WavCache::new();
    let mut state = file.sampler.into_state(|stored| {
        let resolved = dir.join(stored);
        match wavs.load(&resolved) {
            Ok(pcm) => Some(pcm),
            Err(message) => {
                tracing::warn!("drum kit '{name}': sample not loaded — {message}");
                None
            }
        }
    });
    for layer in state.all_layers_mut() {
        layer.path = dir.join(&layer.path);
        layer.source = LayerSource::Take;
    }
    Ok(UserKit::new(name, state))
}

// ── Saving ──

/// Save a sampler's pads as the kit called `name`, replacing a kit of that
/// name if there is one, and answer the kit as it now plays.
///
/// The sounds go first and the file after, the session's order for the
/// session's reason: a kit naming audio that is not there is worse than a
/// stray file in a folder.
pub fn save(dir: &Path, name: &str, state: &SamplerState) -> Result<UserKit, String> {
    let name = check_name(name)?;
    if !has_sounds(state) {
        return Err("there is nothing on the pads to save".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = file_of(dir, &name).unwrap_or_else(|| free_file(dir, &name));
    let samples = sidecar::sidecar_dir(&path);

    let mut kit = state.clone();
    // Every sound with audio behind it becomes a take this kit has to write
    // — except the ones already sitting in this kit's own folder, which are
    // given back the relative name the sidecar knows them by so they are not
    // written a second time. A path anywhere else is cleared: the kit keeps
    // its own copy, and a relative path that happens to look like one of its
    // files must not be mistaken for one.
    for layer in kit.all_layers_mut() {
        if layer.pcm.is_none() {
            continue;
        }
        let ours = layer
            .path
            .strip_prefix(&samples)
            .ok()
            .and_then(|rest| samples.file_name().map(|home| Path::new(home).join(rest)));
        layer.path = ours.unwrap_or_default();
        layer.source = LayerSource::Take;
    }
    sidecar::write_takes(&path, &mut kit)?;
    sidecar::prune_takes(&path, [&kit]);

    let file = KitFile {
        name: name.clone(),
        version: KIT_VERSION,
        sampler: SessionSampler::from_state(&kit),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;

    // The kit as a load would give it back, without decoding a thing: the
    // audio is the audio that was just written.
    for layer in kit.all_layers_mut() {
        if layer.path.is_relative() {
            layer.path = dir.join(&layer.path);
        }
    }
    Ok(UserKit::new(name, kit))
}

/// Delete the kit called `name`: its file, and the sounds in its folder.
///
/// Only the sound files this module could have written are removed — the
/// sidecar's own rule ([`sidecar::prune_takes`] with nothing kept) — and the
/// folder goes only once it is empty, so a file the player put in there by
/// hand survives. Answers whether there was a kit to delete.
pub fn delete(dir: &Path, name: &str) -> Result<bool, String> {
    let Some(path) = file_of(dir, name) else { return Ok(false) };
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    sidecar::prune_takes(&path, std::iter::empty());
    let _ = std::fs::remove_dir(sidecar::sidecar_dir(&path));
    Ok(true)
}

// ── The kit knob ──

/// Where a step of the kit knob lands.
#[derive(Debug, Clone)]
pub enum KitChoice {
    /// One of the machines, by its place on the knob.
    BuiltIn(usize),
    User(Arc<UserKit>),
}

/// One step of the kit knob, through the machines and on into the player's
/// kits, or `None` at either end.
///
/// `builtin` is where the knob's own position says, and `current` the user
/// kit the track is playing, if it is — which wins, since a user kit is
/// chosen *past* the machines. The current kit is placed in the list by its
/// name rather than found by identity, so a session's own copy of a kit
/// stands where the library's version of it does, and one whose name is no
/// longer in the library steps to its neighbours.
#[must_use]
pub fn step(
    library: &[Arc<UserKit>],
    builtin: usize,
    current: Option<&UserKit>,
    up: bool,
) -> Option<KitChoice> {
    let last = KIT_COUNT - 1;
    let Some(kit) = current else {
        return if up {
            if builtin < last {
                Some(KitChoice::BuiltIn(builtin + 1))
            } else {
                library.first().cloned().map(KitChoice::User)
            }
        } else {
            builtin.checked_sub(1).map(KitChoice::BuiltIn)
        };
    };
    let at = library.partition_point(|k| order(&k.name, &kit.name) == Ordering::Less);
    if up {
        let listed = library.get(at).is_some_and(|k| k.name == kit.name);
        library.get(at + usize::from(listed)).cloned().map(KitChoice::User)
    } else {
        Some(match at.checked_sub(1) {
            Some(before) => KitChoice::User(Arc::clone(&library[before])),
            None => KitChoice::BuiltIn(last),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_plugin::sample::SamplePcm;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phosphor-kits-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn pcm(frames: usize, value: f32) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![value; frames], channels: 1, sample_rate: 44_100.0 })
    }

    /// A sampler with a "file" layer on C2 whose wav lives outside any kit,
    /// as a pad loaded from the samples folder does.
    fn sampler_with_a_kick(outside: &Path) -> SamplerState {
        std::fs::create_dir_all(outside).unwrap();
        let wav = outside.join("kick.wav");
        let audio = pcm(100, 0.25);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for &s in &audio.data {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        let mut state = SamplerState::new();
        let pad = SamplerState::pad_of_note(36).unwrap();
        state.add_wav_layer(pad, wav, audio).unwrap();
        state.pads[pad].config.choke = 2;
        state
    }

    /// A kit is self-contained: the file layer is copied into the kit's own
    /// folder, so deleting the original changes nothing, and it reads back
    /// with its settings, absolute paths into that folder, and marked as
    /// takes.
    #[test]
    fn a_saved_kit_carries_its_own_sounds_and_reads_back() {
        let root = scratch("roundtrip");
        let dir = root.join("kits");
        let state = sampler_with_a_kick(&root.join("elsewhere"));
        let saved = save(&dir, "  My Kit/1 ", &state).unwrap();
        assert_eq!(saved.name, "My Kit/1");
        std::fs::remove_dir_all(root.join("elsewhere")).unwrap();

        let library = load_all(&dir);
        assert_eq!(library.len(), 1);
        let kit = &library[0];
        assert_eq!(kit.name, "My Kit/1");
        let pad = SamplerState::pad_of_note(36).unwrap();
        assert_eq!(kit.state.pads[pad].config.choke, 2);
        let layer = &kit.state.pads[pad].layers[0];
        assert!(layer.pcm.is_some(), "the kit depended on a file outside its folder");
        assert_eq!(layer.pcm.as_ref().unwrap().data, vec![0.25; 100]);
        assert!(layer.path.is_absolute() && layer.path.starts_with(&dir));
        assert_eq!(layer.source, LayerSource::Take);
        // The file name is safe however odd the kit name is.
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"My-Kit_1.json".to_string()), "{names:?}");
        assert!(names.contains(&"My-Kit_1.samples".to_string()), "{names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Saving a kit again under its own name replaces it, keeps the sounds
    /// it still uses without writing them twice, and sweeps the ones it
    /// does not.
    #[test]
    fn re_saving_a_kit_updates_it_and_sweeps_what_it_dropped() {
        let root = scratch("resave");
        let dir = root.join("kits");
        let mut state = sampler_with_a_kick(&root.join("elsewhere"));
        state.add_wav_layer(40, root.join("snare.wav"), pcm(50, 0.5)).unwrap();
        save(&dir, "beats", &state).unwrap();
        let samples = dir.join("beats.samples");
        assert_eq!(std::fs::read_dir(&samples).unwrap().count(), 2);

        // Edit the loaded kit the way the sampler would, and save it back.
        let mut edited = load_all(&dir)[0].state.clone();
        edited.pads[40].layers.clear();
        let pad = SamplerState::pad_of_note(36).unwrap();
        edited.pads[pad].config.choke = 5;
        let kick_file = edited.pads[pad].layers[0].path.clone();
        save(&dir, "beats", &edited).unwrap();

        let library = load_all(&dir);
        assert_eq!(library.len(), 1, "a re-save made a second kit");
        assert_eq!(library[0].state.pads[pad].config.choke, 5);
        assert!(library[0].state.pads[40].layers.is_empty());
        assert_eq!(library[0].state.pads[pad].layers[0].path, kick_file, "the kick was rewritten");
        assert_eq!(std::fs::read_dir(&samples).unwrap().count(), 1, "the dropped snare stayed");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Deleting takes the kit and the sounds it wrote — and nothing the
    /// player put in its folder by hand.
    #[test]
    fn deleting_a_kit_takes_only_what_it_wrote() {
        let root = scratch("delete");
        let dir = root.join("kits");
        let state = sampler_with_a_kick(&root.join("elsewhere"));
        save(&dir, "gone", &state).unwrap();
        save(&dir, "kept", &state).unwrap();
        assert!(exists(&dir, "gone"));
        std::fs::write(dir.join("gone.samples").join("mine.wav"), b"mine").unwrap();

        assert!(delete(&dir, "gone").unwrap());
        assert!(!exists(&dir, "gone"));
        assert!(!dir.join("gone.json").exists());
        assert!(dir.join("gone.samples/mine.wav").exists(), "the player's own file went");
        assert!(!dir.join("gone.samples/C2-1.wav").exists(), "the kit's sound stayed");
        let left: Vec<String> = load_all(&dir).iter().map(|k| k.name.clone()).collect();
        assert_eq!(left, ["kept"]);
        assert!(!delete(&dir, "gone").unwrap(), "a second delete found something");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A damaged kit file is skipped, not fatal, and the list is in name
    /// order whatever the files are called.
    #[test]
    fn a_damaged_kit_is_skipped_and_the_rest_are_listed_by_name() {
        let root = scratch("damaged");
        let dir = root.join("kits");
        let state = sampler_with_a_kick(&root.join("elsewhere"));
        for name in ["zed", "Alpha", "beta"] {
            save(&dir, name, &state).unwrap();
        }
        std::fs::write(dir.join("broken.json"), b"{ not a kit").unwrap();
        let names: Vec<String> = load_all(&dir).iter().map(|k| k.name.clone()).collect();
        assert_eq!(names, ["Alpha", "beta", "zed"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two names that make one file name are still two kits.
    #[test]
    fn names_that_share_a_file_name_do_not_overwrite_each_other() {
        let root = scratch("collide");
        let dir = root.join("kits");
        let state = sampler_with_a_kick(&root.join("elsewhere"));
        save(&dir, "a/b", &state).unwrap();
        save(&dir, "a?b", &state).unwrap();
        let names: Vec<String> = load_all(&dir).iter().map(|k| k.name.clone()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_sampler_or_a_bad_name_is_refused() {
        let dir = scratch("refuse");
        assert!(save(&dir, "nothing", &SamplerState::new()).is_err());
        let state = {
            let mut s = SamplerState::new();
            s.add_wav_layer(0, "x.wav".into(), pcm(10, 0.1)).unwrap();
            s
        };
        assert!(save(&dir, "   ", &state).is_err());
        assert!(save(&dir, &"x".repeat(MAX_NAME_LEN + 1), &state).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn kit(name: &str) -> Arc<UserKit> {
        Arc::new(UserKit::new(name, SamplerState::new()))
    }

    fn landed(choice: Option<KitChoice>) -> Option<String> {
        choice.map(|c| match c {
            KitChoice::BuiltIn(i) => format!("#{i}"),
            KitChoice::User(k) => k.name.clone(),
        })
    }

    /// The knob walks the machines, on into the player's kits by name, and
    /// back — and stops at both ends.
    #[test]
    fn the_knob_walks_machines_then_kits_and_back() {
        let library = vec![kit("alpha"), kit("beta")];
        let last = KIT_COUNT - 1;
        assert_eq!(landed(step(&library, 3, None, true)), Some("#4".into()));
        assert_eq!(landed(step(&library, last, None, true)), Some("alpha".into()));
        assert_eq!(landed(step(&library, last, Some(&library[0]), true)), Some("beta".into()));
        assert_eq!(landed(step(&library, last, Some(&library[1]), true)), None);
        assert_eq!(landed(step(&library, 0, Some(&library[1]), false)), Some("alpha".into()));
        assert_eq!(landed(step(&library, 0, Some(&library[0]), false)), Some(format!("#{last}")));
        assert_eq!(landed(step(&library, 0, None, false)), None);
        assert_eq!(landed(step(&[], last, None, true)), None, "no kits, nowhere past the end");

        // A kit that is not in the library — a session's own — stands where
        // its name would.
        let orphan = UserKit::new("aardvark", SamplerState::new());
        assert_eq!(landed(step(&library, 0, Some(&orphan), true)), Some("alpha".into()));
        assert_eq!(landed(step(&library, 0, Some(&orphan), false)), Some(format!("#{last}")));
        let late = UserKit::new("zulu", SamplerState::new());
        assert_eq!(landed(step(&library, 0, Some(&late), true)), None);
        assert_eq!(landed(step(&library, 0, Some(&late), false)), Some("beta".into()));
    }
}
