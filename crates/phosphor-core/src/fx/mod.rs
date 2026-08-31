//! The insert-effect layer: what an effect is, and the chain that runs six.
//!
//! Nothing in here makes a sound. It is the road the effects drive on: the
//! trait they implement, the context they are handed, the six-slot chain that
//! runs them in order, and the click-free bypass every one of them inherits
//! without writing a line of it.
//!
//! Three rules hold everywhere below, and they are the reason the shapes look
//! the way they do:
//!
//! * **Nothing here allocates while audio is running.** A chain is built with
//!   room for its six slots; a bypass crossfade borrows a scratch buffer the
//!   mixer already owns rather than keeping one per track.
//! * **The dry path is untouched, not multiplied by one.** A bypassed slot
//!   returns without reading the buffer at all, so "bypass is bit-identical"
//!   is a property of the control flow rather than of floating-point luck.
//! * **Parameters are in natural units** — decibels, hertz, milliseconds —
//!   not in a 0..1 knob fraction. A session stores what a control *meant*, so
//!   a range that changes later cannot silently re-point it, which is the
//!   defect the instruments' `discrete` table exists to work around.

use std::sync::Arc;

mod gain;
mod meter;

pub use gain::Gain;
pub use meter::{GrBallistics, GrMeter};

/// How many effects one chain holds.
///
/// Tracks, buses and the master all get the same six. The number is a UI
/// decision as much as an audio one — six slots is what fits on a track's
/// panel without scrolling — and the audio thread enforces it so that a UI
/// that forgets to cannot grow a `Vec` inside the callback.
pub const MAX_FX_SLOTS: usize = 6;

/// How long a bypass switch takes to cross over, 8 ms.
///
/// Inside the 5–10 ms window that is short enough to feel instant and long
/// enough that the step in level lands below the ear's click threshold. It is
/// a *time*, not a sample count: at 96 kHz the fade is 768 samples and at
/// 44.1 kHz it is 353, so the switch sounds the same on every device.
pub const BYPASS_FADE_SECONDS: f32 = 0.008;

// ── Addressing ──

/// Which insert chain a command is about.
///
/// An enum rather than a track id with reserved values: the buses and the
/// master are not tracks, they do not live in the track list, and a `usize`
/// that sometimes means "track 4" and sometimes "the master" is the kind of
/// stringly-typed addressing that goes wrong once and then silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FxTarget {
    /// The insert chain on the track with this id.
    Track(usize),
    /// The chain on send bus A.
    BusA,
    /// The chain on send bus B.
    BusB,
    /// The chain on the master bus, ahead of the safety limiter.
    Master,
}

/// Which of the two send buses a level is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SendSlot {
    A,
    B,
}

impl SendSlot {
    /// The two, in the order they are drawn.
    pub const ALL: [SendSlot; 2] = [SendSlot::A, SendSlot::B];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
        }
    }
}

// ── Levels ──

/// The linear gain a decibel value names. `-inf` and anything at or below
/// [`SILENT_DB`] come back as exactly zero, so "off" is off rather than
/// −120 dB of denormals.
#[must_use]
pub fn db_to_gain(db: f32) -> f32 {
    if db <= SILENT_DB {
        0.0
    } else {
        10.0f32.powf(db / 20.0)
    }
}

/// The decibel value a linear gain names. Zero comes back as [`SILENT_DB`]
/// rather than as negative infinity, which JSON cannot store and no meter can
/// draw.
#[must_use]
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        SILENT_DB
    } else {
        20.0 * gain.log10()
    }
}

/// The bottom of every level control in the FX layer: below this, silence.
pub const SILENT_DB: f32 = -60.0;

