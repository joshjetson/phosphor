//! Session save/load — .phos file format.
//!
//! Serializes the full project state to a human-readable JSON file.
//! Atomic writes (tmp + rename) prevent corruption.

use std::path::Path;
use serde::{Serialize, Deserialize};
use anyhow::Result;

use crate::state::{NavState, InstrumentType};
use phosphor_core::transport::Transport;

// ── Session file format ──

/// Current `.phos` format version.
///
/// * **1** — every synth parameter stored as the normalised `f32` the panel
///   holds, selectors included.
/// * **2** — selectors additionally stored by the position they pick, in
///   [`SessionTrack::discrete`]. A fraction only names a patch as long as the
///   bank is the size it was when the fraction was written, and two banks have
///   since changed size; see [`crate::discrete`]. Version 1 files still load —
///   see `do_load` — but their selectors are only right if nothing has been
///   added to the bank since.
pub const FORMAT_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub transport: SessionTransport,
    pub tracks: Vec<SessionTrack>,
}

#[derive(Serialize, Deserialize)]
pub struct SessionTransport {
    pub tempo_bpm: f64,
    pub loop_enabled: bool,
    pub loop_start_bar: u32,
    pub loop_end_bar: u32,
    pub metronome: bool,
}

#[derive(Serialize, Deserialize)]
pub struct SessionTrack {
    pub name: String,
    pub instrument_type: String,
    pub synth_params: Vec<f32>,
    /// Where every selector on this panel was pointing, by position rather
    /// than by knob fraction. Absent in version 1 files.
    #[serde(default)]
    pub discrete: Vec<SessionSelector>,
    pub muted: bool,
    pub soloed: bool,
    pub armed: bool,
    pub volume: f32,
    pub color_index: usize,
    pub clips: Vec<SessionClip>,
}

/// One discrete control, stored by what it selects.
///
/// The knob fraction is still in `synth_params` — this is the authority when
/// both are present, and the fraction is what a version 1 file has to fall
/// back on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSelector {
    /// Index into `synth_params`.
    pub param: usize,
    /// Which position of that control, counting from zero.
    pub index: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SessionClip {
    pub start_tick: i64,
    pub length_ticks: i64,
    pub notes: Vec<SessionNote>,
}

#[derive(Serialize, Deserialize)]
pub struct SessionNote {
    pub note: u8,
    pub velocity: u8,
    pub start_frac: f64,
    pub duration_frac: f64,
}

// ── InstrumentType <-> String conversion ──

/// The stable on-disk spelling of an instrument type.
///
/// One source of truth: sessions store it per track, and the preset banks are
/// named after it, so a rename here has to move both together rather than
/// leaving one format reading files the other cannot write.
pub fn instrument_key(t: InstrumentType) -> &'static str {
    match t {
        InstrumentType::Synth => "synth",
        InstrumentType::DrumRack => "drums",
        InstrumentType::DX7 => "dx7",
        InstrumentType::Jupiter8 => "jupiter8",
        InstrumentType::Odyssey => "odyssey",
        InstrumentType::Juno60 => "juno60",
        InstrumentType::Rhodes => "rhodes",
        InstrumentType::Sampler => "sampler",
        InstrumentType::LittlePhatty => "phatty",
    }
}

fn instrument_type_to_string(t: InstrumentType) -> String {
    instrument_key(t).to_string()
}

fn string_to_instrument_type(s: &str) -> Option<InstrumentType> {
    match s {
        "synth" => Some(InstrumentType::Synth),
        "drums" => Some(InstrumentType::DrumRack),
        "dx7" => Some(InstrumentType::DX7),
        "jupiter8" => Some(InstrumentType::Jupiter8),
        "odyssey" => Some(InstrumentType::Odyssey),
        "juno60" => Some(InstrumentType::Juno60),
        "rhodes" => Some(InstrumentType::Rhodes),
        "sampler" => Some(InstrumentType::Sampler),
        "phatty" => Some(InstrumentType::LittlePhatty),
        _ => None,
    }
}

// ── Save ──

