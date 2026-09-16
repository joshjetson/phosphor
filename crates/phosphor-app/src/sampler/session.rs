//! How a sampler goes into a `.phos` file and comes back.
//!
//! PCM never enters the JSON — a session file is something a person can
//! read. A wav layer is stored as the path the player typed, and the
//! loader is handed a resolver so the IO stays with the caller (and out
//! of the tests). A path that resolves to nothing keeps its pad and its
//! settings with no audio behind them: the session names what it wanted,
//! and the player fixes the path instead of rebuilding the kit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use phosphor_plugin::sample::{PadConfig, SamplePcm, TrigMode};

use super::{LayerSource, LayerState, PadSource, PadState, SamplerState};

/// The stable spelling of a trigger mode.
fn trig_key(t: TrigMode) -> &'static str {
    match t {
        TrigMode::OneShot => "one-shot",
        TrigMode::Gate => "gate",
    }
}

/// A spelling this build does not know falls back to one-shot: the pad
/// still sounds, which beats a session that will not open.
fn trig_from_key(s: &str) -> TrigMode {
    match s {
        "gate" => TrigMode::Gate,
        _ => TrigMode::OneShot,
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionSampler {
    pub pads: Vec<SessionPad>,
}

/// One pad, addressed by its MIDI note — readable in the file, and
/// stable however the pad table is indexed internally.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionPad {
    pub note: u8,
    pub trig: String,
    pub poly: u8,
    pub choke: u8,
    pub pitch_st: i8,
    pub pitch_cents: i8,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub level: f32,
    pub pan: f32,
    pub root: u8,
    pub keytrack: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<SessionLayer>,
    /// What this pad was last recorded from. Absent on every pad that has
    /// never been resampled, which is every pad in every session written
    /// before takes existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SessionPadSource>,
}

/// A pad's remembered source instrument, by the same key a track's
/// instrument is stored under — so the two cannot drift apart.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionPadSource {
    pub instrument: String,
    pub params: Vec<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionLayer {
    pub path: String,
    /// `take` for a performance rendered here, absent for a file the
    /// player named. Skipped when it is a file so that every session
    /// written before takes existed reads back byte for byte.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub gain: f32,
    pub pan: f32,
    pub tune_st: i8,
    pub tune_cents: i8,
    pub start_frame: u64,
    pub end_frame: u64,
    pub reverse: bool,
    pub mute: bool,
    pub vel_lo: u8,
    pub vel_hi: u8,
}

impl SessionSampler {
    /// What the file keeps: only the pads that differ from a fresh
    /// sampler, so an empty one writes an empty list.
    pub fn from_state(state: &SamplerState) -> Self {
        let pads = state
            .occupied_pads()
            .map(|i| {
                let pad = &state.pads[i];
                let c = &pad.config;
                SessionPad {
                    note: SamplerState::note_of_pad(i),
                    trig: trig_key(c.trig).into(),
                    poly: c.poly,
                    choke: c.choke,
                    pitch_st: c.pitch_st,
                    pitch_cents: c.pitch_cents,
                    attack_ms: c.attack_ms,
                    decay_ms: c.decay_ms,
                    sustain: c.sustain,
                    release_ms: c.release_ms,
                    level: c.level,
                    pan: c.pan,
                    root: c.root,
                    keytrack: c.keytrack,
                    source: pad.source.as_ref().map(|s| SessionPadSource {
                        instrument: crate::session::instrument_key(s.instrument).into(),
                        params: s.params.clone(),
                    }),
                    layers: pad
                        .layers
                        .iter()
                        .map(|l| SessionLayer {
                            path: l.path.display().to_string(),
                            kind: match l.source {
                                LayerSource::File => String::new(),
                                LayerSource::Take => "take".into(),
                            },
                            gain: l.gain,
                            pan: l.pan,
                            tune_st: l.tune_st,
                            tune_cents: l.tune_cents,
                            start_frame: l.start_frame,
                            end_frame: l.end_frame,
                            reverse: l.reverse,
                            mute: l.mute,
                            vel_lo: l.vel_lo,
                            vel_hi: l.vel_hi,
                        })
                        .collect(),
                }
            })
            .collect();
        Self { pads }
    }

    /// Rebuild the state, decoding each layer's file through `resolve`.
    /// `None` from the resolver is a missing file: the layer keeps its
    /// seat with no PCM behind it.
    pub fn into_state(
        &self,
        resolve: impl Fn(&Path) -> Option<Arc<SamplePcm>>,
    ) -> SamplerState {
        let mut state = SamplerState::new();
        for saved in &self.pads {
            let Some(idx) = SamplerState::pad_of_note(saved.note) else { continue };
            let config = PadConfig {
                trig: trig_from_key(&saved.trig),
                poly: saved.poly.clamp(1, 8),
                choke: saved.choke.min(8),
                pitch_st: saved.pitch_st.clamp(-48, 48),
                pitch_cents: saved.pitch_cents.clamp(-50, 50),
                attack_ms: saved.attack_ms.max(0.0),
                decay_ms: saved.decay_ms.max(0.0),
                sustain: saved.sustain.clamp(0.0, 1.0),
                release_ms: saved.release_ms.max(0.0),
                level: saved.level.clamp(0.0, 4.0),
                pan: saved.pan.clamp(-1.0, 1.0),
                root: saved.root.min(127),
                keytrack: saved.keytrack,
            };
            let layers = saved
                .layers
                .iter()
                .take(super::MAX_LAYERS)
                .map(|l| {
                    let path = PathBuf::from(&l.path);
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "sample".into());
                    LayerState {
                        pcm: resolve(&path),
                        path,
                        source: if l.kind == "take" {
                            LayerSource::Take
                        } else {
                            LayerSource::File
                        },
                        name,
                        gain: l.gain,
                        pan: l.pan,
                        tune_st: l.tune_st,
                        tune_cents: l.tune_cents,
                        start_frame: l.start_frame,
                        end_frame: l.end_frame,
                        reverse: l.reverse,
                        mute: l.mute,
                        vel_lo: l.vel_lo,
                        vel_hi: l.vel_hi,
                    }
                })
                .collect();
            // An instrument this build does not know is dropped rather
            // than guessed at: the pad keeps its sounds and forgets only
            // how to record another one.
            let source = saved.source.as_ref().and_then(|s| {
                crate::session::parse_instrument_type(&s.instrument)
                    .map(|instrument| PadSource { instrument, params: s.params.clone() })
            });
            state.pads[idx] = PadState { config, layers, source };
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(frames: usize) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![0.5; frames], channels: 1, sample_rate: 48_000.0 })
    }

    #[test]
    fn a_kit_survives_the_round_trip() {
        let mut state = SamplerState::new();
        state.add_wav_layer(15, PathBuf::from("samples/kick.wav"), pcm(500)).unwrap();
        state.pads[15].config.trig = TrigMode::Gate;
        state.pads[15].config.choke = 2;
        state.pads[15].layers[0].reverse = true;
        state.pads[15].layers[0].start_frame = 100;

        let saved = SessionSampler::from_state(&state);
        assert_eq!(saved.pads.len(), 1);
        assert_eq!(saved.pads[0].note, 36);
        assert_eq!(saved.pads[0].trig, "gate");

        let restored = saved.into_state(|p| {
            assert_eq!(p, Path::new("samples/kick.wav"));
            Some(pcm(500))
        });
        assert_eq!(restored.pads[15].config.trig, TrigMode::Gate);
        assert_eq!(restored.pads[15].config.choke, 2);
        let layer = &restored.pads[15].layers[0];
        assert!(layer.reverse);
        assert_eq!(layer.start_frame, 100);
        assert_eq!(layer.name, "kick");
        assert!(layer.pcm.is_some());
    }

    #[test]
    fn a_missing_file_keeps_its_pad_and_its_settings() {
        let mut state = SamplerState::new();
        state.add_wav_layer(0, PathBuf::from("gone.wav"), pcm(10)).unwrap();
        state.pads[0].layers[0].gain = 0.5;
        let saved = SessionSampler::from_state(&state);
        let restored = saved.into_state(|_| None);
        let layer = &restored.pads[0].layers[0];
        assert!(layer.pcm.is_none());
        assert_eq!(layer.gain, 0.5);
        assert_eq!(layer.path, PathBuf::from("gone.wav"));
        assert_eq!(restored.missing_layers(), 1);
    }

    #[test]
    fn hostile_numbers_in_a_file_are_clamped_on_the_way_in() {
        let saved = SessionSampler {
            pads: vec![SessionPad {
                note: 60,
                trig: "sideways".into(), // unknown spelling
                poly: 200,
                choke: 99,
                pitch_st: 120,
                pitch_cents: -120,
                attack_ms: -5.0,
                decay_ms: 100.0,
                sustain: 9.0,
                release_ms: 50.0,
                level: 100.0,
                pan: -7.0,
                root: 200,
                keytrack: false,
                layers: Vec::new(),
                source: None,
            }],
        };
        let state = saved.into_state(|_| None);
        let c = &state.pads[39].config;
        assert_eq!(c.trig, TrigMode::OneShot);
        assert_eq!(c.poly, 8);
        assert_eq!(c.choke, 8);
        assert_eq!(c.pitch_st, 48);
        assert_eq!(c.sustain, 1.0);
        assert_eq!(c.pan, -1.0);
        assert_eq!(c.root, 127);
    }

    #[test]
    fn a_pad_off_the_bed_in_a_file_is_skipped_not_fatal() {
        let mut pad = SessionPad {
            note: 5, // below A0
            trig: "one-shot".into(),
            poly: 1,
            choke: 0,
            pitch_st: 0,
            pitch_cents: 0,
            attack_ms: 0.0,
            decay_ms: 400.0,
            sustain: 1.0,
            release_ms: 60.0,
            level: 1.0,
            pan: 0.0,
            root: 5,
            keytrack: false,
            layers: Vec::new(),
            source: None,
        };
        let state = SessionSampler { pads: vec![pad.clone()] }.into_state(|_| None);
        assert_eq!(state.occupied_pads().count(), 0);
        pad.note = 250;
        let state = SessionSampler { pads: vec![pad] }.into_state(|_| None);
        assert_eq!(state.occupied_pads().count(), 0);
    }

    /// A take is marked in the file and a file layer is not, so that every
    /// session written before takes existed reads back byte for byte.
    #[test]
    fn a_take_is_marked_and_a_file_layer_is_not() {
        let mut state = SamplerState::new();
        state.add_wav_layer(0, PathBuf::from("kick.wav"), pcm(10)).unwrap();
        state.pads[0].layers.push(LayerState {
            path: PathBuf::from("kit.samples/A-1-1.wav"),
            source: LayerSource::Take,
            name: "take 1".into(),
            pcm: Some(pcm(10)),
            gain: 1.0,
            pan: 0.0,
            tune_st: 0,
            tune_cents: 0,
            start_frame: 0,
            end_frame: 10,
            reverse: false,
            mute: false,
            vel_lo: 0,
            vel_hi: 127,
        });
        let saved = SessionSampler::from_state(&state);
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(json.matches("\"kind\"").count(), 1, "the file layer wrote a kind:\n{json}");
        assert!(json.contains("\"kind\":\"take\""), "the take is not marked:\n{json}");

        let back: SessionSampler = serde_json::from_str(&json).unwrap();
        let restored = back.into_state(|_| Some(pcm(10)));
        assert_eq!(restored.pads[0].layers[0].source, LayerSource::File);
        assert_eq!(restored.pads[0].layers[1].source, LayerSource::Take);
        assert_eq!(restored.takes().count(), 1);
    }

    /// A pad's remembered source instrument survives, and one this build
    /// has never heard of is dropped rather than guessed at.
    #[test]
    fn a_pad_remembers_what_it_was_recorded_from() {
        let mut state = SamplerState::new();
        state.pads[20].source = Some(PadSource {
            instrument: crate::state::InstrumentType::Rhodes,
            params: vec![0.25, 0.75],
        });
        let saved = SessionSampler::from_state(&state);
        assert_eq!(saved.pads[0].source.as_ref().unwrap().instrument, "rhodes");

        let restored = saved.into_state(|_| None);
        let source = restored.pads[20].source.as_ref().expect("the memory went missing");
        assert_eq!(source.instrument, crate::state::InstrumentType::Rhodes);
        assert_eq!(source.params, vec![0.25, 0.75]);

        let mut hostile = SessionSampler::from_state(&state);
        hostile.pads[0].source.as_mut().unwrap().instrument = "moogophone".into();
        let restored = hostile.into_state(|_| None);
        assert!(restored.pads[20].source.is_none(), "an unknown instrument was guessed at");
    }

    #[test]
    fn a_ninth_saved_layer_is_dropped_on_load() {
        let mut state = SamplerState::new();
        for _ in 0..super::super::MAX_LAYERS {
            state.add_wav_layer(0, PathBuf::from("x.wav"), pcm(10)).unwrap();
        }
        let mut saved = SessionSampler::from_state(&state);
        let ninth = saved.pads[0].layers[0].clone(); // a hand-edited ninth
        saved.pads[0].layers.push(ninth);
        let restored = saved.into_state(|_| Some(pcm(10)));
        assert_eq!(restored.pads[0].layers.len(), super::super::MAX_LAYERS);
    }
}