/// The pair of gains a pan position asks for, left then right.
///
/// Equal power: `l² + r²` is the same at every position, so sweeping a source
/// across the image does not change how loud it is. The two ends are the two
/// channels at full travel and the centre sits 3.01 dB below them, which is
/// the shape every constant-power law has.
///
/// **Where the reference sits is a deliberate deviation.** The usual
/// spelling of this law puts the centre at −3 dB and the extremes at 0 dB;
/// this one puts the centre at 0 dB and the extremes at +3 dB. The difference
/// between the two is one constant, and it is the difference between an
/// existing session rendering exactly as it did before this layer was built
/// and every existing session becoming 3.01 dB quieter the day it lands. The
/// mixer's own tests assert the first — `fader_scales_the_track` expects a
/// track at unity to leave the mixer at unity — and a pan law that is a
/// mix-wide trim is a worse thing to be silently right about than one whose
/// hard-panned extreme asks the master limiter for 3 dB it was already built
/// to give.
///
/// Exactly `(1.0, 1.0)` at the centre, and that is load-bearing rather than
/// approximate: the arithmetic is done in `f64` and rounded once, so an
/// unpanned track is multiplied by one and a session written before pan
/// existed renders sample for sample as it did.
#[must_use]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = if pan.is_nan() { 0.0 } else { pan.clamp(-1.0, 1.0) };
    // θ sweeps a quarter turn: 0 at hard left, π/4 at centre, π/2 at hard
    // right. The √2 is what moves the reference point from the extremes to
    // the centre — see above.
    let theta = (f64::from(pan) + 1.0) * std::f64::consts::FRAC_PI_4;
    let norm = std::f64::consts::SQRT_2;
    ((norm * theta.cos()) as f32, (norm * theta.sin()) as f32)
}

// ── What an effect is ──

/// One parameter of an effect, in the units the control actually has.
///
/// `Copy` and `&'static str`: reading a parameter's name happens while the
/// UI is drawing, sixty times a second, and a `String` per parameter per
/// frame is an allocation storm for text that never changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FxParamInfo {
    /// Short name, as it appears on the panel.
    pub name: &'static str,
    /// The unit the value is in — `"dB"`, `"Hz"`, `"ms"`, `"%"`, or empty.
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// What an effect is told about the block it is about to render.
///
/// Handed by reference, built once per chain per callback. A compressor that
/// keys off another track and a delay that follows the tempo are both
/// impossible without this, so it exists from the first slot rather than
/// being retrofitted once an effect needs it.
///
/// `key` is the other track's signal **as the instrument produced it** —
/// post-instrument, pre-insert — and it is always the same block this call is
/// rendering, never the previous one. That is what the mixer's two passes
/// buy: every key is same-block and the answer does not depend on which order
/// the tracks happen to sit in.
#[derive(Clone, Copy)]
pub struct FxContext<'a> {
    /// Samples per second.
    pub sample_rate: f32,
    /// The transport's tempo, read once for this block.
    pub tempo_bpm: f32,
    /// Whether the transport is rolling.
    pub playing: bool,
    /// The resolved sidechain key, left and right, or `None` when this chain
    /// asked for none or the track it named is gone. Both slices are exactly
    /// as long as the block being rendered.
    pub key: Option<(&'a [f32], &'a [f32])>,
}

impl FxContext<'_> {
    /// A context with no key and no transport, for tests and for effects
    /// rendered outside the mixer.
    #[must_use]
    pub fn bare(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            tempo_bpm: 120.0,
            playing: false,
            key: None,
        }
    }
}

/// An audio effect in an insert slot.
///
/// Deliberately not [`phosphor_plugin::Plugin`]. An instrument is handed MIDI
/// and writes into empty buffers; an effect is handed audio and rewrites it
/// in place, needs the tempo and the sidechain, and has no notion of a voice.
/// Sharing one trait would mean every instrument carrying a `latency()` it
/// does not have and every effect a `parameter_info` shaped for a knob
/// fraction it does not use.
///
/// **Real-time contract.** `process` and `reset` run on the audio thread:
/// no allocation, no locks, no logging, no unbounded loops. Everything an
/// effect needs is built in [`Effect::init`], which is called before the
/// effect is put in a slot.
pub trait Effect: Send {
    /// The stable name this effect is stored under in a session file.
    ///
    /// It is an identifier, not a label: renaming it orphans every saved
    /// chain that contains one.
    fn name(&self) -> &'static str;

    /// Build everything. Called once, off the audio thread's critical path,
    /// before the effect reaches a slot.
    fn init(&mut self, sample_rate: f64, max_buffer_size: usize);

    /// Rewrite one block in place. Real-time: see the trait's contract.
    ///
    /// `left` and `right` are the same length, and no longer than the
    /// `max_buffer_size` handed to [`Effect::init`].
    fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &FxContext<'_>);

