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
    pub muted: bool,
    pub soloed: bool,
    pub armed: bool,
    pub volume: f32,
    pub color_index: usize,
    pub clips: Vec<SessionClip>,
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

fn instrument_type_to_string(t: InstrumentType) -> String {
    match t {
        InstrumentType::Synth => "synth".into(),
        InstrumentType::DrumRack => "drums".into(),
        InstrumentType::DX7 => "dx7".into(),
        InstrumentType::Jupiter8 => "jupiter8".into(),
        InstrumentType::Odyssey => "odyssey".into(),
        InstrumentType::Juno60 => "juno60".into(),
        InstrumentType::Sampler => "sampler".into(),
    }
}

fn string_to_instrument_type(s: &str) -> Option<InstrumentType> {
    match s {
        "synth" => Some(InstrumentType::Synth),
        "drums" => Some(InstrumentType::DrumRack),
        "dx7" => Some(InstrumentType::DX7),
        "jupiter8" => Some(InstrumentType::Jupiter8),
        "odyssey" => Some(InstrumentType::Odyssey),
        "juno60" => Some(InstrumentType::Juno60),
        "sampler" => Some(InstrumentType::Sampler),
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
            muted: track.muted,
            soloed: track.soloed,
            armed: track.armed,
            volume: track.volume,
            color_index: track.color_index,
            clips,
        });
    }

    SessionFile {
        version: 1,
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
            version: 1,
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

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.transport.tempo_bpm, 120.0);
        assert_eq!(loaded.transport.loop_enabled, true);
        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].name, "synth");
        assert_eq!(loaded.tracks[0].instrument_type, "dx7");
        assert_eq!(loaded.tracks[0].synth_params, vec![0.0, 0.5, 0.7]);
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
