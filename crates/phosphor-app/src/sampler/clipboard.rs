//! Copying on the sampler: a whole pad, or one sound off it.
//!
//! `y` takes the pad under the caret whole — every sound on it, every
//! setting, the instrument it was recorded from — and `p` lays it on
//! another key, replacing what was there. `Y` takes just the sound under
//! the layer cursor, and `p` stacks it onto the key's own sounds. In keys
//! mode the same keys address the zone under the caret, as every pad key
//! does.
//!
//! A copy is a copy of the settings and a *share* of the audio: the pasted
//! layers hold the same buffers the originals do, so copying a long break
//! to ten keys costs the break once, and saves as one file.
//!
//! Phrases are notes played through the sampler's one child instrument, so
//! a pad carrying phrases carries its kit's child with it. Pasted into
//! another sampler, it points that kit's child at the same instrument — the
//! phrase would otherwise play through whatever the new kit happened to
//! have, and sound like something else — and says so, in the words
//! recording a phrase already uses.

use super::{ChildChange, LayerState, PadRow, PadSource, PadState, SamplerState};

/// What `y` or `Y` took.
#[derive(Debug, Clone)]
pub enum SamplerClip {
    /// A whole pad or zone sound, with the child its phrases play through.
    Pad { pad: Box<PadState>, child: Option<PadSource>, from: String },
    /// One sampled sound.
    Sound { layer: LayerState, from: String },
}

impl SamplerClip {
    /// What it is, for the flash: `pad C4`, `kick from pad C4`.
    pub fn label(&self) -> &str {
        match self {
            Self::Pad { from, .. } | Self::Sound { from, .. } => from,
        }
    }
}

/// What a paste did.
#[derive(Debug, Clone, PartialEq)]
pub struct Pasted {
    /// Where the pasted sound now sits in the target's list — the row the
    /// layer cursor should stand on.
    pub row: usize,
    /// What pointing the kit's child at the pasted pad's instrument changed.
    pub child: ChildChange,
}

impl SamplerState {
    /// `y`: the pad (or zone sound) under the caret, whole.
    pub fn yank_pad(&self) -> Result<SamplerClip, String> {
        let title = self.edit_title();
        let pad = self.edited().ok_or_else(SamplerState::no_zone_message)?;
        if pad.layers.is_empty() && pad.phrases.is_empty() {
            return Err(format!("nothing on {title} to copy \u{00b7} a loads a sound"));
        }
        let child = if pad.phrases.is_empty() { None } else { self.child.clone() };
        Ok(SamplerClip::Pad { pad: Box::new(pad.clone()), child, from: title })
    }

    /// `Y`: the one sound at `row` of the pad under the caret.
    pub fn yank_sound(&self, row: usize) -> Result<SamplerClip, String> {
        let title = self.edit_title();
        let pad = self.edited().ok_or_else(SamplerState::no_zone_message)?;
        match pad.row(row) {
            Some(PadRow::Layer(layer)) => Ok(SamplerClip::Sound {
                from: format!("{} from {title}", layer.name),
                layer: layer.clone(),
            }),
            Some(PadRow::Phrase(_)) => Err(
                "Y copies one sampled sound \u{00b7} y copies the whole pad, phrases and all".into(),
            ),
            None => Err(format!("nothing on {title} to copy \u{00b7} a loads a sound")),
        }
    }

    /// `p`: put `clip` on the pad (or zone) under the caret.
    ///
    /// A pad replaces what was there, settings and all — a paste is a copy
    /// of the thing, and the undo step is where the old one is kept. A sound
    /// is stacked onto the key's own, and refused in words when the key is
    /// full. `Err` changes nothing.
    pub fn paste(&mut self, clip: &SamplerClip) -> Result<Pasted, String> {
        let title = self.edit_title();
        match clip {
            SamplerClip::Pad { pad, child, .. } => {
                let target = self.edited_mut().ok_or_else(SamplerState::no_zone_message)?;
                *target = (**pad).clone();
                let change = match child {
                    Some(child) => self.set_child(child.clone()),
                    None => ChildChange::Same,
                };
                Ok(Pasted { row: 0, child: change })
            }
            SamplerClip::Sound { layer, .. } => {
                let target = self.edited_mut().ok_or_else(SamplerState::no_zone_message)?;
                let row = target.push_layer(layer.clone(), &title)?;
                Ok(Pasted { row, child: ChildChange::Same })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::{MapMode, Zone};
    use crate::state::InstrumentType;
    use phosphor_plugin::sample::{SamplePcm, TrigMode};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn pcm() -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![0.5; 100], channels: 1, sample_rate: 48_000.0 })
    }

    fn pad(note: u8) -> usize {
        SamplerState::pad_of_note(note).unwrap()
    }

    /// A kit with a dressed pad on C4: two sounds, a gate trigger, a choke.
    fn kit() -> SamplerState {
        let mut state = SamplerState::new();
        let c4 = pad(60);
        state.add_wav_layer(c4, PathBuf::from("kick.wav"), pcm()).unwrap();
        state.add_wav_layer(c4, PathBuf::from("click.wav"), pcm()).unwrap();
        state.pads[c4].config.trig = TrigMode::Gate;
        state.pads[c4].config.choke = 3;
        state.pads[c4].layers[0].start_frame = 20;
        state.cursor = c4;
        state
    }

