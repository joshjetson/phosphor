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

use phosphor_plugin::sample::{PadConfig, PadLayer, PadPhrase, SamplePcm};

use super::phrase::{PhraseSlot, MAX_PHRASES};

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

/// Repair a delivered velocity window.
///
/// Inverted ranges are the interesting case: a UI that lets a player drag
/// the low edge past the high one would otherwise deliver a sound that can
/// never be heard, so `hi` is lifted to `lo` rather than the pair being
/// refused. Shared by layers and phrases because a velocity window means the
/// same thing to both, and two repairs would eventually disagree.
pub(crate) fn repair_vel(lo: u8, hi: u8) -> (u8, u8) {
    let lo = lo.min(127);
    (lo, hi.min(127).max(lo))
}

/// Whether a hit at `vel` falls inside a repaired window, inclusive.
#[inline]
pub(crate) fn in_vel_window(lo: u8, hi: u8, vel: u8) -> bool {
    vel >= lo && vel <= hi
}

/// Copy a delivered list into fixed slots, sanitizing each entry and
/// clearing the slots past its end.
///
/// The one place the delivery rule lives: extras past the cap are dropped
/// rather than trusted, and a slot the new list does not reach is emptied
/// rather than left holding what used to be there. Both halves matter —
/// forgetting the second is how a pad keeps playing a layer the player
/// deleted.
fn fill_slots<T, S>(slots: &mut [Option<S>], src: &[T], sanitize: impl Fn(&T) -> Option<S>) {
    for (slot, item) in slots.iter_mut().zip(src.iter()) {
        *slot = sanitize(item);
    }
    let kept = src.len().min(slots.len());
    for slot in slots.iter_mut().skip(kept) {
        *slot = None;
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

impl LayerSlot {
    /// Sanitize a delivered layer into one a voice can be pointed at, or
    /// `None` when there is nothing playable behind it.
    ///
    /// The one place a trim is clamped. Everything downstream — the voice's
    /// interpolator taps, its edge fades, its reverse start position —
    /// trusts `start < end <= frames` completely, so `end < start`, a trim
    /// past the buffer and an empty buffer are all caught here or not at
    /// all. Audition shares it for that reason: a preview that clamped its
    /// own region differently would be a second set of rules for the same
    /// question.
    pub fn from_layer(layer: &PadLayer) -> Option<Self> {
        let frames = layer.pcm.frames();
        if frames == 0 {
            return None;
        }
        let start = layer.start_frame.min(frames - 1);
        let end = layer.end_frame.clamp(start + 1, frames);
        let (vel_lo, vel_hi) = repair_vel(layer.vel_lo, layer.vel_hi);
        Some(Self {
            pcm: Arc::clone(&layer.pcm),
            gain: layer.gain.clamp(0.0, 4.0),
            pan: layer.pan.clamp(-1.0, 1.0),
            tune_st: layer.tune_st.clamp(-48, 48),
            tune_cents: layer.tune_cents.clamp(-50, 50),
            start: start as f64,
            end: end as f64,
            reverse: layer.reverse,
            mute: layer.mute,
            vel_lo,
            vel_hi,
        })
    }

    /// Everything a voice needs to fire this layer.
    pub fn trigger(&self) -> super::voice::LayerTrigger<'_> {
        super::voice::LayerTrigger {
            pcm: &self.pcm,
            gain: self.gain,
            pan: self.pan,
            tune_st: self.tune_st,
            tune_cents: self.tune_cents,
            start: self.start,
            end: self.end,
            reverse: self.reverse,
        }
    }

    /// Whether this layer answers a hit at `vel`. A muted layer answers
    /// nothing; a velocity-switched one answers only its own range.
    pub fn answers(&self, vel: u8) -> bool {
        !self.mute && in_vel_window(self.vel_lo, self.vel_hi, vel)
    }
}

pub(crate) struct Pad {
    pub config: PadConfig,
    pub layers: [Option<LayerSlot>; MAX_LAYERS],
    /// The pad's phrase layers, beside its sampled ones. A pad can carry
    /// both: a sampled kick under a phrase that plays the bassline, on one
    /// key, is the arrangement the feature exists for.
    pub phrases: [Option<PhraseSlot>; MAX_PHRASES],
}

impl Pad {
    pub fn for_note(note: u8) -> Self {
        Self {
            config: PadConfig::for_key(note),
            layers: std::array::from_fn(|_| None),
            phrases: std::array::from_fn(|_| None),
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

        fill_slots(&mut self.layers, layers, LayerSlot::from_layer);
    }

    /// Replace this pad's phrase layers with a sanitized copy.
    ///
    /// Separate from [`Pad::set`] because the two arrive separately: a pad's
    /// sampled layers and its phrases are edited by different parts of the
    /// UI, and a phrase edit that had to re-send the pad's config would make
    /// every phrase edit a chance to overwrite a pad setting with a stale
    /// one. Same delivery rule, same audio thread, no allocation.
    pub fn set_phrases(&mut self, phrases: &[PadPhrase]) {
        fill_slots(&mut self.phrases, phrases, PhraseSlot::from_phrase);
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

    fn phrase(events: usize, frames: u64) -> PadPhrase {
        let list: Vec<phosphor_plugin::sample::PhraseEvent> = (0..events)
            .map(|i| phosphor_plugin::sample::PhraseEvent {
                frame: i as u64 * 100,
                status: 0x90,
                data1: 60,
                data2: 100,
            })
            .collect();
        PadPhrase::from_events(Arc::from(list), frames)
    }

    #[test]
    fn a_phrase_with_no_events_never_reaches_a_runner() {
        let mut pad = Pad::for_note(60);
        pad.set_phrases(&[PadPhrase::from_events(Arc::from(Vec::new()), 44_100)]);
        assert!(pad.phrases[0].is_none());
    }

    #[test]
    fn a_fifth_phrase_is_dropped_and_old_slots_are_cleared() {
        let mut pad = Pad::for_note(60);
        let five: Vec<PadPhrase> = (0..5).map(|_| phrase(2, 1_000)).collect();
        pad.set_phrases(&five);
        assert!(pad.phrases.iter().all(Option::is_some));

        pad.set_phrases(&[phrase(2, 1_000)]);
        assert!(pad.phrases[0].is_some());
        assert!(pad.phrases[1..].iter().all(Option::is_none), "a deleted phrase survived");
    }

    #[test]
    fn a_phrase_is_never_shorter_than_the_notes_in_it() {
        let mut pad = Pad::for_note(60);
        // Events out to frame 400, a length of 10 claimed: the runner would
        // otherwise end before its own last note-off.
        pad.set_phrases(&[phrase(5, 10)]);
        let slot = pad.phrases[0].as_ref().unwrap();
        assert!(slot.length() > 400, "phrase length {} cuts its own events", slot.length());
    }

    #[test]
    fn a_wild_phrase_is_tamed() {
        let mut pad = Pad::for_note(60);
        let mut p = phrase(2, 1_000);
        p.gain = f32::NAN;
        p.vel_lo = 200;
        p.vel_hi = 3;
        pad.set_phrases(&[p]);
        let slot = pad.phrases[0].as_ref().unwrap();
        // A NaN gain multiplies every velocity into a NaN, which rounds to
        // zero and silently drops the phrase's notes — so it is caught here.
        assert_eq!(slot.velocity_scale(), 0.0);
        assert!(slot.answers(127), "a repaired window answers its own edge");
        assert!(!slot.answers(0));
    }

    #[test]
    fn a_muted_phrase_answers_nothing() {
        let mut pad = Pad::for_note(60);
        let mut p = phrase(2, 1_000);
        p.mute = true;
        pad.set_phrases(&[p]);
        let slot = pad.phrases[0].as_ref().unwrap();
        for vel in [1u8, 64, 127] {
            assert!(!slot.answers(vel), "a muted phrase answered velocity {vel}");
        }
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