    /// Drop every tail: delay lines to silence, detectors to rest. Real-time.
    fn reset(&mut self);

    fn parameter_count(&self) -> usize;

    /// What parameter `index` is, or `None` if there is no such parameter.
    fn parameter_info(&self, index: usize) -> Option<FxParamInfo>;

    /// The current value of a parameter, in its natural unit.
    fn get_parameter(&self, index: usize) -> f32;

    /// Set a parameter, in its natural unit. Out-of-range values are the
    /// effect's to clamp; an unknown index is ignored.
    fn set_parameter(&mut self, index: usize, value: f32);

    /// Whether this effect reads [`FxContext::key`]. Resolving a key costs a
    /// lookup, so the mixer only does it for chains that say yes.
    fn wants_key(&self) -> bool {
        false
    }

    /// The meter this effect publishes its gain reduction to, if it reduces
    /// gain at all.
    ///
    /// A method on the trait rather than a downcast, because gain reduction
    /// is a first-class thing in this layer: the master limiter already
    /// publishes one, the UI already draws one, and an effect that takes
    /// level off should be able to say so without the front end knowing which
    /// effect it is holding. The `Arc` is cloned once when the effect is
    /// built, never while audio is running.
    fn gr_meter(&self) -> Option<Arc<GrMeter>> {
        None
    }

    /// Samples of delay this effect adds. Zero for everything built so far,
    /// and the reason there is no delay compensation to go with it: nothing
    /// in the box has any latency to compensate.
    fn latency(&self) -> usize {
        0
    }
}

// ── A slot ──

/// One effect and the bypass switch in front of it.
pub struct FxSlot {
    effect: Box<dyn Effect>,
    /// Where the switch is. The audible state is [`FxSlot::fade`], which
    /// walks towards this.
    bypass: bool,
    /// How much of the effect is in the signal: 1.0 fully wet, 0.0 fully dry.
    fade: f32,
    /// How far `fade` moves per sample. Derived from the sample rate, so the
    /// crossfade lasts [`BYPASS_FADE_SECONDS`] at any rate.
    fade_step: f32,
}

impl FxSlot {
    fn new(effect: Box<dyn Effect>, sample_rate: f32) -> Self {
        Self {
            effect,
            bypass: false,
            fade: 1.0,
            fade_step: 1.0 / (BYPASS_FADE_SECONDS * sample_rate.max(1.0)),
        }
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        self.effect.name()
    }

    #[must_use]
    pub fn is_bypassed(&self) -> bool {
        self.bypass
    }

    /// Whether the crossfade has finished — a steady state, wet or dry.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        if self.bypass {
            self.fade <= 0.0
        } else {
            self.fade >= 1.0
        }
    }

    #[must_use]
    pub fn effect(&self) -> &dyn Effect {
        self.effect.as_ref()
    }

    #[must_use]
    pub fn effect_mut(&mut self) -> &mut dyn Effect {
        self.effect.as_mut()
    }

    /// Run this slot over a block.
    ///
    /// Three paths, and the first two are the whole point:
    ///
    /// * **Settled wet** — the effect is handed the buffer and nothing else
    ///   happens to it. No crossfade arithmetic, so a chain of one effect is
    ///   exactly that effect.
    /// * **Settled dry** — the buffer is not read or written at all. Bypass
    ///   is bit-identical because the samples are never touched, not because
    ///   a multiply by one happened to round back to where it started.
    /// * **Crossfading** — the dry signal is copied aside, the effect runs,
    ///   and the two are mixed sample by sample. Linear, because the two ends
    ///   have to land exactly on the steady states above.
    fn process(&mut self, left: &mut [f32], right: &mut [f32], ctx: &FxContext<'_>, dry: &mut FxScratch) {
        if self.is_settled() {
            if !self.bypass {
                self.effect.process(left, right, ctx);
            }
            return;
        }

        let frames = left.len().min(right.len());
        let (dry_l, dry_r) = dry.slices(frames);
        dry_l.copy_from_slice(&left[..frames]);
        dry_r.copy_from_slice(&right[..frames]);

        self.effect.process(left, right, ctx);

        let step = if self.bypass { -self.fade_step } else { self.fade_step };
        let mut fade = self.fade;
        for i in 0..frames {
            fade = (fade + step).clamp(0.0, 1.0);
            left[i] = left[i] * fade + dry_l[i] * (1.0 - fade);
            right[i] = right[i] * fade + dry_r[i] * (1.0 - fade);
        }
        self.fade = fade;

        // The switch has finished closing: drop the tail rather than freezing
        // it. A reverb that is switched back on a bar later starts from
        // silence instead of resuming a stale tail mid-decay, which is what
        // "bypass is hard" means.
        if self.bypass && self.fade <= 0.0 {
            self.effect.reset();
        }
    }
}

