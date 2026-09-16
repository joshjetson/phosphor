//! Where a recorded take lives on disk: `<session>.samples/` beside the
//! session file.
//!
//! A wav layer is a reference to a file the player already owns, and the
//! session only has to remember its name. A take exists nowhere but in
//! memory until something writes it down, so a save that skipped this
//! would lose a performance — which is not a deferral, it is data loss.
//!
//! # The rules, and why each one
//!
//! * **Samples before the session.** A session naming a wav that is not
//!   there is a red pad and a puzzled player; an orphan wav beside a
//!   session that does not mention it is a file nobody notices. Write the
//!   audio first and the worst crash leaves the harmless one.
//! * **Relative paths.** The stored path is relative to the session's own
//!   directory, so a project folder copied to another machine — or to
//!   another disk — opens with its takes intact.
//! * **Written once.** A take already sitting in the sidecar keeps its
//!   file: saving a session with forty takes in it must not rewrite forty
//!   WAVs every time. "Already there" is checked against the file the path
//!   names, so a *Save As* to a new name writes fresh copies into the new
//!   sidecar rather than pointing at the old one.
//! * **32-bit float.** The take was rendered in float and is going back
//!   into a float engine; anything else is a conversion nobody asked for.

use std::path::{Path, PathBuf};

use phosphor_plugin::sample::SamplePcm;

use super::{LayerAddr, SamplerState};

/// The directory a session's takes live in: the session file's name
/// without its extension, plus `.samples`, in the same directory. Beside
/// the session rather than inside a shared folder so that deleting a
/// project takes its recordings with it.
#[must_use]
pub fn sidecar_dir(session: &Path) -> PathBuf {
    let stem = session.file_stem().unwrap_or(session.as_os_str());
    let mut name = stem.to_os_string();
    name.push(".samples");
    match session.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

/// Write every take that has no file yet, and give each one its path.
///
/// Returns how many were written. `Err` is a status-bar sentence: the
/// caller must not go on to write the session, because the session would
/// name audio that is not there.
pub fn write_takes(session: &Path, state: &mut SamplerState) -> Result<usize, String> {
    let takes: Vec<LayerAddr> = state.takes().collect();
    if takes.is_empty() {
        return Ok(0);
    }
    let dir = sidecar_dir(session);
    let base = session.parent().unwrap_or_else(|| Path::new(""));
    // What a take's stored path has to start with to count as already
    // written *for this session*. A project carries its own recordings:
    // a session saved under a new name gets its own copies rather than a
    // reference into the old session's folder, which would break the
    // moment either one was deleted.
    let home = sidecar_dir(Path::new(session.file_name().unwrap_or_default()));
    let mut written = 0usize;
    let mut created = false;

    for addr in takes {
        let Some(layer) = state.layer_at(addr) else { continue };
        if layer.path.parent() == Some(home.as_path()) && base.join(&layer.path).exists() {
            continue;
        }
        let Some(pcm) = layer.pcm.clone() else {
            // A take whose audio is gone is a take that was loaded from a
            // sidecar that has since been deleted. It keeps its seat and
            // its path; there is nothing to write.
            continue;
        };
        if !created {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
            created = true;
        }
        // A zone's take is named for the zone's first key, which is a key
        // a pad might also have recorded one on — so the collision counter
        // settles it, exactly as it does for two takes on one pad.
        let file = free_name(&dir, &state.addr_label(addr), addr.layer() + 1);
        write_wav(&dir.join(&file), &pcm)?;
        // Relative to the session, which is the whole point: the project
        // is a directory that can be moved.
        if let Some(layer) = state.layer_at_mut(addr) {
            layer.path = home.join(&file);
        }
        written += 1;
    }
    Ok(written)
}

/// A name inside `dir` that is not taken. The pad and the take's place in
/// its stack name the file, so a sidecar is readable from the outside; a
/// numeric suffix settles the collision when a pad is recorded, cleared
/// and recorded again into a directory that kept the first file.
fn free_name(dir: &Path, pad: &str, take: usize) -> PathBuf {
    // '#' is legal everywhere this runs, but a sharp in a filename reads
    // badly in a shell and in a URL; the keyboard's own 's' is what a
    // sample library would call it.
    let pad = pad.replace('#', "s");
    let mut name = format!("{pad}-{take}.wav");
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("{pad}-{take}-{n}.wav");
        n += 1;
        if n > 999 {
            break;
        }
    }
    PathBuf::from(name)
}

