//! The pad table: 88 pads, one per piano key, each holding up to eight
//! layer slots.
//!
//! Everything here is fixed-capacity, because [`Pad::set`] runs on the
//! audio thread when a `MixerCommand` delivers an edit: copying a config
//! and re-pointing eight `Arc`s is real-time safe, growing a `Vec` is not.
//! The trims are clamped here, once, at delivery — the voice trusts its
//! region completely, so this is the only place `end < start`, a trim past
//! the buffer, or an empty buffer can be caught.

use std::sync::Arc;

use phosphor_plugin::sample::{PadConfig, PadLayer, SamplePcm};

/// One pad per piano key, A0..C8.
pub const NUM_PADS: usize = 88;

/// MIDI note of the lowest pad (A0).
pub const PAD_BASE_NOTE: u8 = 21;

/// Layer slots per pad. The UI enforces the same cap; extras past it are
/// dropped here rather than trusted.
pub const MAX_LAYERS: usize = 8;

/// The pad this note addresses, if any. Notes off the 88-key bed are
/// ignored rather than wrapped — a pad map that aliases is worse than one
/// that is silent at the extremes.
pub fn pad_index(note: u8) -> Option<usize> {
    let last = PAD_BASE_NOTE + (NUM_PADS as u8 - 1);
    if (PAD_BASE_NOTE..=last).contains(&note) {
        Some(usize::from(note - PAD_BASE_NOTE))
    } else {
        None
    }
}

/// A layer as the engine holds it: the same fields as [`PadLayer`], with
/// the trim already clamped into a region the voice can trust.
pub(crate) struct LayerSlot {
    pub pcm: Arc<SamplePcm>,
    pub gain: f32,
    pub pan: f32,
    pub tune_st: i8,
    pub tune_cents: i8,
    /// Clamped: `start < end <= frames`, in frames.
    pub start: f64,
    pub end: f64,
    pub reverse: bool,
    pub mute: bool,
    pub vel_lo: u8,
    pub vel_hi: u8,
}

pub(crate) struct Pad {
    pub config: PadConfig,
    pub layers: [Option<LayerSlot>; MAX_LAYERS],
}

impl Pad {
    pub fn for_note(note: u8) -> Self {
        Self {
            config: PadConfig::for_key(note),
            layers: std::array::from_fn(|_| None),
        }
    }

