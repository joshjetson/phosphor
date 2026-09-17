//! The pad table: 88 pads, one per piano key, each holding up to eight
//! layer slots.
//!
//! Everything here is fixed-capacity, because [`Pad::set`] runs on the
//! audio thread when a `MixerCommand` delivers an edit: copying a config
//! and re-pointing eight `Arc`s is real-time safe, growing a `Vec` is not.
//! The clamps are applied here, at delivery, out of the one table in
//! [`phosphor_plugin::sample`] that the session loader and the knobs also
//! read — the voice trusts its region completely, so this is where `end <
//! start`, a trim past the buffer, or an empty buffer is caught.

use std::sync::Arc;

use phosphor_plugin::sample::{
    clamp_config, clamp_layer, trim_region, PadConfig, PadLayer, PadPhrase, SamplePcm,
};

use super::phrase::{PhraseSlot, MAX_PHRASES};

// The pad geometry lives in the interface crate, because the app addresses
// the same eighty-eight seats and two constants that agree today are two
// constants that disagree the day one of them is edited. Re-exported rather
// than redeclared, so the link is the compiler's and not a comment's.
pub use phosphor_plugin::sample::{pad_index, MAX_LAYERS, NUM_PADS, PAD_BASE_NOTE};

// Shared with [`super::phrase`], which asks the same question of a phrase's
// velocity window that this file asks of a layer's.
pub(crate) use phosphor_plugin::sample::{in_vel_window, repair_vel};

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
    /// Where the shared clamps are applied on the way in. Everything
    /// downstream — the voice's interpolator taps, its edge fades, its
    /// reverse start position — trusts `start < end <= frames` completely,
    /// so an empty buffer stops here. Audition shares this door for the same
    /// reason: a preview that clamped its own region differently would be a
    /// second set of rules for the same question.
    pub fn from_layer(layer: &PadLayer) -> Option<Self> {
        let (start, end) = trim_region(&layer.pcm, layer.start_frame, layer.end_frame)?;
        // By value, so the `Arc` this clones is the one the slot keeps —
        // one refcount bump for the whole delivery, as before.
        let layer = clamp_layer(layer.clone());
        Some(Self {
            pcm: layer.pcm,
            gain: layer.gain,
            pan: layer.pan,
            tune_st: layer.tune_st,
            tune_cents: layer.tune_cents,
            start: start as f64,
            end: end as f64,
            reverse: layer.reverse,
            mute: layer.mute,
            vel_lo: layer.vel_lo,
            vel_hi: layer.vel_hi,
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
    /// Which layer slots have already taken their turn this lap, one bit
    /// each. Round robin's whole state, in a byte.
    ///
    /// A used-set rather than a counter, and the difference is the pad that
    /// has velocity-split layers *and* cycles. A counter would take
    /// `hits % answering` and hand the same two layers out forever when soft
    /// and hard hits alternate, because each kind of hit would be advancing
    /// the other's place. A set asks a smaller question — has this layer had
    /// its turn yet — and both halves of the split rotate properly, while a
    /// pad with no split rotates in plain order because the lowest unused
    /// layer is the next one along.
    ///
    /// It lives on the pad rather than beside the voices because the rotation
    /// is the *pad's* memory, the way it is on a hardware sampler: the
    /// transport stopping does not put it back to the first layer, and
    /// neither does editing the pad — a trim nudge in keys mode re-sends
    /// eighty-eight pads, and a rotation that restarted on every one of them
    /// would never leave layer one. Only [`Pad::rewind_cycle`] does, and only
    /// the panic key and a fresh instrument reach that.
    cycle_used: u8,
}

impl Pad {
    pub fn for_note(note: u8) -> Self {
        Self {
            config: PadConfig::for_key(note),
            layers: std::array::from_fn(|_| None),
            phrases: std::array::from_fn(|_| None),
            cycle_used: 0,
        }
    }

    /// Replace this pad's config and layers with a sanitized copy.
    ///
    /// Runs on the audio thread: no allocation, `Arc` clones and drops
    /// only. A layer whose buffer is empty is dropped entirely — a voice
    /// must never be pointed at zero frames.
    pub fn set(&mut self, config: &PadConfig, layers: &[PadLayer]) {
        self.config = clamp_config(*config);
        fill_slots(&mut self.layers, layers, LayerSlot::from_layer);
    }

    /// Which layer slots answer a hit at `vel` — every one of them when the
    /// pad stacks, exactly one when it cycles.
    ///
    /// A bitmask rather than a list because it is read inside the note-on
    /// loop and eight bits are one register. A cycling pad takes its turn out
    /// of the layers that *answer* this hit, so a muted layer and a layer
    /// outside the velocity window both lose their turn rather than spending
    /// it on silence — round robin on a velocity-split pad is two rotations,
    /// not one with holes in it.
    ///
    /// Advances the rotation, so it is asked exactly once per hit.
    pub fn answering(&mut self, vel: u8) -> u8 {
        let mut answering = 0u8;
        for (i, slot) in self.layers.iter().enumerate() {
            if slot.as_ref().is_some_and(|s| s.answers(vel)) {
                answering |= 1 << i;
            }
        }
        if !self.config.cycle || answering == 0 {
            return answering;
        }
        // Whoever has not had a turn yet. When they all have, the lap is over
        // and a fresh one starts — with this hit, not with the next, or a
        // cycling pad would go silent once per lap.
        let mut left = answering & !self.cycle_used;
        if left == 0 {
            self.cycle_used = 0;
            left = answering;
        }
        let next = 1u8 << left.trailing_zeros();
        self.cycle_used |= next;
        next
    }

    /// Put the rotation back to the first layer. The panic key's reach, and
    /// a fresh instrument's — see [`Pad::cycle_used`].
    pub fn rewind_cycle(&mut self) {
        self.cycle_used = 0;
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