// ── Scratch ──

/// The dry copy a bypass crossfade needs.
///
/// One per mixer rather than one per chain: only one slot is ever mid-fade
/// inside a single `process` call, because the slots run one after another.
/// Sixty-four tracks each holding two spare buffers would be a quarter of a
/// megabyte to make that fact invisible.
pub struct FxScratch {
    l: Vec<f32>,
    r: Vec<f32>,
}

impl FxScratch {
    #[must_use]
    pub fn new(max_buffer_size: usize) -> Self {
        Self {
            l: vec![0.0; max_buffer_size],
            r: vec![0.0; max_buffer_size],
        }
    }

    /// Two buffers of exactly `frames` samples.
    ///
    /// Grows only if a device hands the callback a block larger than the
    /// maximum it promised — the same deliberately-dead branch the mixer's
    /// own buffers carry, for the same reason.
    fn slices(&mut self, frames: usize) -> (&mut [f32], &mut [f32]) {
        if self.l.len() < frames {
            self.l.resize(frames, 0.0);
            self.r.resize(frames, 0.0);
        }
        (&mut self.l[..frames], &mut self.r[..frames])
    }
}

// ── The chain ──

/// Six insert slots, run in order.
///
/// Order is the chain's whole meaning — a compressor before an EQ is a
/// different sound from an EQ before a compressor — so the slots are a
/// sequence and moving one is an explicit operation rather than a sort.
pub struct FxChain {
    slots: Vec<FxSlot>,
    sample_rate: f32,
}

impl FxChain {
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            // Built to its cap: pushing a sixth effect must not reallocate on
            // the audio thread, and a seventh is refused rather than grown.
            slots: Vec::with_capacity(MAX_FX_SLOTS),
            sample_rate: sample_rate.max(1) as f32,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    #[must_use]
    pub fn is_full(&self) -> bool {
        self.slots.len() >= MAX_FX_SLOTS
    }

    #[must_use]
    pub fn slot(&self, index: usize) -> Option<&FxSlot> {
        self.slots.get(index)
    }

    pub fn slots(&self) -> impl Iterator<Item = &FxSlot> {
        self.slots.iter()
    }

    /// Put an effect in at `index`, pushing the slots after it along.
    ///
    /// Returns the box back when the chain is full, so the caller decides
    /// what happens to it — on the audio thread that is a drop, and dropping
    /// it here rather than silently keeping it is what makes the cap real.
    pub fn insert(&mut self, index: usize, effect: Box<dyn Effect>) -> Option<Box<dyn Effect>> {
        if self.is_full() {
            return Some(effect);
        }
        let index = index.min(self.slots.len());
        self.slots.insert(index, FxSlot::new(effect, self.sample_rate));
        None
    }

    /// Take the effect out of a slot. The box comes back so the caller can
    /// decide where it is dropped.
    pub fn remove(&mut self, index: usize) -> Option<Box<dyn Effect>> {
        (index < self.slots.len()).then(|| self.slots.remove(index).effect)
    }

    /// Move one slot to another position, sliding the rest.
    pub fn move_slot(&mut self, from: usize, to: usize) -> bool {
        if from >= self.slots.len() || to >= self.slots.len() || from == to {
            return false;
        }
        let slot = self.slots.remove(from);
        self.slots.insert(to, slot);
        true
    }