    /// Replace this pad's config and layers with a sanitized copy.
    ///
    /// Runs on the audio thread: no allocation, `Arc` clones and drops
    /// only. A layer whose buffer is empty is dropped entirely — a voice
    /// must never be pointed at zero frames.
    pub fn set(&mut self, config: &PadConfig, layers: &[PadLayer]) {
        let mut cfg = *config;
        cfg.poly = cfg.poly.clamp(1, 8);
        cfg.choke = cfg.choke.min(8);
        cfg.pitch_st = cfg.pitch_st.clamp(-48, 48);
        cfg.pitch_cents = cfg.pitch_cents.clamp(-50, 50);
        cfg.sustain = cfg.sustain.clamp(0.0, 1.0);
        cfg.level = cfg.level.clamp(0.0, 4.0);
        cfg.pan = cfg.pan.clamp(-1.0, 1.0);
        self.config = cfg;

        for (slot, layer) in self.layers.iter_mut().zip(layers.iter()) {
            let frames = layer.pcm.frames();
            if frames == 0 {
                *slot = None;
                continue;
            }
            let start = layer.start_frame.min(frames - 1);
            let end = layer.end_frame.clamp(start + 1, frames);
            *slot = Some(LayerSlot {
                pcm: Arc::clone(&layer.pcm),
                gain: layer.gain.clamp(0.0, 4.0),
                pan: layer.pan.clamp(-1.0, 1.0),
                tune_st: layer.tune_st.clamp(-48, 48),
                tune_cents: layer.tune_cents.clamp(-50, 50),
                start: start as f64,
                end: end as f64,
                reverse: layer.reverse,
                mute: layer.mute,
                vel_lo: layer.vel_lo.min(127),
                vel_hi: layer.vel_hi.min(127).max(layer.vel_lo.min(127)),
            });
        }
        for slot in self.layers.iter_mut().skip(layers.len().min(MAX_LAYERS)) {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(frames: usize) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![0.5; frames], channels: 1, sample_rate: 44_100.0 })
    }

    #[test]
    fn the_key_bed_maps_and_the_edges_do_not_wrap() {
        assert_eq!(pad_index(21), Some(0));
        assert_eq!(pad_index(108), Some(87));
        assert_eq!(pad_index(20), None);
        assert_eq!(pad_index(109), None);
        assert_eq!(pad_index(0), None);
        assert_eq!(pad_index(127), None);
    }

    #[test]
    fn a_backwards_trim_is_clamped_not_trusted() {
        let mut pad = Pad::for_note(60);
        let mut layer = PadLayer::from_pcm(pcm(100));
        layer.start_frame = 90;
        layer.end_frame = 10; // end before start
        pad.set(&PadConfig::for_key(60), &[layer]);
        let slot = pad.layers[0].as_ref().unwrap();
        assert!(slot.end > slot.start, "region collapsed: {}..{}", slot.start, slot.end);
        assert!(slot.end <= 100.0);
    }

    #[test]
    fn a_trim_past_the_buffer_is_pulled_back_in() {
        let mut pad = Pad::for_note(60);
        let mut layer = PadLayer::from_pcm(pcm(100));
        layer.start_frame = 5_000;
        layer.end_frame = 9_000;
        pad.set(&PadConfig::for_key(60), &[layer]);
        let slot = pad.layers[0].as_ref().unwrap();
        assert_eq!((slot.start, slot.end), (99.0, 100.0));
    }

    #[test]
    fn an_empty_buffer_never_reaches_a_voice() {
        let mut pad = Pad::for_note(60);
        let empty = Arc::new(SamplePcm { data: vec![], channels: 2, sample_rate: 44_100.0 });
        pad.set(&PadConfig::for_key(60), &[PadLayer::from_pcm(empty)]);
        assert!(pad.layers[0].is_none());
    }

    #[test]
    fn a_ninth_layer_is_dropped_and_old_slots_are_cleared() {
        let mut pad = Pad::for_note(60);
        let nine: Vec<PadLayer> = (0..9).map(|_| PadLayer::from_pcm(pcm(10))).collect();
        pad.set(&PadConfig::for_key(60), &nine);
        assert!(pad.layers.iter().all(|s| s.is_some()));

        pad.set(&PadConfig::for_key(60), &[PadLayer::from_pcm(pcm(10))]);
        assert!(pad.layers[0].is_some());
        assert!(pad.layers[1..].iter().all(|s| s.is_none()));
    }

    #[test]
    fn a_wild_config_is_tamed() {
        let mut pad = Pad::for_note(60);
        let mut cfg = PadConfig::for_key(60);
        cfg.poly = 0;
        cfg.choke = 200;
        cfg.pitch_st = 127;
        cfg.pitch_cents = -120;
        cfg.sustain = 7.0;
        cfg.pan = -3.0;
        pad.set(&cfg, &[]);
        assert_eq!(pad.config.poly, 1);
        assert_eq!(pad.config.choke, 8);
        assert_eq!(pad.config.pitch_st, 48);
        assert_eq!(pad.config.pitch_cents, -50);
        assert_eq!(pad.config.sustain, 1.0);
        assert_eq!(pad.config.pan, -1.0);
    }

    #[test]
    fn an_inverted_velocity_range_is_repaired() {
        let mut pad = Pad::for_note(60);
        let mut layer = PadLayer::from_pcm(pcm(10));
        layer.vel_lo = 100;
        layer.vel_hi = 20;
        pad.set(&PadConfig::for_key(60), &[layer]);
        let slot = pad.layers[0].as_ref().unwrap();
        assert!(slot.vel_hi >= slot.vel_lo);
    }
}
