//! Session save/load — .phos file format.
//!
//! Serializes the full project state to a human-readable JSON file.
//! Atomic writes (tmp + rename) prevent corruption.

use std::path::Path;
use serde::{Serialize, Deserialize};
use anyhow::Result;

use crate::state::{FxInstance, FxType, GridResolution, InstrumentType, NavState, TrackState};
use phosphor_core::fx::SendSlot;
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
    /// The send buses and the master: their insert chains and return levels.
    ///
    /// Absent — not null — whenever all three are empty and both returns sit
    /// at unity, which is every session written before the insert layer
    /// existed and every session that has not used one since. That is what
    /// makes this addition invisible to `session_digest`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buses: Option<SessionBuses>,
}

#[derive(Serialize, Deserialize)]
pub struct SessionTransport {
    pub tempo_bpm: f64,
    pub loop_enabled: bool,
    pub loop_start_bar: u32,
    pub loop_end_bar: u32,
    pub metronome: bool,
    /// Bars of count-in before recording. Absent in files written before
    /// the count-in existed, which is the same as off.
    #[serde(default)]
    pub count_in_bars: u32,
    /// Record quantize: 0 off, 1 = 1/32, 2 = 1/16, 3 = 1/8. Absent in
    /// older files, which is off.
    #[serde(default)]
    pub record_quantize: u8,
    /// Whether R clears the loop range before recording (re-record) rather
    /// than layering onto it (overdub, the default and the absent value).
    #[serde(default)]
    pub record_replace: bool,
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
    /// The step sequencer on this track, when it has one.
    ///
    /// Absent — not null — on every track that is not one, so a session with
    /// no sequencer in it is byte for byte the file it was before sequencers
    /// existed. That is what `session_digest` is run against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequencer: Option<crate::sequencer::SessionSequencer>,
    /// This track's insert chain, in order. Each effect by name, so that a
    /// build which reorders its menu — or gains an effect between it and the
    /// one that wrote the file — still loads the right thing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fx: Vec<SessionFx>,
    /// The pre-instrument MIDI effects, same shape as the audio chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub midi_fx: Vec<SessionFx>,
    /// Pan position, −1..=1. Absent at centre, which is where every track
    /// written before pan existed sits.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pan: f32,
    /// Send levels as linear gains. Absent when closed, which is the default.
    ///
    /// Linear rather than decibels because a closed send is −inf dB and JSON
    /// has no infinity: `serde_json` writes it as `null`, and a level that
    /// round-trips into a different type is a bug waiting for the first
    /// player who closes a send and saves.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub send_a: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub send_b: f32,
    /// Which track this one's sidechain keys off, as a position in this
    /// file's own track list.
    ///
    /// Identity, not a runtime id: the mixer's ids are handed out as tracks
    /// are created and mean nothing between one session and the next, whereas
    /// a position in the file is exactly as stable as the file is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_track: Option<usize>,
}

/// One effect in a saved chain.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionFx {
    /// The effect's stable name — see [`FxType::key`]. A name this build does
    /// not know is a slot that is dropped with a warning rather than a
    /// session that will not open.
    pub kind: String,
    /// Absent unless the slot is bypassed, which is the exception.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// The effect's controls in its own units, in the order it declares them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<f32>,
    /// A chord device's user progression, resolved. Stored in the same
    /// dual-read form the library uses, so learned voicings travel too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chords: Vec<crate::progressions::StoredChord>,
    /// The loaded progression's display name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chords_name: String,
}

/// The send buses and the master.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SessionBuses {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub send_a: Vec<SessionFx>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub send_b: Vec<SessionFx>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub master: Vec<SessionFx>,
    /// Return level of each bus into the master, as a linear gain.
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub return_a: f32,
    #[serde(default = "unity", skip_serializing_if = "is_unity")]
    pub return_b: f32,
}

impl SessionBuses {
    /// Whether there is anything here worth writing down.
    fn is_default(&self) -> bool {
        self.send_a.is_empty()
            && self.send_b.is_empty()
            && self.master.is_empty()
            && is_unity(&self.return_a)
            && is_unity(&self.return_b)
    }

    /// The chain for one of the two sends.
    #[must_use]
    pub fn send(&self, slot: SendSlot) -> &[SessionFx] {
        match slot {
            SendSlot::A => &self.send_a,
            SendSlot::B => &self.send_b,
        }
    }