/// Write PCM as a 32-bit float WAV.
fn write_wav(path: &Path, pcm: &SamplePcm) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: pcm.channels.max(1),
        sample_rate: pcm.sample_rate.max(1.0) as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let shown = path.display();
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("{shown}: {e}"))?;
    for &sample in &pcm.data {
        writer.write_sample(sample).map_err(|e| format!("{shown}: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("{shown}: {e}"))
}

/// Where to look for a layer's audio when a session is opened.
///
/// The session's own directory first — that is where a take lives and
/// where a project's own samples folder would be — and then the usual
/// chain, so a bare `kick` still finds `<app dir>/samples/kick.wav`.
#[must_use]
pub fn find_layer_file(session_dir: &Path, stored: &Path) -> PathBuf {
    if !stored.is_absolute() {
        let beside = session_dir.join(stored);
        if beside.exists() {
            return beside;
        }
    }
    crate::paths::find_sample(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::render::RenderedTake;
    use std::sync::Arc;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("phosphor-sidecar-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn take(frames: usize) -> RenderedTake {
        let pcm = Arc::new(SamplePcm {
            data: (0..frames * 2).map(|i| (i % 7) as f32 * 0.1 - 0.3).collect(),
            channels: 2,
            sample_rate: 44_100.0,
        });
        RenderedTake {
            start_frame: 0,
            end_frame: pcm.frames(),
            peak: 0.3,
            root: None,
            pcm,
        }
    }

    #[test]
    fn the_sidecar_sits_beside_the_session_under_its_own_name() {
        assert_eq!(
            sidecar_dir(Path::new("/songs/neon.phos")),
            PathBuf::from("/songs/neon.samples"),
        );
        assert_eq!(sidecar_dir(Path::new("neon.phos")), PathBuf::from("neon.samples"));
    }

    #[test]
    fn a_take_is_written_once_and_named_relatively() {
        let dir = scratch("write");
        let session = dir.join("kit.phos");
        let mut state = SamplerState::new();
        let pad = SamplerState::pad_of_note(60).unwrap();
        state.add_take_layer(pad, &take(100)).unwrap();

        assert_eq!(write_takes(&session, &mut state).unwrap(), 1);
        let stored = state.pads[pad].layers[0].path.clone();
        assert_eq!(stored, PathBuf::from("kit.samples/C3-1.wav"));
        assert!(dir.join(&stored).exists(), "the sidecar wav was not written");
        assert!(!stored.is_absolute(), "the session would name a machine, not a project");

        // A second save writes nothing and keeps the path.
        assert_eq!(write_takes(&session, &mut state).unwrap(), 0);
        assert_eq!(state.pads[pad].layers[0].path, stored);

        // ...but a save under another name writes the take into its own
        // sidecar rather than pointing at the first session's.
        let other = dir.join("kit2.phos");
        assert_eq!(write_takes(&other, &mut state).unwrap(), 1);
        assert_eq!(
            state.pads[pad].layers[0].path,
            PathBuf::from("kit2.samples/C3-1.wav"),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_written_take_reads_back_as_the_same_audio() {
        let dir = scratch("roundtrip");
        let session = dir.join("kit.phos");
        let mut state = SamplerState::new();
        let pad = 0;
        let source = take(64);
        state.add_take_layer(pad, &source).unwrap();
        write_takes(&session, &mut state).unwrap();

        let path = find_layer_file(&dir, &state.pads[pad].layers[0].path);
        let back = super::super::wav::load_wav(&path).unwrap();
        assert_eq!(back.channels, 2);
        assert_eq!(back.sample_rate, 44_100.0);
        assert_eq!(back.data, source.pcm.data, "the take changed on the way to disk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_collision_takes_the_next_number() {
        let dir = scratch("collide");
        std::fs::create_dir_all(dir.join("kit.samples")).unwrap();
        std::fs::write(dir.join("kit.samples/C3-1.wav"), b"not mine").unwrap();
        let session = dir.join("kit.phos");
        let mut state = SamplerState::new();
        let pad = SamplerState::pad_of_note(60).unwrap();
        state.add_take_layer(pad, &take(10)).unwrap();
        write_takes(&session, &mut state).unwrap();
        assert_eq!(
            state.pads[pad].layers[0].path,
            PathBuf::from("kit.samples/C3-1-2.wav"),
            "the take overwrote a file that was already there",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sharp_in_a_pad_name_does_not_reach_the_filesystem() {
        let dir = scratch("sharp");
        let session = dir.join("kit.phos");
        let mut state = SamplerState::new();
        let pad = SamplerState::pad_of_note(61).unwrap(); // C#3
        state.add_take_layer(pad, &take(10)).unwrap();
        write_takes(&session, &mut state).unwrap();
        assert_eq!(
            state.pads[pad].layers[0].path,
            PathBuf::from("kit.samples/Cs3-1.wav"),
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A take recorded into a zone is a performance that exists nowhere
    /// else either: it is written out like any other, named for the zone's
    /// first key, and a pad's take on that same key does not overwrite it.
    #[test]
    fn a_zones_take_is_written_out_too() {
        let dir = scratch("zone-take");
        let session = dir.join("kit.phos");
        let mut state = SamplerState::new();
        state.mode = crate::sampler::MapMode::Keys;
        let lo = SamplerState::pad_of_note(60).unwrap();
        let mut pad = crate::sampler::PadState::empty(60);
        pad.add_take(&take(50), "zone").unwrap();
        state.zones.push(crate::sampler::Zone::new(lo, lo + 11, pad));
        // ...and one on the pad under the same key, in the mode that is off.
        state.add_take_layer(lo, &take(50)).unwrap();

        assert_eq!(write_takes(&session, &mut state).unwrap(), 2);
        let zone_path = state.zones[0].pad.layers[0].path.clone();
        let pad_path = state.pads[lo].layers[0].path.clone();
        assert_ne!(zone_path, pad_path, "the two takes share one file");
        assert!(dir.join(&zone_path).exists(), "the zone's take was not written");
        assert!(dir.join(&pad_path).exists(), "the pad's take was not written");

        // A second save writes neither of them again.
        assert_eq!(write_takes(&session, &mut state).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A kit of nothing but wav layers writes no sidecar at all — the
    /// directory is only made when there is something to put in it.
    #[test]
    fn a_kit_with_no_takes_makes_no_directory() {
        let dir = scratch("nothing");
        let session = dir.join("kit.phos");
        let mut state = SamplerState::new();
        state
            .add_wav_layer(
                0,
                PathBuf::from("kick.wav"),
                Arc::new(SamplePcm { data: vec![0.0; 8], channels: 1, sample_rate: 44_100.0 }),
            )
            .unwrap();
        assert_eq!(write_takes(&session, &mut state).unwrap(), 0);
        assert!(!sidecar_dir(&session).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