pub fn save(path: &Path, nav: &NavState, transport: &Transport) -> Result<()> {
    let session = extract_session(nav, transport);
    let json = serde_json::to_string_pretty(&session)?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Atomic write: write to tmp, then rename
    let tmp = path.with_extension("phos.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;

    tracing::debug!("session saved: {}", path.display());
    Ok(())
}

fn extract_session(nav: &NavState, transport: &Transport) -> SessionFile {
    let mut tracks = Vec::new();

    for track in &nav.tracks {
        // Only save instrument tracks (not bus tracks)
        if track.instrument_type.is_none() {
            continue;
        }

        let clips: Vec<SessionClip> = track.clips.iter().map(|clip| {
            SessionClip {
                start_tick: clip.start_tick,
                length_ticks: clip.length_ticks,
                notes: clip.notes.iter().map(|n| SessionNote {
                    note: n.note,
                    velocity: n.velocity,
                    start_frac: n.start_frac,
                    duration_frac: n.duration_frac,
                }).collect(),
            }
        }).collect();

        tracks.push(SessionTrack {
            name: track.name.clone(),
            instrument_type: track.instrument_type
                .map(instrument_type_to_string)
                .unwrap_or_default(),
            synth_params: track.synth_params.clone(),
            discrete: track.instrument_type
                .map(|i| selectors_of(i, &track.synth_params))
                .unwrap_or_default(),
            muted: track.muted,
            soloed: track.soloed,
            armed: track.armed,
            volume: track.volume,
            color_index: track.color_index,
            clips,
        });
    }

    SessionFile {
        version: FORMAT_VERSION,
        transport: SessionTransport {
            tempo_bpm: transport.tempo_bpm(),
            loop_enabled: nav.loop_editor.enabled,
            loop_start_bar: nav.loop_editor.start_bar,
            loop_end_bar: nav.loop_editor.end_bar,
            metronome: transport.is_metronome_on(),
        },
        tracks,
    }
}

// ── Load ──

pub fn load(path: &Path) -> Result<SessionFile> {
    let json = std::fs::read_to_string(path)?;
    let session: SessionFile = serde_json::from_str(&json)?;
    tracing::debug!("session loaded: {} (v{}, {} tracks)",
        path.display(), session.version, session.tracks.len());
    Ok(session)
}

/// Get the InstrumentType from a session track string.
pub fn parse_instrument_type(s: &str) -> Option<InstrumentType> {
    string_to_instrument_type(s)
}

// ── Selectors ──

/// Every selector on `params`, as the position it is pointing at.
///
/// Which controls those are comes from the instrument's own `is_discrete`
/// rather than from a list here: a panel that gains a switch has to start
/// storing it without this file being edited, because the failure this guards
/// against is silent.
#[must_use]
pub fn selectors_of(instrument: InstrumentType, params: &[f32]) -> Vec<SessionSelector> {
    (0..params.len())
        .filter(|&param| crate::discrete::is_discrete(instrument, param))
        .filter_map(|param| {
            crate::discrete::index_of(instrument, param, params[param])
                .map(|index| SessionSelector { param, index })
        })
        .collect()
}

/// Point the selectors in `params` at the positions the session stored.
///
/// Returns the entries that could not be restored exactly, as
/// `(parameter, wanted, given)` — a bank that has *shrunk* since the session
/// was written has nothing at the far end of it any more, and the nearest
/// thing to what the player chose is its last entry. Anything the instrument
/// does not call a selector is ignored rather than written blind.
pub fn apply_selectors(
    instrument: InstrumentType,
    params: &mut [f32],
    stored: &[SessionSelector],
) -> Vec<(usize, usize, usize)> {
    let mut clamped = Vec::new();
    for selector in stored {
        if selector.param >= params.len() {
            continue;
        }
        let Some(positions) = crate::discrete::positions(instrument, selector.param) else {
            continue;
        };
        let index = selector.index.min(positions.len().saturating_sub(1));
        let Some(&knob) = positions.get(index) else { continue };
        if index != selector.index {
            clamped.push((selector.param, selector.index, index));
        }
        params[selector.param] = knob;
    }
    clamped
}

/// Get the notes for a clip as NoteSnapshots.
pub fn session_notes_to_snapshots(notes: &[SessionNote]) -> Vec<phosphor_core::clip::NoteSnapshot> {
    notes.iter().map(|n| phosphor_core::clip::NoteSnapshot {
        note: n.note,
        velocity: n.velocity,
        start_frac: n.start_frac,
        duration_frac: n.duration_frac,
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_serialize() {
        let session = SessionFile {
            version: FORMAT_VERSION,
            transport: SessionTransport {
                tempo_bpm: 120.0,
                loop_enabled: true,
                loop_start_bar: 1,
                loop_end_bar: 5,
                metronome: true,
            },
            tracks: vec![
                SessionTrack {
                    name: "synth".into(),
                    instrument_type: "dx7".into(),
                    synth_params: vec![0.0, 0.5, 0.7],
                    discrete: vec![SessionSelector { param: 0, index: 3 }],
                    muted: false,
                    soloed: false,
                    armed: true,
                    volume: 0.75,
                    color_index: 2,
                    clips: vec![
                        SessionClip {
                            start_tick: 0,
                            length_ticks: 3840,
                            notes: vec![
                                SessionNote { note: 60, velocity: 100, start_frac: 0.0, duration_frac: 0.25 },
                                SessionNote { note: 64, velocity: 80, start_frac: 0.25, duration_frac: 0.25 },
                            ],
                        },
                    ],
                },
            ],
        };

        let json = serde_json::to_string_pretty(&session).unwrap();
        let loaded: SessionFile = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.version, FORMAT_VERSION);
        assert_eq!(loaded.transport.tempo_bpm, 120.0);
        assert!(loaded.transport.loop_enabled);
        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].name, "synth");
        assert_eq!(loaded.tracks[0].instrument_type, "dx7");
        assert_eq!(loaded.tracks[0].synth_params, vec![0.0, 0.5, 0.7]);
        assert_eq!(
            loaded.tracks[0].discrete,
            vec![SessionSelector { param: 0, index: 3 }]
        );
        assert_eq!(loaded.tracks[0].clips.len(), 1);
        assert_eq!(loaded.tracks[0].clips[0].notes.len(), 2);
        assert_eq!(loaded.tracks[0].clips[0].notes[0].note, 60);
    }

    #[test]
    fn instrument_type_round_trip() {
        for inst in InstrumentType::ALL {
            let s = instrument_type_to_string(*inst);
            let back = string_to_instrument_type(&s);
            assert_eq!(back, Some(*inst), "Failed round-trip for {s}");
        }
    }
}