    /// The return level for one of the two sends.
    #[must_use]
    pub fn return_level(&self, slot: SendSlot) -> f32 {
        match slot {
            SendSlot::A => self.return_a,
            SendSlot::B => self.return_b,
        }
    }
}

fn unity() -> f32 {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's `skip_serializing_if` shape
fn is_unity(v: &f32) -> bool {
    *v == 1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(v: &bool) -> bool {
    !*v
}

/// A chain as it is written to disk.
#[must_use]
pub fn chain_to_session(chain: &[FxInstance]) -> Vec<SessionFx> {
    chain
        .iter()
        .map(|slot| SessionFx {
            kind: slot.fx_type.key().to_string(),
            bypass: slot.bypass,
            params: slot.params.clone(),
            chords: Vec::new(),
            chords_name: String::new(),
        })
        .collect()
}

/// A chain as it comes back.
///
/// Slots naming an effect this build does not have are dropped — the rest of
/// the chain loads, and the caller is told how many went missing so it can
/// say so rather than leaving the player to notice the reverb is gone.
#[must_use]
pub fn chain_from_session(stored: &[SessionFx]) -> (Vec<FxInstance>, usize) {
    let mut chain = Vec::new();
    let mut dropped = 0;
    for slot in stored {
        let Some(fx_type) = FxType::from_key(&slot.kind) else {
            dropped += 1;
            continue;
        };
        chain.push(FxInstance {
            fx_type,
            bypass: slot.bypass,
            params: slot.params.clone(),
            // Attached when the chain reaches the audio thread, which is
            // where the effect that owns the meter is actually built.
            gr: None,
        });
    }
    (chain, dropped)
}

/// The MIDI rack as it is stored, and as it comes back. The same
/// name-plus-params shape the audio chain uses, and the same rule for a
/// name this build does not know: the slot is dropped, the session opens.
#[must_use]
pub fn midi_fx_to_session(rack: &[crate::state::MidiFxInstance]) -> Vec<SessionFx> {
    rack.iter()
        .map(|slot| SessionFx {
            kind: slot.fx_type.key().to_string(),
            bypass: slot.bypass,
            params: slot.params.clone(),
            chords: slot
                .custom_chords
                .iter()
                .map(crate::progressions::StoredChord::from_wire)
                .collect(),
            chords_name: slot.custom_name.clone(),
        })
        .collect()
}

#[must_use]
pub fn midi_fx_from_session(stored: &[SessionFx]) -> (Vec<crate::state::MidiFxInstance>, usize) {
    let mut rack = Vec::new();
    let mut dropped = 0;
    for slot in stored {
        let Some(fx_type) = crate::state::MidiFxType::from_key(&slot.kind) else {
            dropped += 1;
            continue;
        };
        rack.push(crate::state::MidiFxInstance {
            fx_type,
            bypass: slot.bypass,
            params: slot.params.clone(),
            custom_chords: slot
                .chords
                .iter()
                .map(crate::progressions::StoredChord::to_wire)
                .collect(),
            custom_name: slot.chords_name.clone(),
        });
    }
    (rack, dropped)
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
    /// Recorded performance controllers as `(tick, status, data1, data2)`,
    /// ticks from the clip's start. Absent — not an empty list — for every
    /// clip that has none, which is every clip written before controllers
    /// were recorded at all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<(i64, u8, u8, u8)>,
}

#[derive(Serialize, Deserialize)]
pub struct SessionNote {
    pub note: u8,
    pub velocity: u8,
    /// Ticks from the clip's start — the format every save writes now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_tick: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ticks: Option<i64>,
    /// The old fractional position. Never written any more; still read, so
    /// every session saved before the tick format loads exactly as it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_frac: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_frac: Option<f64>,
    /// Default keeps sessions saved before the flag existed loading clean.
    #[serde(default)]
    pub muted: bool,
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
        InstrumentType::Prophet6 => "prophet6",
        InstrumentType::Teo5 => "teo5",
        InstrumentType::Sequencer => "sequencer",
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
        "prophet6" => Some(InstrumentType::Prophet6),
        "teo5" => Some(InstrumentType::Teo5),
        "sequencer" => Some(InstrumentType::Sequencer),
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

    // A sidechain key names a track by the mixer id it has *this run*, and
    // the file has to name it by something that survives being closed. The
    // saved tracks are numbered in the order they are written, so that is the
    // identity: this maps one to the other.
    let mut position: Vec<(usize, usize)> = Vec::new();
    for track in nav.tracks.iter().filter(|t| t.instrument_type.is_some()) {
        if let Some(mixer_id) = track.mixer_id {
            position.push((mixer_id, position.len()));
        }
    }
    let position_of = |mixer_id: usize| -> Option<usize> {
        position.iter().find(|(id, _)| *id == mixer_id).map(|(_, at)| *at)
    };

    for track in &nav.tracks {
        // Only save instrument tracks (not bus tracks)
        if track.instrument_type.is_none() {
            continue;
        }

        let clips: Vec<SessionClip> = track.clips.iter().map(|clip| {
            SessionClip {
                start_tick: clip.start_tick,
                length_ticks: clip.length_ticks,
                controls: clip
                    .controls
                    .iter()
                    .map(|e| (e.tick, e.status, e.data1, e.data2))
                    .collect(),
                notes: clip.notes.iter().map(|n| SessionNote {
                    note: n.note,
                    velocity: n.velocity,
                    start_tick: Some(n.start_tick),
                    duration_ticks: Some(n.duration_ticks),
                    start_frac: None,
                    duration_frac: None,
                    muted: n.muted,
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
            sequencer: track.sequencer.as_ref().map(|state| {
                crate::sequencer::SessionSequencer::from_state(
                    state,
                    track.instrument_type.unwrap_or(crate::sequencer::DEFAULT_CHILD),
                    &track.synth_params,
                )
            }),
            fx: chain_to_session(&track.fx_chain),
            midi_fx: midi_fx_to_session(&track.midi_fx),
            pan: track.pan,
            send_a: track.send(SendSlot::A),
            send_b: track.send(SendSlot::B),
            key_track: track.key_source.and_then(position_of),
        });
    }

    let buses = extract_buses(nav);

    SessionFile {
        version: FORMAT_VERSION,
        transport: SessionTransport {
            tempo_bpm: transport.tempo_bpm(),
            loop_enabled: nav.loop_editor.enabled,
            loop_start_bar: nav.loop_editor.start_bar,
            loop_end_bar: nav.loop_editor.end_bar,
            metronome: transport.is_metronome_on(),
            count_in_bars: transport.count_in_bars(),
            record_replace: nav.record_replace,
            record_quantize: match nav.clip_view.piano_roll.record_quantize {
                None => 0,
                Some(GridResolution::ThirtySecond) => 1,
                Some(GridResolution::Sixteenth) => 2,
                _ => 3,
            },
        },
        tracks,
        buses: (!buses.is_default()).then_some(buses),
    }
}

/// The bus strips as they are written down.
fn extract_buses(nav: &NavState) -> SessionBuses {
    let strip = |kind: phosphor_core::project::TrackKind| -> Option<&TrackState> {
        nav.tracks.iter().find(|t| t.kind == kind)
    };
    use phosphor_core::project::TrackKind;
    let send_a = strip(TrackKind::SendA);
    let send_b = strip(TrackKind::SendB);
    SessionBuses {
        send_a: send_a.map(|t| chain_to_session(&t.fx_chain)).unwrap_or_default(),
        send_b: send_b.map(|t| chain_to_session(&t.fx_chain)).unwrap_or_default(),
        master: strip(TrackKind::Master)
            .map(|t| chain_to_session(&t.fx_chain))
            .unwrap_or_default(),
        return_a: send_a.map_or(1.0, |t| t.volume),
        return_b: send_b.map_or(1.0, |t| t.volume),
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

/// Get the notes for a clip as NoteSnapshots. Ticks pass straight through;
/// a note from an old fractional session is converted once, here, against
/// the clip length it was saved with — the last fraction-to-tick sum in the
/// application.
pub fn session_notes_to_snapshots(
    notes: &[SessionNote],
    length_ticks: i64,
) -> Vec<phosphor_core::clip::NoteSnapshot> {
    notes.iter().map(|n| {
        let start_tick = n.start_tick.unwrap_or_else(|| {
            (n.start_frac.unwrap_or(0.0) * length_ticks as f64).round() as i64
        });
        let duration_ticks = n.duration_ticks.unwrap_or_else(|| {
            ((n.duration_frac.unwrap_or(0.0) * length_ticks as f64).round() as i64).max(1)
        });
        phosphor_core::clip::NoteSnapshot {
            note: n.note,
            velocity: n.velocity,
            start_tick,
            duration_ticks,
            muted: n.muted,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session saved before the tick format stores notes as fractions.
    /// It must load with every note on the same tick it always played on.
    #[test]
    fn fractional_notes_from_old_sessions_convert_once_and_exactly() {
        let json = r#"{"note":64,"velocity":90,"start_frac":0.5,"duration_frac":0.25,"muted":true}"#;
        let old: SessionNote = serde_json::from_str(json).expect("old note parses");
        let snaps = session_notes_to_snapshots(&[old], 3840);
        assert_eq!(snaps[0].start_tick, 1920);
        assert_eq!(snaps[0].duration_ticks, 960);
        assert!(snaps[0].muted, "the mute flag was dropped in conversion");

        // And a new-format note passes straight through, fractions ignored.
        let json = r#"{"note":60,"velocity":100,"start_tick":77,"duration_ticks":192}"#;
        let new: SessionNote = serde_json::from_str(json).expect("tick note parses");
        let snaps = session_notes_to_snapshots(&[new], 3840);
        assert_eq!(snaps[0].start_tick, 77);
        assert_eq!(snaps[0].duration_ticks, 192);
    }

    /// New saves write ticks and omit the legacy fraction keys entirely.
    #[test]
    fn new_saves_write_ticks_and_no_fractions() {
        let note = SessionNote {
            note: 60, velocity: 100,
            start_tick: Some(960), duration_ticks: Some(480),
            start_frac: None, duration_frac: None, muted: false,
        };
        let json = serde_json::to_string(&note).expect("serializes");
        assert!(json.contains("start_tick"), "ticks missing from save: {json}");
        assert!(!json.contains("frac"), "legacy keys leaked into a new save: {json}");
    }

    /// Sessions saved before the mute flag existed have no `muted` key on
    /// their notes, and they must load with every note sounding.
    #[test]
    fn notes_without_a_mute_key_load_unmuted() {
        let json = r#"{"note":60,"velocity":100,"start_frac":0.0,"duration_frac":0.25}"#;
        let note: SessionNote = serde_json::from_str(json).expect("old note parses");
        assert!(!note.muted);
    }


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
                count_in_bars: 0,
                record_quantize: 0,
                record_replace: false,
            },
            buses: Some(SessionBuses {
                send_a: vec![SessionFx {
                    kind: "reverb".into(),
                    bypass: false,
                    params: vec![20.0, 1.8],
                    chords: Vec::new(),
                    chords_name: String::new(),
                }],
                send_b: Vec::new(),
                master: vec![SessionFx { kind: "eq".into(), bypass: true, params: vec![0.0] ,
                    chords: Vec::new(),
                    chords_name: String::new(),}],
                return_a: 0.8,
                return_b: 1.0,
            }),
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
                    sequencer: None,
                    midi_fx: Vec::new(),
                    fx: vec![
                        SessionFx {
                            kind: "eq".into(),
                            bypass: false,
                            params: vec![120.0, 3.0],
                            chords: Vec::new(),
                            chords_name: String::new(),
                        },
                        SessionFx { kind: "comp".into(), bypass: true, params: vec![-18.0] ,
                    chords: Vec::new(),
                    chords_name: String::new(),},
                    ],
                    pan: -0.5,
                    send_a: 0.5,
                    send_b: 0.0,
                    key_track: Some(1),
                    clips: vec![
                        SessionClip {
                            start_tick: 0,
                            length_ticks: 3840,
                            notes: vec![
                                SessionNote { note: 60, velocity: 100, start_tick: Some(0), duration_ticks: Some(960), start_frac: None, duration_frac: None, muted: false },
                                SessionNote { note: 64, velocity: 80, start_tick: Some(960), duration_ticks: Some(960), start_frac: None, duration_frac: None, muted: false },
                            ],
                            controls: Vec::new(),
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

        // The insert layer: chains by name, pan, sends and the key identity.
        assert_eq!(loaded.tracks[0].fx.len(), 2);
        assert_eq!(loaded.tracks[0].fx[0].kind, "eq");
        assert_eq!(loaded.tracks[0].fx[0].params, vec![120.0, 3.0]);
        assert!(loaded.tracks[0].fx[1].bypass);
        assert_eq!(loaded.tracks[0].pan, -0.5);
        assert_eq!(loaded.tracks[0].send_a, 0.5);
        assert_eq!(loaded.tracks[0].send_b, 0.0);
        assert_eq!(loaded.tracks[0].key_track, Some(1));

        let buses = loaded.buses.expect("the buses were written");
        assert_eq!(buses.send(SendSlot::A)[0].kind, "reverb");
        assert!(buses.send(SendSlot::B).is_empty());
        assert_eq!(buses.master[0].kind, "eq");
        assert_eq!(buses.return_level(SendSlot::A), 0.8);
        assert_eq!(buses.return_level(SendSlot::B), 1.0);
    }

    /// A session with nothing in the insert layer is the file it was before
    /// the insert layer existed — byte for byte. Every field added here is
    /// absent at its default, which is what lets an old session load and
    /// re-save without moving.
    #[test]
    fn a_session_that_uses_no_effects_writes_no_effect_fields() {
        let session = SessionFile {
            version: FORMAT_VERSION,
            transport: SessionTransport {
                tempo_bpm: 120.0,
                loop_enabled: false,
                loop_start_bar: 1,
                loop_end_bar: 2,
                metronome: false,
                count_in_bars: 0,
                record_quantize: 0,
                record_replace: false,
            },
            buses: None,
            tracks: vec![SessionTrack {
                name: "synth".into(),
                instrument_type: "synth".into(),
                synth_params: vec![0.5],
                discrete: Vec::new(),
                muted: false,
                soloed: false,
                armed: false,
                volume: 0.75,
                color_index: 0,
                clips: Vec::new(),
                sequencer: None,
                midi_fx: Vec::new(),
                fx: Vec::new(),
                pan: 0.0,
                send_a: 0.0,
                send_b: 0.0,
                key_track: None,
            }],
        };
        let json = serde_json::to_string_pretty(&session).unwrap();
        for absent in ["\"fx\"", "\"pan\"", "\"send_a\"", "\"send_b\"", "\"key_track\"", "\"buses\""] {
            assert!(
                !json.contains(absent),
                "an unused {absent} was written into the file:\n{json}"
            );
        }
    }

    /// A file written before any of this existed still opens, with every new
    /// field at the value that means "not used".
    #[test]
    fn a_session_from_before_the_insert_layer_loads() {
        let json = r#"{
            "version": 2,
            "transport": {
                "tempo_bpm": 128.0,
                "loop_enabled": false,
                "loop_start_bar": 1,
                "loop_end_bar": 3,
                "metronome": false
            },
            "tracks": [{
                "name": "juno",
                "instrument_type": "juno60",
                "synth_params": [0.1, 0.2],
                "muted": false,
                "soloed": false,
                "armed": true,
                "volume": 0.75,
                "color_index": 1,
                "clips": []
            }]
        }"#;
        let loaded: SessionFile = serde_json::from_str(json).expect("an old session must load");
        assert_eq!(loaded.tracks.len(), 1);
        assert!(loaded.buses.is_none());
        assert!(loaded.tracks[0].fx.is_empty());
        assert_eq!(loaded.tracks[0].pan, 0.0);
        assert_eq!(loaded.tracks[0].send_a, 0.0);
        assert_eq!(loaded.tracks[0].send_b, 0.0);
        assert_eq!(loaded.tracks[0].key_track, None);
    }

    /// Chains are stored by name, so a build whose menu has grown or been
    /// reordered still loads the effect that was saved rather than the one
    /// that happens to sit in that position now.
    #[test]
    fn a_chain_round_trips_by_name() {
        let chain = vec![
            FxInstance { fx_type: FxType::Eq, bypass: false, params: vec![120.0, 0.7], gr: None },
            FxInstance {
                fx_type: FxType::Delay,
                bypass: true,
                params: vec![0.375, 0.3],
                gr: None,
            },
        ];
        let stored = chain_to_session(&chain);
        assert_eq!(stored[0].kind, "eq");
        assert_eq!(stored[1].kind, "delay");
        let (back, dropped) = chain_from_session(&stored);
        assert_eq!(dropped, 0);
        assert_eq!(back, chain);
    }

    /// An effect this build has never heard of costs its own slot and
    /// nothing else. The rest of the chain, and its order, survive.
    #[test]
    fn an_unknown_effect_costs_one_slot() {
        let stored = vec![
            SessionFx { kind: "eq".into(), bypass: false, params: vec![] ,
                    chords: Vec::new(),
                    chords_name: String::new(),},
            SessionFx { kind: "quantum-flanger".into(), bypass: false, params: vec![] ,
                    chords: Vec::new(),
                    chords_name: String::new(),},
            SessionFx { kind: "reverb".into(), bypass: false, params: vec![] ,
                    chords: Vec::new(),
                    chords_name: String::new(),},
        ];
        let (chain, dropped) = chain_from_session(&stored);
        assert_eq!(dropped, 1);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].fx_type, FxType::Eq);
        assert_eq!(chain[1].fx_type, FxType::Reverb);
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