    pub fn set_parameter(&mut self, index: usize, param: usize, value: f32) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.effect.set_parameter(param, value);
        }
    }

    pub fn set_bypass(&mut self, index: usize, bypass: bool) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.bypass = bypass;
        }
    }

    /// Whether any slot reads the sidechain key.
    #[must_use]
    pub fn wants_key(&self) -> bool {
        self.slots.iter().any(|s| s.effect.wants_key())
    }

    /// The gain-reduction meter of the effect in a slot, if it has one.
    #[must_use]
    pub fn gr_meter(&self, index: usize) -> Option<Arc<GrMeter>> {
        self.slots.get(index).and_then(|s| s.effect.gr_meter())
    }

    /// Total latency of the chain. Zero today; the place a delay compensator
    /// would read.
    #[must_use]
    pub fn latency(&self) -> usize {
        self.slots.iter().map(|s| s.effect.latency()).sum()
    }

    /// Run every slot, in order.
    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        ctx: &FxContext<'_>,
        scratch: &mut FxScratch,
    ) {
        for slot in &mut self.slots {
            slot.process(left, right, ctx, scratch);
        }
    }

    /// Drop every tail in the chain, and land every bypass switch where it
    /// was heading. Called by the panic path.
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.effect.reset();
            slot.fade = if slot.bypass { 0.0 } else { 1.0 };
        }
    }

    /// The name of the first effect in the chain, which is what a bus is
    /// labelled by on the track strip.
    #[must_use]
    pub fn first_name(&self) -> Option<&'static str> {
        self.slots.first().map(FxSlot::name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An effect that adds a constant, so a test can see exactly which
    /// samples it touched.
    struct AddConst(f32);

    impl Effect for AddConst {
        fn name(&self) -> &'static str {
            "addconst"
        }
        fn init(&mut self, _sample_rate: f64, _max_buffer_size: usize) {}
        fn process(&mut self, left: &mut [f32], right: &mut [f32], _ctx: &FxContext<'_>) {
            for s in left.iter_mut().chain(right.iter_mut()) {
                *s += self.0;
            }
        }
        fn reset(&mut self) {}
        fn parameter_count(&self) -> usize {
            1
        }
        fn parameter_info(&self, _index: usize) -> Option<FxParamInfo> {
            None
        }
        fn get_parameter(&self, _index: usize) -> f32 {
            self.0
        }
        fn set_parameter(&mut self, _index: usize, value: f32) {
            self.0 = value;
        }
    }

    fn chain_with(effects: Vec<Box<dyn Effect>>) -> FxChain {
        let mut chain = FxChain::new(44_100);
        for e in effects {
            assert!(chain.insert(chain.len(), e).is_none());
        }
        chain
    }

    fn run(chain: &mut FxChain, l: &mut [f32], r: &mut [f32]) {
        let mut scratch = FxScratch::new(l.len());
        chain.process(l, r, &FxContext::bare(44_100.0), &mut scratch);
    }

    #[test]
    fn an_empty_chain_is_a_wire() {
        let mut chain = FxChain::new(44_100);
        let source: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut l = source.clone();
        let mut r = source.clone();
        run(&mut chain, &mut l, &mut r);
        for (i, (a, b)) in source.iter().zip(&l).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "sample {i} changed");
        }
        assert_eq!(r, source);
    }

    #[test]
    fn slots_run_in_order_and_move() {
        // Two effects that are not commutative under `move`: a chain of
        // add-1 then add-10 sums to 11 either way, so use the ordering of
        // names to prove the move rather than the arithmetic.
        let mut chain = chain_with(vec![Box::new(AddConst(1.0)), Box::new(AddConst(10.0))]);
        let mut l = vec![0.0; 4];
        let mut r = vec![0.0; 4];
        run(&mut chain, &mut l, &mut r);
        assert_eq!(l, vec![11.0; 4]);

        assert_eq!(chain.slot(0).unwrap().effect().get_parameter(0), 1.0);
        assert!(chain.move_slot(0, 1));
        assert_eq!(chain.slot(0).unwrap().effect().get_parameter(0), 10.0);
        assert_eq!(chain.slot(1).unwrap().effect().get_parameter(0), 1.0);
    }

    /// The cap is enforced where the memory is: the audio thread. A seventh
    /// effect comes back to the caller rather than growing the slot list.
    #[test]
    fn a_seventh_effect_is_refused() {
        let mut chain = FxChain::new(44_100);
        let capacity = chain.slots.capacity();
        for _ in 0..MAX_FX_SLOTS {
            assert!(chain.insert(chain.len(), Box::new(AddConst(0.0))).is_none());
        }
        assert!(chain.is_full());
        assert!(chain.insert(6, Box::new(AddConst(0.0))).is_some(), "the cap did not hold");
        assert_eq!(chain.len(), MAX_FX_SLOTS);
        assert_eq!(chain.slots.capacity(), capacity, "the slot list reallocated");
    }

    /// A bypassed slot does not read or write the buffer, so the dry path is
    /// the samples themselves rather than a copy of them.
    #[test]
    fn a_settled_bypass_is_bit_identical() {
        let mut chain = chain_with(vec![Box::new(AddConst(0.25))]);
        chain.set_bypass(0, true);
        // Land the switch: one block long enough to finish the crossfade.
        let mut warm_l = vec![0.0; 1024];
        let mut warm_r = vec![0.0; 1024];
        run(&mut chain, &mut warm_l, &mut warm_r);
        assert!(chain.slot(0).unwrap().is_settled());

        let source: Vec<f32> = (0..64).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut l = source.clone();
        let mut r = source.clone();
        run(&mut chain, &mut l, &mut r);
        for (i, (a, b)) in source.iter().zip(&l).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "bypassed slot altered sample {i}");
        }
        assert_eq!(r, source);
    }

    /// ...and the wet steady state is the effect and nothing else.
    #[test]
    fn a_settled_wet_slot_is_the_effect_alone() {
        let mut chain = chain_with(vec![Box::new(AddConst(0.25))]);
        let source: Vec<f32> = (0..64).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut l = source.clone();
        let mut r = source.clone();
        run(&mut chain, &mut l, &mut r);
        for (i, (a, b)) in source.iter().zip(&l).enumerate() {
            assert_eq!((a + 0.25).to_bits(), b.to_bits(), "sample {i}");
        }
    }

    /// The switch is a fade, not a step. The largest sample-to-sample jump
    /// across the transition has to stay near the size of the step the fade
    /// takes, which is what makes it inaudible.
    #[test]
    fn bypass_crossfades_rather_than_stepping() {
        let sample_rate = 44_100.0f32;
        let mut chain = chain_with(vec![Box::new(AddConst(0.5))]);
        let mut l = vec![0.0f32; 64];
        let mut r = vec![0.0f32; 64];

        // Wet: every sample sits at 0.5.
        run(&mut chain, &mut l, &mut r);
        assert!((l[63] - 0.5).abs() < 1.0e-6);

        chain.set_bypass(0, true);
        let mut trail = Vec::new();
        let mut previous = l[63];
        for _ in 0..16 {
            l.fill(0.0);
            r.fill(0.0);
            run(&mut chain, &mut l, &mut r);
            trail.extend_from_slice(&l);
        }
        let mut worst_jump = 0.0f32;
        for &s in &trail {
            worst_jump = worst_jump.max((s - previous).abs());
            previous = s;
        }
        // One fade step is 0.5 * (1 / (0.008 * 44100)) of level.
        let step = 0.5 / (BYPASS_FADE_SECONDS * sample_rate);
        assert!(
            worst_jump <= step * 1.5,
            "bypass stepped by {worst_jump:.6}, more than the {step:.6} a fade takes"
        );
        assert_eq!(trail.last().copied(), Some(0.0), "the fade never reached dry");
    }

    /// **Click energy.** A switch is a click when the waveform has a step in
    /// it, and the size of a click is the size of that step. Measured against
    /// the switch this replaced — a hard cut from wet to dry — the crossfade
    /// has to be orders of magnitude quieter, not merely smaller.
    ///
    /// The measure is the energy in the first difference of the signal, which
    /// is what a step puts there and what the ear hears as a click.
    #[test]
    fn the_crossfade_has_a_fraction_of_a_hard_switch_s_click_energy() {
        /// A tone, so that there is a waveform for the switch to interrupt.
        struct Tone {
            phase: f32,
        }
        impl Effect for Tone {
            fn name(&self) -> &'static str {
                "tone"
            }
            fn init(&mut self, _sr: f64, _mb: usize) {}
            fn process(&mut self, left: &mut [f32], right: &mut [f32], _c: &FxContext<'_>) {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    // A quarter of full scale of 440 Hz at 44.1 kHz, added to
                    // whatever is there: the effect is a difference, so the
                    // dry and wet signals are not the same waveform.
                    self.phase += 2.0 * std::f32::consts::PI * 440.0 / 44_100.0;
                    let s = 0.25 * self.phase.sin();
                    *l += s;
                    *r += s;
                }
            }
            fn reset(&mut self) {}
            fn parameter_count(&self) -> usize {
                0
            }
            fn parameter_info(&self, _i: usize) -> Option<FxParamInfo> {
                None
            }
            fn get_parameter(&self, _i: usize) -> f32 {
                0.0
            }
            fn set_parameter(&mut self, _i: usize, _v: f32) {}
        }

        /// The largest step between one sample and the next.
        fn worst_step(samples: &[f32]) -> f32 {
            samples
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max)
        }

        // The waveform's own slope: how far it moves between samples when
        // nothing is being switched. Anything the switch adds on top of this
        // is what the ear hears as a click.
        let mut chain = chain_with(vec![Box::new(Tone { phase: 0.0 })]);
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        run(&mut chain, &mut l, &mut r);
        let natural = worst_step(&l);
        assert!(natural > 0.0, "the tone is not a waveform");

        // Crossfaded: the switch thrown, then long enough to land.
        chain.set_bypass(0, true);
        let mut faded = Vec::new();
        for _ in 0..16 {
            l.fill(0.0);
            r.fill(0.0);
            let mut short_l = l[..64].to_vec();
            let mut short_r = r[..64].to_vec();
            run(&mut chain, &mut short_l, &mut short_r);
            faded.extend_from_slice(&short_l);
        }

        // Hard: the same transition with the fade taken out, which is what
        // the switch would be without this machinery.
        let mut tone = Tone { phase: 0.0 };
        let ctx = FxContext::bare(44_100.0);
        let mut hard = vec![0.0f32; 64];
        let mut hard_r = vec![0.0f32; 64];
        tone.process(&mut hard, &mut hard_r, &ctx);
        hard.resize(128, 0.0);

        let faded_step = worst_step(&faded);
        let hard_step = worst_step(&hard);
        assert!(
            faded_step < natural * 1.25,
            "the crossfade put a {faded_step:.5} step into a waveform whose own \
             slope is {natural:.5}"
        );
        assert!(
            hard_step > natural * 5.0,
            "the hard switch is supposed to be the bad case, and it stepped \
             {hard_step:.5} against a slope of {natural:.5}"
        );
        assert!(
            faded_step * 5.0 < hard_step,
            "the crossfade stepped {faded_step:.5} against the hard switch's {hard_step:.5}"
        );
    }

    /// The fade is a duration, so it takes the same number of milliseconds at
    /// every rate rather than the same number of samples.
    #[test]
    fn the_crossfade_is_the_same_length_at_every_rate() {
        for rate in [44_100u32, 48_000, 96_000] {
            let mut chain = FxChain::new(rate);
            assert!(chain.insert(0, Box::new(AddConst(1.0))).is_none());
            let mut l = vec![0.0f32; 1];
            let mut r = vec![0.0f32; 1];
            run(&mut chain, &mut l, &mut r);
            chain.set_bypass(0, true);

            let mut samples = 0usize;
            while !chain.slot(0).unwrap().is_settled() && samples < rate as usize {
                l.fill(0.0);
                r.fill(0.0);
                run(&mut chain, &mut l, &mut r);
                samples += 1;
            }
            let seconds = samples as f32 / rate as f32;
            assert!(
                (seconds - BYPASS_FADE_SECONDS).abs() < BYPASS_FADE_SECONDS * 0.05,
                "at {rate} Hz the fade took {seconds:.4}s, not {BYPASS_FADE_SECONDS}s"
            );
        }
    }

    /// Bypass is hard: the tail is dropped when the switch lands, so coming
    /// back on does not resume a stale one.
    #[test]
    fn landing_the_bypass_resets_the_effect() {
        /// Counts how many times it was reset.
        struct Counter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Effect for Counter {
            fn name(&self) -> &'static str {
                "counter"
            }
            fn init(&mut self, _sr: f64, _mb: usize) {}
            fn process(&mut self, _l: &mut [f32], _r: &mut [f32], _c: &FxContext<'_>) {}
            fn reset(&mut self) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            fn parameter_count(&self) -> usize {
                0
            }
            fn parameter_info(&self, _i: usize) -> Option<FxParamInfo> {
                None
            }
            fn get_parameter(&self, _i: usize) -> f32 {
                0.0
            }
            fn set_parameter(&mut self, _i: usize, _v: f32) {}
        }

        let resets = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut chain = chain_with(vec![Box::new(Counter(resets.clone()))]);
        chain.set_bypass(0, true);
        let mut l = vec![0.0f32; 2048];
        let mut r = vec![0.0f32; 2048];
        run(&mut chain, &mut l, &mut r);
        assert_eq!(resets.load(std::sync::atomic::Ordering::Relaxed), 1);

        // ...and only once: a settled bypass does no work at all.
        run(&mut chain, &mut l, &mut r);
        assert_eq!(resets.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    // ── Pan ──

    /// The centre is unity, exactly, in both channels. Everything about an
    /// unpanned session rendering as it did before rests on this.
    #[test]
    fn the_pan_centre_is_exactly_unity() {
        let (l, r) = pan_gains(0.0);
        assert_eq!(l.to_bits(), 1.0f32.to_bits(), "centre left is {l}");
        assert_eq!(r.to_bits(), 1.0f32.to_bits(), "centre right is {r}");
    }

    /// Equal power: the sum of squares is the same everywhere in the travel,
    /// so sweeping does not change how loud the source is.
    #[test]
    fn pan_holds_power_across_the_sweep() {
        let reference = {
            let (l, r) = pan_gains(0.0);
            l * l + r * r
        };
        for step in -100..=100 {
            let pan = step as f32 / 100.0;
            let (l, r) = pan_gains(pan);
            let power = l * l + r * r;
            assert!(
                (power - reference).abs() < 1.0e-5,
                "power at pan {pan} is {power}, not {reference}"
            );
        }
    }

    /// The ends are one channel and silence in the other, and they sit
    /// 3.01 dB above the centre — the shape of a constant-power law.
    #[test]
    fn pan_extremes_are_three_db_above_centre() {
        let (left_l, left_r) = pan_gains(-1.0);
        assert!((left_r).abs() < 1.0e-6, "hard left leaked {left_r} into the right");
        let (right_l, right_r) = pan_gains(1.0);
        assert!((right_l).abs() < 1.0e-6, "hard right leaked {right_l} into the left");

        let (centre, _) = pan_gains(0.0);
        for extreme in [left_l, right_r] {
            let db = 20.0 * (extreme / centre).log10();
            assert!((db - 3.0103).abs() < 0.01, "extreme is {db:.3} dB over centre");
        }
    }

    /// Out of range, and not a number, are positions the audio thread must
    /// survive: a pan gain of NaN multiplies a track into silence.
    #[test]
    fn pan_clamps_its_input() {
        assert_eq!(pan_gains(-4.0), pan_gains(-1.0));
        assert_eq!(pan_gains(4.0), pan_gains(1.0));
        assert_eq!(pan_gains(f32::NAN), pan_gains(0.0));
        for step in -20..=20 {
            let (l, r) = pan_gains(step as f32 / 10.0);
            assert!(l.is_finite() && r.is_finite());
        }
    }

    // ── Levels ──

    #[test]
    fn decibels_round_trip_and_bottom_out() {
        for db in [-40.0f32, -12.0, -6.0, -3.0, 0.0] {
            let back = gain_to_db(db_to_gain(db));
            assert!((back - db).abs() < 1.0e-4, "{db} dB came back as {back}");
        }
        assert_eq!(db_to_gain(SILENT_DB), 0.0);
        assert_eq!(db_to_gain(f32::NEG_INFINITY), 0.0);
        assert_eq!(gain_to_db(0.0), SILENT_DB);
        assert!((db_to_gain(-6.0) - 0.501_187).abs() < 1.0e-5);
        assert_eq!(db_to_gain(0.0), 1.0);
    }
}