    #[test]
    fn a_pad_copies_whole_and_replaces_the_target() {
        let mut state = kit();
        let clip = state.yank_pad().unwrap();
        assert_eq!(clip.label(), "pad C4");
        let d4 = pad(62);
        state.add_wav_layer(d4, PathBuf::from("old.wav"), pcm()).unwrap();
        state.cursor = d4;
        state.paste(&clip).unwrap();
        assert_eq!(state.pads[d4], state.pads[pad(60)], "the copy is not the pad");
        assert!(state.pads[d4].layers.iter().all(|l| l.name != "old"), "the old sound stayed");
        let (a, b) = (&state.pads[pad(60)].layers[0], &state.pads[d4].layers[0]);
        assert!(Arc::ptr_eq(a.pcm.as_ref().unwrap(), b.pcm.as_ref().unwrap()), "the audio was duplicated");
    }

    #[test]
    fn one_sound_copies_and_stacks_onto_the_target() {
        let mut state = kit();
        let clip = state.yank_sound(1).unwrap();
        assert_eq!(clip.label(), "click from pad C4");
        let d4 = pad(62);
        state.add_wav_layer(d4, PathBuf::from("snare.wav"), pcm()).unwrap();
        state.cursor = d4;
        let pasted = state.paste(&clip).unwrap();
        assert_eq!(pasted.row, 1, "the cursor would not stand on what arrived");
        let names: Vec<_> = state.pads[d4].layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["snare", "click"]);
        // The source pad is gate; the target keeps the fresh pad's trigger.
        assert_eq!(state.pads[d4].config.trig, PadState::fresh_config(62).trig, "a sound brought its pad's settings");
    }

    #[test]
    fn a_full_key_refuses_a_sound_and_keeps_its_own() {
        let mut state = kit();
        let clip = state.yank_sound(0).unwrap();
        let d4 = pad(62);
        for _ in 0..crate::sampler::MAX_LAYERS {
            state.add_wav_layer(d4, PathBuf::from("x.wav"), pcm()).unwrap();
        }
        state.cursor = d4;
        let before = state.clone();
        assert!(state.paste(&clip).is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn an_empty_pad_or_a_phrase_row_has_nothing_to_copy_in_words() {
        let mut state = SamplerState::new();
        let err = state.yank_pad().unwrap_err();
        assert!(err.contains("nothing on pad C4"), "{err}");
        assert!(state.yank_sound(0).is_err());
        state = kit();
        assert!(state.yank_sound(7).is_err(), "a row past the list was copied");
    }

    /// In keys mode the keys address zones, as every other pad key does.
    #[test]
    fn keys_mode_copies_zone_to_zone_and_refuses_a_bare_key() {
        let mut state = kit();
        let sound = state.pads[pad(60)].clone();
        state.zones.push(Zone::new(pad(48), pad(59), sound));
        state.zones.push(Zone::new(pad(72), pad(83), PadState::empty(72)));
        state.set_mode(MapMode::Keys);
        state.cursor = pad(50);
        let clip = state.yank_pad().unwrap();
        assert!(clip.label().starts_with("zone"), "{}", clip.label());
        state.cursor = pad(75);
        state.paste(&clip).unwrap();
        assert_eq!(state.zones[1].pad.layers.len(), 2);
        assert_eq!((state.zones[1].lo, state.zones[1].hi), (pad(72), pad(83)), "the paste moved the zone");
        state.cursor = pad(65);
        assert!(state.paste(&clip).is_err(), "a bare key in keys mode took a paste");
    }

    /// A pad's phrases play through its kit's child; pasted into a kit that
    /// has another, the child follows, and the paste says what it changed.
    #[test]
    fn a_pad_with_phrases_brings_the_instrument_they_play_through() {
        let mut from = kit();
        from.child = Some(PadSource { instrument: InstrumentType::DX7, params: vec![0.5; 3] });
        let c4 = pad(60);
        let note = phosphor_plugin::sample::PhraseEvent { frame: 0, status: 0x90, data1: 60, data2: 100 };
        from.pads[c4].add_phrase(Arc::from(vec![note]), 100, 48_000.0, "pad C4").unwrap();
        let clip = from.yank_pad().unwrap();

        let mut into = SamplerState::new();
        into.child = Some(PadSource { instrument: InstrumentType::Rhodes, params: vec![] });
        let pasted = into.paste(&clip).unwrap();
        assert_eq!(pasted.child, ChildChange::Instrument);
        assert_eq!(into.child.as_ref().map(|c| c.instrument), Some(InstrumentType::DX7));

        // A pad of sounds alone leaves the kit's child where it was.
        let plain = kit().yank_pad().unwrap();
        assert_eq!(into.paste(&plain).unwrap().child, ChildChange::Same);
        assert_eq!(into.child.as_ref().map(|c| c.instrument), Some(InstrumentType::DX7));
    }
}
