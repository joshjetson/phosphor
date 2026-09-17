//! The sampler: 88 pads, one per piano key, each stacking up to eight
//! layers of shared PCM.
//!
//! The panel parameters here are the *globals* — output level and
//! velocity depth. Everything per-pad arrives whole through
//! [`Plugin::set_sampler_pad`] instead of the parameter system, because
//! 88 pads times a dozen values is not a knob list, it is a data model
//! (the step sequencer's `SetPattern` made the same call).
//!
//! Voice policy, stated once:
//!
//! * `poly` counts **hits**, not voices — a pad with three layers fired
//!   once is one hit. Past the limit the oldest hit is cut with the 3 ms
//!   kill fade while its replacement starts in other slots, which is why
//!   an MPC-style hat sounds tight instead of truncated.
//! * `choke` cuts sounding voices on *other* pads in the same group —
//!   the closed hat stopping the open one. The pad's own retriggers are
//!   poly's business.
//! * At most [`ACTIVE_CAP`] voices count as sounding; the pool holds
//!   [`VOICE_POOL`] so that cut voices have somewhere to finish their
//!   fades. Past the cap the oldest voice anywhere is cut.
//! * A pad's [`phrase`] layers share all of that: they are part of the same
//!   hit, so poly cuts them, a choke stops them, and a gate releases them.
//!   What they do not share is the voice pool — a phrase plays an
//!   instrument, not a buffer.
//! * The UI's audition ([`preview`]) is none of the above — see that
//!   module for why it is deliberately outside all three rules.

mod pad;
mod phrase;
mod preview;
mod voice;

pub use pad::{pad_index, MAX_LAYERS, NUM_PADS, PAD_BASE_NOTE};
pub use phrase::{MAX_PHRASES, PHRASE_RUNNERS};

use std::sync::Arc;

use phosphor_plugin::sample::{PadConfig, PadLayer, PadPhrase, PreviewLayer, TrigMode};
use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;
use pad::Pad;
use phrase::{ChildEvents, ChildHost, RunnerPool, RunnerStart};
use preview::Preview;
use voice::SamplerVoice;

pub const PARAM_COUNT: usize = 2;
pub const P_LEVEL: usize = 0;
pub const P_VEL: usize = 1;

pub const PARAM_NAMES: [&str; PARAM_COUNT] = ["level", "vel"];

/// `level` defaults to 0.8, which the gain mapping places at exactly
/// unity: a sampler plays the user's material at the level it was
/// recorded, with headroom above the default rather than below it.
pub const PARAM_DEFAULTS: [f32; PARAM_COUNT] = [0.8, 0.7];

pub fn is_discrete(_index: usize) -> bool {
    false
}

pub fn step_discrete(_index: usize, value: f32, up: bool) -> f32 {
    let step = if up { 0.05 } else { -0.05 };
    (value + step).clamp(0.0, 1.0)
}

/// Total voice slots, fade-outs included.
const VOICE_POOL: usize = 80;

/// Voices allowed to *sound* at once; the remainder of the pool exists so
/// cut voices can finish their kill fades without blocking new hits.
const ACTIVE_CAP: usize = 64;

/// Unity, deliberately — see `level.rs` for the philosophy the synths
/// follow. The sampler is the exception to the −12 dBFS trims because its
/// content is the user's: attenuating their mastered kick by default reads
/// as "the sampler is quiet", and the soft saturator plus the master
/// limiter still bound the worst case.
const OUTPUT_TRIM: f32 = 1.0;

/// One-pole DC blocker. Samples captured from analogue-modelled synths
/// often carry a standing offset, and a fade cannot save an output that
/// sits at +0.05 all day.
struct DcBlocker {
    x1: f32,
    y1: f32,
    r: f32,
}

impl DcBlocker {
    fn new() -> Self {
        Self { x1: 0.0, y1: 0.0, r: 0.9986 }
    }

    fn set_rate(&mut self, sample_rate: f64) {
        // ~10 Hz corner.
        self.r = (1.0 - core::f64::consts::TAU * 10.0 / sample_rate).max(0.9) as f32;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        // One non-finite input used to latch `y1` forever, and a filter
        // that has eaten a NaN silences everything after it until reset —
        // the whole track, for the rest of the session. The decoder now
        // guards the front door, but PCM reaches this line from more
        // places than the decoder, and a filter should not be one bad
        // sample away from permanent. The check is a bit test, cheaper
        // than the multiply above it.
        if !y.is_finite() {
            self.x1 = 0.0;
            self.y1 = 0.0;
            return 0.0;
        }
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

pub struct Sampler {
    sample_rate: f64,
    params: [f32; PARAM_COUNT],
    pads: Vec<Pad>,
    voices: Vec<SamplerVoice>,
    /// Monotonic per voice start, for oldest-first stealing.
    age_counter: u64,
    /// Monotonic per note-on gesture; all layers of one hit share it.
    hit_counter: u64,
    /// The UI's audition, on voices of its own.
    preview: Preview,
    /// The playing phrases, and the one instrument they all play through.
    runners: RunnerPool,
    child: ChildHost,
    /// The sample voices' own mix, held back until the child's audio can be
    /// summed into it. The child is rendered once per block and can only be
    /// rendered after the block's notes are known, so the sampler's own
    /// output cannot be finished a sample at a time any more — it is written
    /// here, joined by the child, and then taken through the blocker and the
    /// saturator in one pass.
    mix_l: Vec<f32>,
    mix_r: Vec<f32>,
    dc_l: DcBlocker,
    dc_r: DcBlocker,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            sample_rate: 44_100.0,
            params: PARAM_DEFAULTS,
            pads: (0..NUM_PADS).map(|i| Pad::for_note(PAD_BASE_NOTE + i as u8)).collect(),
            voices: Vec::new(),
            age_counter: 0,
            hit_counter: 0,
            preview: Preview::new(),
            runners: RunnerPool::new(),
            child: ChildHost::new(),
            mix_l: Vec::new(),
            mix_r: Vec::new(),
            dc_l: DcBlocker::new(),
            dc_r: DcBlocker::new(),
        }
    }

    fn note_on(&mut self, note: u8, vel: u8, offset: u32, out: &mut ChildEvents) {
        let Some(pad_idx) = pad_index(note) else { return };
        let depth = self.params[P_VEL];
        let v = f32::from(vel) / 127.0;
        let vel_gain = 1.0 + depth * (v * v - 1.0);

        let (poly, choke) = {
            let cfg = &self.pads[pad_idx].config;
            (cfg.poly, cfg.choke)
        };

        // Choke: this hit silences the rest of its mute group, phrases
        // included — a phrase on the open hat is as much the open hat as a
        // sample of one is.
        if choke > 0 {
            let pads = &self.pads;
            for voice in &mut self.voices {
                if voice.is_active()
                    && voice.pad != pad_idx
                    && pads.get(voice.pad).is_some_and(|p| p.config.choke == choke)
                {
                    voice.kill();
                }
            }
            self.runners.choke(
                pad_idx,
                choke,
                |p| pads.get(p).map_or(0, |pad| pad.config.choke),
                offset,
                out,
            );
        }

        // Poly: make room for this hit among the pad's own.
        self.enforce_poly(pad_idx, poly, offset, out);

        self.hit_counter += 1;
        let hit = self.hit_counter;

        let ctx = {
            let pad = &self.pads[pad_idx];
            RunnerStart {
                pad: pad_idx,
                hit,
                note,
                root: pad.config.root,
                vel_gain,
                gate: pad.config.trig == TrigMode::Gate,
            }
        };
        self.start_phrases(&ctx, vel, offset, out);

        // Which sampled layers answer this hit: all of them that match on a
        // stacking pad, one of them on a cycling one. Asked once, because
        // asking advances the rotation. Phrases are not in it — they are
        // never rotated, and they have already been started above.
        let answering = self.pads[pad_idx].answering(vel);

        for layer_idx in 0..MAX_LAYERS {
            if answering & (1 << layer_idx) == 0 {
                continue;
            }
            let Some(trigger) = ({
                let pad = &self.pads[pad_idx];
                pad.layers[layer_idx].as_ref().map(pad::LayerSlot::trigger)
            }) else {
                continue;
            };

            // The borrow of the pad table ends before the voice pool is
            // touched: resolve the trigger into locals first.
            let pcm = Arc::clone(trigger.pcm);
            let trigger = voice::LayerTrigger { pcm: &pcm, ..trigger };

            self.enforce_cap();
            self.age_counter += 1;
            let age = self.age_counter;
            let cfg = self.pads[pad_idx].config;
            let slot = self.find_slot();
            if let Some(voice) = self.voices.get_mut(slot) {
                voice.start(note, pad_idx, hit, age, &cfg, &trigger, vel_gain, self.sample_rate);
            }
        }
    }

    /// Start a runner for every phrase on the pad that answers this hit.
    ///
    /// Before the layers rather than after, so that a hit which has to steal
    /// a runner steals from the phrases already playing rather than from
    /// itself — and so that a phrase's first notes and its pad's samples
    /// land on the same sample offset.
    fn start_phrases(
        &mut self,
        ctx: &RunnerStart,
        vel: u8,
        offset: u32,
        out: &mut ChildEvents,
    ) {
        if !self.child.is_loaded() {
            // No instrument to play through: a phrase would take a runner,
            // hold notes nothing hears, and count against the pad's poly.
            return;
        }
        for i in 0..MAX_PHRASES {
            let Some(slot) = self.pads[ctx.pad].phrases[i].as_ref().filter(|p| p.answers(vel))
            else {
                continue;
            };
            self.runners.start(slot, ctx, offset, out);
        }
    }

    /// Cut the oldest hits on a pad until a new one fits under `poly`.
    fn enforce_poly(&mut self, pad_idx: usize, poly: u8, offset: u32, out: &mut ChildEvents) {
        // A poly of zero would ask for room that cutting cannot make, and
        // the loop below would never end. The pad table clamps it to at
        // least one; this is the guard for the day something else does not.
        let poly = usize::from(poly.max(1));
        loop {
            let (hits, oldest) = self.hit_census(pad_idx);
            if hits < poly {
                return;
            }
            for v in &mut self.voices {
                if v.is_active() && v.pad == pad_idx && v.hit == oldest {
                    v.kill();
                }
            }
            self.runners.stop_hit(pad_idx, oldest, offset, out);
        }
    }

    /// Every hit sounding on a pad, samples and phrases together.
    ///
    /// One iterator over both because a hit is a *gesture*, not a voice: a
    /// pad whose only layer is a phrase still has hits, and poly has to cut
    /// them exactly as it cuts sampled ones.
    fn pad_hits(&self, pad_idx: usize) -> impl Iterator<Item = u64> + '_ {
        self.voices
            .iter()
            .filter(move |v| v.is_active() && v.pad == pad_idx)
            .map(|v| v.hit)
            .chain(self.runners.hits_on(pad_idx))
    }

    /// How many distinct hits a pad has sounding, and the oldest of them.
    ///
    /// Distinct: one hit spreads across a voice per layer and a runner per
    /// phrase, and poly counts the gesture once. The dedupe is a scan of
    /// what came before rather than a set, because the list is short, the
    /// deadline is not negotiable, and a set is an allocation.
    fn hit_census(&self, pad_idx: usize) -> (usize, u64) {
        let mut hits = 0usize;
        let mut oldest = u64::MAX;
        for (i, hit) in self.pad_hits(pad_idx).enumerate() {
            if !self.pad_hits(pad_idx).take(i).any(|seen| seen == hit) {
                hits += 1;
                oldest = oldest.min(hit);
            }
        }
        (hits, oldest)
    }

    /// Keep the sounding population under the global cap.
    fn enforce_cap(&mut self) {
        loop {
            let mut active = 0usize;
            let mut oldest_age = u64::MAX;
            for v in &self.voices {
                if v.is_active() {
                    active += 1;
                    oldest_age = oldest_age.min(v.age);
                }
            }
            if active < ACTIVE_CAP {
                return;
            }
            for v in &mut self.voices {
                if v.is_active() && v.age == oldest_age {
                    v.kill();
                }
            }
        }
    }

    /// A slot for a new voice: a dead one, else the most-faded dying one.
    /// The cap keeps "everything active" impossible, but the fallback is
    /// the oldest voice rather than a panic all the same.
    fn find_slot(&self) -> usize {
        let mut best_killing = usize::MAX;
        let mut best_killing_age = u64::MAX;
        let mut oldest = 0usize;
        let mut oldest_age = u64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_sounding() {
                return i;
            }
            if v.is_killing() && v.age < best_killing_age {
                best_killing = i;
                best_killing_age = v.age;
            }
            if v.age < oldest_age {
                oldest = i;
                oldest_age = v.age;
            }
        }
        if best_killing != usize::MAX {
            best_killing
        } else {
            oldest
        }
    }

    fn note_off(&mut self, note: u8, offset: u32, out: &mut ChildEvents) {
        let Some(pad_idx) = pad_index(note) else { return };
        for v in &mut self.voices {
            if v.pad == pad_idx {
                v.note_off();
            }
        }
        self.runners.note_off(pad_idx, offset, out);
    }

    /// All-sound-off: the panic gesture and the transport's stop edge. The
    /// audition goes with it — a preview left looping after a panic is the
    /// one sound in the box the player has no way to reach.
    ///
    /// The child gets both halves: every note its runners were holding, so
    /// the sampler's own books are straight, and the panic itself, so the
    /// child's voices die even if one of them is ringing from a note the
    /// sampler has already given back.
    fn kill_all(&mut self, offset: u32, out: &mut ChildEvents) {
        for v in &mut self.voices {
            v.kill();
        }
        self.preview.stop();
        // The panic goes first, and goes even when the block's event list is
        // nearly full: it is one message, it silences the child whatever its
        // voices are doing, and the note-offs behind it are bookkeeping that
        // the next block can finish if this one runs out of room.
        out.push_cc(offset, 120);
        self.runners.stop_all(offset, out);
    }

    /// All-notes-off: gates release, one-shots play on.
    ///
    /// Nothing is forwarded to the child, and nothing needs to be: the only
    /// notes it is holding are the ones its runners put there, and releasing
    /// those runners releases exactly those notes.
    fn release_all(&mut self, offset: u32, out: &mut ChildEvents) {
        for v in &mut self.voices {
            v.note_off();
        }
        self.runners.release_gates(offset, out);
    }

    #[cfg(test)]
    fn sounding_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_sounding()).count()
    }

    #[cfg(test)]
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    #[cfg(test)]
    fn sounding_on(&self, pad_idx: usize) -> usize {
        self.voices.iter().filter(|v| v.is_sounding() && v.pad == pad_idx).count()
    }

    #[cfg(test)]
    pub(super) fn playing_phrases(&self) -> usize {
        self.runners.playing()
    }

    /// Whether any runner still believes the child is holding a note. The
    /// hung-note assertion, asked of the books rather than of the audio.
    #[cfg(test)]
    pub(super) fn phrases_hold_notes(&self) -> bool {
        self.runners.holds_any_note()
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Sampler {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "Sampler".into(),
            version: "0.1.0".into(),
            author: "Phosphor".into(),
            category: PluginCategory::Instrument,
        }
    }

    fn init(&mut self, sample_rate: f64, max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..VOICE_POOL).map(|_| SamplerVoice::new()).collect();
        self.mix_l = vec![0.0; max_buffer_size];
        self.mix_r = vec![0.0; max_buffer_size];
        // A child delivered before the sampler was started has been waiting
        // for this: it is started here, with the rate and the block size the
        // sampler itself was just given.
        self.child.init(sample_rate, max_buffer_size);
        // And the runners learn the rate they will be counting engine frames
        // at, so a phrase captured on another device plays at its own speed.
        self.runners.set_rate(sample_rate);
        self.dc_l.set_rate(sample_rate);
        self.dc_r.set_rate(sample_rate);
        self.dc_l.reset();
        self.dc_r.reset();
    }

    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]) {
        if outputs.is_empty() {
            return;
        }
        let buf_len = outputs[0].len();

        // level 0.8 = unity; the knob's top is a 1.56× boost.
        let gain = self.params[P_LEVEL] * 1.25 * OUTPUT_TRIM;

        // MIDI event sorting (allocation-free).
        let mut event_indices: [usize; 256] = [0; 256];
        let event_count = midi_events.len().min(256);
        for (i, slot) in event_indices.iter_mut().enumerate().take(event_count) {
            *slot = i;
        }
        for i in 1..event_count {
            let mut j = i;
            while j > 0
                && midi_events[event_indices[j]].sample_offset
                    < midi_events[event_indices[j - 1]].sample_offset
            {
                event_indices.swap(j, j - 1);
                j -= 1;
            }
        }
        let mut ei = 0;

        let stereo = outputs.len() >= 2;

        // The child's note traffic for this block. Written as the block is
        // walked, so it comes out already in timeline order.
        let mut child_events = ChildEvents::new();
        // Note-offs a stopping runner could not hand over last block go
        // first, at offset zero: a key held down on the child is the one
        // thing that must never wait for anything else.
        self.runners.flush_owed(&mut child_events);

        // Dead in practice, alive in principle: a device that hands the
        // callback more frames than it promised finds the mix buffer grown
        // rather than the block truncated. The mixer's own buffers carry the
        // same branch.
        if self.mix_l.len() < buf_len {
            self.mix_l.resize(buf_len, 0.0);
            self.mix_r.resize(buf_len, 0.0);
        }

        for i in 0..buf_len {
            let offset = i as u32;
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            self.note_on(ev.data1, ev.data2, offset, &mut child_events);
                        } else {
                            self.note_off(ev.data1, offset, &mut child_events);
                        }
                    }
                    0x80 => self.note_off(ev.data1, offset, &mut child_events),
                    0xB0 => match ev.data1 {
                        // All sound off: the panic gesture and the
                        // transport's stop edge. A fade, not a truncation
                        // — but a fast one.
                        120 => self.kill_all(offset, &mut child_events),
                        // All notes off: gates release musically. A
                        // one-shot ignores a note-off by definition, and
                        // this is a note-off.
                        123 => self.release_all(offset, &mut child_events),
                        _ => {}
                    },
                    _ => {}
                }
                ei += 1;
            }

            // Phrases move a frame, handing the child whatever fell due at
            // this sample — after the block's own notes, so a phrase
            // triggered at this offset starts here rather than a sample late.
            self.runners.advance(offset, &mut child_events);

            let (mut sum_l, mut sum_r) = self.preview.tick(self.sample_rate);
            for v in &mut self.voices {
                if v.is_sounding() {
                    let (l, r) = v.tick();
                    sum_l += l;
                    sum_r += r;
                }
            }
            self.mix_l[i] = sum_l;
            self.mix_r[i] = sum_r;
        }

        // One render for every phrase on every pad, its events already in
        // order. Summed in before the blocker and the saturator, so a child
        // with a standing offset is treated exactly like a sample with one.
        let child_played =
            self.child.render(buf_len, child_events.events(), self.runners.busy());
        let (child_l, child_r) = self.child.block(buf_len);

        for i in 0..buf_len {
            let mut sum_l = self.mix_l[i];
            let mut sum_r = self.mix_r[i];
            if child_played {
                sum_l += child_l[i];
                sum_r += child_r[i];
            }
            let left = soft_saturate(self.dc_l.process(sum_l) * gain);
            let right = soft_saturate(self.dc_r.process(sum_r) * gain);

            if stereo {
                outputs[0][i] = left;
                outputs[1][i] = right;
            } else {
                outputs[0][i] = (left + right) * 0.5;
            }
        }
    }

    fn parameter_count(&self) -> usize {
        PARAM_COUNT
    }

    fn parameter_info(&self, index: usize) -> Option<ParameterInfo> {
        if index >= PARAM_COUNT {
            return None;
        }
        Some(ParameterInfo {
            name: PARAM_NAMES[index].into(),
            min: 0.0,
            max: 1.0,
            default: PARAM_DEFAULTS[index],
            unit: String::new(),
        })
    }

    fn get_parameter(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn set_parameter(&mut self, index: usize, value: f32) {
        if let Some(p) = self.params.get_mut(index) {
            *p = phosphor_plugin::clamp_parameter(value);
        }
    }

    fn reset(&mut self) {
        for v in &mut self.voices {
            v.silence();
        }
        self.preview.silence();
        // The runners let go of their notes without sending anything, which
        // is honest only because the child is reset in the same breath: a
        // reset instrument holds nothing, so there is nothing to give back.
        self.runners.silence();
        self.child.reset();
        self.dc_l.reset();
        self.dc_r.reset();
        self.age_counter = 0;
        self.hit_counter = 0;
        // Round robin goes back to the top here and nowhere else. A stopped
        // transport does not rewind a hardware sampler's rotation and does
        // not rewind this one; a panic and a fresh instrument do.
        for pad in &mut self.pads {
            pad.rewind_cycle();
        }
    }

    fn set_sampler_pad(&mut self, pad: u8, config: &PadConfig, layers: &[PadLayer]) {
        if let Some(p) = self.pads.get_mut(usize::from(pad)) {
            p.set(config, layers);
        }
    }

    fn set_sampler_preview(&mut self, preview: Option<&PreviewLayer>) {
        self.preview.set(preview, self.sample_rate);
    }

    fn set_sampler_child(&mut self, child: Option<Box<dyn Plugin>>) {
        // Whatever the runners were playing was being played by the child
        // that is leaving. They let go of it here rather than keeping notes
        // held on an instrument that no longer exists.
        self.runners.silence();
        self.child.set(child);
    }

    fn set_sampler_child_param(&mut self, index: usize, value: f32) {
        self.child.set_parameter(index, value);
    }

    fn set_sampler_phrases(&mut self, pad: u8, phrases: &[PadPhrase]) {
        if let Some(p) = self.pads.get_mut(usize::from(pad)) {
            p.set_phrases(phrases);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::tests::allocations_during;
    use phosphor_plugin::sample::{PreviewMode, SamplePcm, TrigMode};

    pub(super) const SR: f64 = 44_100.0;

    pub(super) fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
    }

    pub(super) fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }

    pub(super) fn cc(controller: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: controller, data2: 0 }
    }

    pub(super) fn constant_pcm(value: f32, frames: usize) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![value; frames], channels: 1, sample_rate: SR as f32 })
    }

    /// A 220 Hz sine. Level tests that go through `process` must use an
    /// audio-band signal: the DC blocker is *supposed* to bleed a
    /// constant buffer to nothing, so a constant is only fit for testing
    /// the blocker itself.
    pub(super) fn sine_pcm(amp: f32, frames: usize) -> Arc<SamplePcm> {
        let data = (0..frames)
            .map(|i| amp * (core::f32::consts::TAU * 220.0 * i as f32 / SR as f32).sin())
            .collect();
        Arc::new(SamplePcm { data, channels: 1, sample_rate: SR as f32 })
    }

    pub(super) fn sampler_with(pad_note: u8, config: PadConfig, layers: &[PadLayer]) -> Sampler {
        let mut s = Sampler::new();
        s.init(SR, 512);
        s.set_sampler_pad(pad_note - PAD_BASE_NOTE, &config, layers);
        s
    }

    pub(super) fn load(s: &mut Sampler, pad_note: u8, config: PadConfig, layers: &[PadLayer]) {
        s.set_sampler_pad(pad_note - PAD_BASE_NOTE, &config, layers);
    }

    /// Run one stereo block and return (left, right).
    pub(super) fn process(s: &mut Sampler, events: &[MidiEvent], n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        {
            let mut outs: Vec<&mut [f32]> = vec![&mut l, &mut r];
            s.process(&[], &mut outs, events);
        }
        (l, r)
    }

    pub(super) fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0f32, |a, s| a.max(s.abs()))
    }

    #[test]
    fn an_empty_sampler_is_silent_for_every_note() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        for note in [0u8, 20, 21, 60, 108, 109, 127] {
            let (l, r) = process(&mut s, &[note_on(note, 127, 0)], 512);
            assert_eq!(peak(&l), 0.0, "note {note} made sound from nothing");
            assert_eq!(peak(&r), 0.0);
        }
    }

    #[test]
    fn notes_off_the_key_bed_are_ignored_not_wrapped() {
        let mut s = sampler_with(
            21,
            PadConfig::for_key(21),
            &[PadLayer::from_pcm(constant_pcm(0.5, 44_100))],
        );
        // 20 and 109 sit one key off each end; if the map wrapped or
        // clamped they would land on a real pad.
        let (l, _) = process(&mut s, &[note_on(20, 127, 0), note_on(109, 127, 0)], 512);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn a_loaded_pad_speaks_at_its_recorded_level() {
        let mut s = sampler_with(
            60,
            PadConfig::for_key(60),
            &[PadLayer::from_pcm(sine_pcm(0.5, 44_100))],
        );
        let (l, r) = process(&mut s, &[note_on(60, 127, 0)], 2_000);
        // Level default is unity; mono centre is -3 dB per side. Measure
        // the peak over a settled window holding several full cycles.
        let expected = 0.5 * core::f32::consts::FRAC_1_SQRT_2;
        assert!((peak(&l[500..1_500]) - expected).abs() < 0.01, "left {}", peak(&l[500..1_500]));
        assert!((peak(&r[500..1_500]) - expected).abs() < 0.01);
    }

    #[test]
    fn layers_stack_and_a_muted_layer_does_not() {
        let one = PadLayer::from_pcm(sine_pcm(0.25, 44_100));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[one.clone()]);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        let single = peak(&l[500..1_500]);

        let mut s = sampler_with(60, PadConfig::for_key(60), &[one.clone(), one.clone()]);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        let double = peak(&l[500..1_500]);
        // Same buffer, same phase: two layers sum to exactly twice one.
        assert!(
            (double - single * 2.0).abs() < 0.02,
            "two layers made {double} not {}",
            single * 2.0
        );

        let mut muted = one.clone();
        muted.mute = true;
        let mut s = sampler_with(60, PadConfig::for_key(60), &[one, muted]);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        let with_mute = peak(&l[500..1_500]);
        assert!(
            (with_mute - single).abs() < 0.02,
            "a muted layer leaked: {with_mute} vs {single}"
        );
    }

    // ── Round robin ──

    /// One layer per level, so the peak of a block names which one answered.
    /// A sine rather than a constant: the DC blocker is supposed to bleed a
    /// constant away, and a level read through it would be reading the
    /// blocker.
    fn ladder(levels: &[f32]) -> Vec<PadLayer> {
        levels.iter().map(|&a| PadLayer::from_pcm(sine_pcm(a, 44_100))).collect()
    }

    /// Which layer of a ladder answered, by the level that came out. `None`
    /// for silence; the pan law takes 3 dB off each side, so the expected
    /// level is scaled the same way before it is matched.
    fn answered(levels: &[f32], out: &[f32]) -> Option<usize> {
        let heard = peak(&out[500..1_500]);
        levels.iter().position(|&a| (heard - a * core::f32::consts::FRAC_1_SQRT_2).abs() < 0.02)
    }

    /// A pad set cycling hands out its layers one at a time, in order, round
    /// and round. Six hits on three layers is two full laps.
    #[test]
    fn cycle_hands_out_one_layer_per_hit_in_order() {
        let levels = [0.2f32, 0.5, 0.8];
        let mut cfg = PadConfig::for_key(60);
        cfg.cycle = true;
        let mut s = sampler_with(60, cfg, &ladder(&levels));

        let mut order = Vec::new();
        for _ in 0..6 {
            let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
            order.push(answered(&levels, &l));
            // Well clear of the poly cut's fade before the next hit.
            process(&mut s, &[cc(120, 0)], 1_024);
        }
        assert_eq!(
            order,
            vec![Some(0), Some(1), Some(2), Some(0), Some(1), Some(2)],
            "the rotation did not go round",
        );
    }

    /// Off — the default — every layer that answers still sounds, and they
    /// stack. The defect this catches is a cycle that leaked into a pad that
    /// never asked for one.
    #[test]
    fn a_pad_that_does_not_cycle_still_stacks_every_layer() {
        let one = PadLayer::from_pcm(sine_pcm(0.25, 44_100));
        let cfg = PadConfig::for_key(60);
        assert!(!cfg.cycle, "a fresh pad cycles");
        let mut s = sampler_with(60, cfg, &[one.clone(), one.clone(), one]);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        // Same buffer, same phase: three layers sum to exactly three.
        let expected = 0.75 * core::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (peak(&l[500..1_500]) - expected).abs() < 0.03,
            "three stacked layers made {}, not {expected}",
            peak(&l[500..1_500]),
        );
    }

    /// A muted layer loses its turn rather than spending it on silence: the
    /// rotation is over the layers that answer, so a three-layer pad with the
    /// middle one muted alternates between the other two.
    #[test]
    fn a_muted_layer_skips_its_turn_in_the_rotation() {
        let levels = [0.2f32, 0.5, 0.8];
        let mut layers = ladder(&levels);
        layers[1].mute = true;
        let mut cfg = PadConfig::for_key(60);
        cfg.cycle = true;
        let mut s = sampler_with(60, cfg, &layers);

        let mut order = Vec::new();
        for _ in 0..4 {
            let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
            order.push(answered(&levels, &l));
            process(&mut s, &[cc(120, 0)], 1_024);
        }
        assert_eq!(
            order,
            vec![Some(0), Some(2), Some(0), Some(2)],
            "the muted layer took a turn, or took the others' turns with it",
        );
    }

    /// Velocity is honoured per hit, and each half of a split has a rotation
    /// of its own: a soft hit takes the next soft layer, a hard hit the next
    /// hard one, and neither spends the other's turn.
    #[test]
    fn a_cycling_pad_rotates_inside_the_velocity_window_that_answers() {
        let levels = [0.2f32, 0.4, 0.6, 0.8];
        let mut layers = ladder(&levels);
        for (i, layer) in layers.iter_mut().enumerate() {
            // Two soft, two loud.
            let (lo, hi) = if i < 2 { (0, 63) } else { (64, 127) };
            layer.vel_lo = lo;
            layer.vel_hi = hi;
        }
        let mut cfg = PadConfig::for_key(60);
        cfg.cycle = true;
        let mut s = sampler_with(60, cfg, &layers);

        let mut order = Vec::new();
        for vel in [30u8, 100, 30, 100, 30, 100] {
            let (l, _) = process(&mut s, &[note_on(60, vel, 0)], 1_500);
            // The velocity curve scales what comes out, so the level is
            // matched against the layer's own amplitude after it.
            let v = f32::from(vel) / 127.0;
            let vel_gain = 1.0 + PARAM_DEFAULTS[P_VEL] * (v * v - 1.0);
            let scaled: Vec<f32> = levels.iter().map(|&a| a * vel_gain).collect();
            order.push(answered(&scaled, &l));
            process(&mut s, &[cc(120, 0)], 1_024);
        }
        assert_eq!(
            order,
            vec![Some(0), Some(2), Some(1), Some(3), Some(0), Some(2)],
            "the two halves of the split shared one rotation",
        );
    }

    /// The rotation is the pad's memory. A transport stop — which is the
    /// all-sound-off gesture — leaves it where it was; the panic key's reset
    /// takes it back to the first layer. That is the hardware convention and
    /// it is the one a player's hands expect.
    #[test]
    fn the_rotation_survives_a_stop_and_is_rewound_by_a_reset() {
        let levels = [0.2f32, 0.5, 0.8];
        let mut cfg = PadConfig::for_key(60);
        cfg.cycle = true;
        let mut s = sampler_with(60, cfg, &ladder(&levels));

        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        assert_eq!(answered(&levels, &l), Some(0));
        // All sound off, which is what the transport's stop edge sends.
        process(&mut s, &[cc(120, 0)], 1_024);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        assert_eq!(answered(&levels, &l), Some(1), "a stop rewound the rotation");

        s.reset();
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        assert_eq!(answered(&levels, &l), Some(0), "a reset left the rotation standing");
    }

    /// Editing a pad does not rewind its rotation — in keys mode one trim
    /// nudge re-delivers eighty-eight pads, and a rotation that restarted on
    /// every delivery would never leave the first layer.
    #[test]
    fn re_delivering_a_pad_leaves_the_rotation_where_it_was() {
        let levels = [0.2f32, 0.5, 0.8];
        let mut cfg = PadConfig::for_key(60);
        cfg.cycle = true;
        let layers = ladder(&levels);
        let mut s = sampler_with(60, cfg, &layers);

        process(&mut s, &[note_on(60, 127, 0)], 1_500);
        process(&mut s, &[cc(120, 0)], 1_024);
        // The same pad, delivered again: what a knob turn or a trim sends.
        load(&mut s, 60, cfg, &layers);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        assert_eq!(answered(&levels, &l), Some(1), "the delivery rewound the rotation");
    }

    #[test]
    fn velocity_ranges_gate_layers() {
        let mut soft = PadLayer::from_pcm(constant_pcm(0.5, 44_100));
        soft.vel_lo = 0;
        soft.vel_hi = 64;
        let mut s = sampler_with(60, PadConfig::for_key(60), &[soft]);
        let (quiet, _) = process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(peak(&quiet), 0.0, "a hard hit reached a soft-only layer");
        let (heard, _) = process(&mut s, &[note_on(60, 40, 0)], 512);
        assert!(peak(&heard) > 0.05, "a soft hit missed its own layer");
    }

    #[test]
    fn velocity_depth_shapes_gain_and_zero_velocity_is_a_note_off() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 44_100));
        let mut cfg = PadConfig::for_key(60);
        cfg.trig = TrigMode::Gate;
        let mut s = sampler_with(60, cfg, &[layer]);
        // Full depth: a velocity-1 hit is near silent.
        s.set_parameter(P_VEL, 1.0);
        let (l, _) = process(&mut s, &[note_on(60, 1, 0)], 600);
        assert!(peak(&l) < 0.01, "depth 1.0 velocity 1 gave {}", peak(&l));
        // A 0x90 with velocity 0 is a note-off, not a silent retrigger.
        // 512 samples put the poly cut's 3 ms fade fully behind us.
        process(&mut s, &[note_on(60, 100, 0)], 512);
        assert_eq!(s.sounding_on(39), 1);
        process(&mut s, &[note_on(60, 0, 0)], 8_192);
        assert_eq!(s.sounding_on(39), 0, "velocity-0 note-on failed to release");
    }

    #[test]
    fn poly_one_retrigger_cuts_the_old_hit_without_a_click() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 88_200));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[layer]);
        process(&mut s, &[note_on(60, 127, 0)], 1_000);
        let (l, _) = process(&mut s, &[note_on(60, 127, 100)], 1_000);
        // The old hit fades under the new one instead of stacking with
        // it: even with the two sines out of phase, the sum never
        // approaches the doubled level a stack would show.
        let expected = 0.5 * core::f32::consts::FRAC_1_SQRT_2;
        assert!(peak(&l) < expected * 1.6, "retrigger stacked: peak {}", peak(&l));
        // And no step discontinuity anywhere around the cut. A 220 Hz
        // sine at this level moves ~0.011 per sample on its own; a
        // truncation would jump by the full amplitude.
        let max_jump = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(max_jump < 0.05, "retrigger clicked: step {max_jump}");
        // Well after the fade, exactly one hit's worth of voices sounds.
        process(&mut s, &[], 1_000);
        assert_eq!(s.sounding_on(39), 1);
    }

    #[test]
    fn poly_counts_hits_not_voices() {
        // Two layers per hit, poly 2: the third hit must cut the first,
        // leaving four voices (two hits), never six.
        let layer = PadLayer::from_pcm(constant_pcm(0.2, 88_200));
        let mut cfg = PadConfig::for_key(60);
        cfg.poly = 2;
        let mut s = sampler_with(60, cfg, &[layer.clone(), layer]);
        process(&mut s, &[note_on(60, 127, 0)], 256);
        process(&mut s, &[note_on(60, 127, 0)], 256);
        assert_eq!(s.active_voices(), 4);
        process(&mut s, &[note_on(60, 127, 0)], 1_000);
        assert_eq!(s.active_voices(), 4, "a third hit under poly 2 left extra voices");
    }

    #[test]
    fn choke_cuts_the_other_pad_in_the_group() {
        let long = PadLayer::from_pcm(constant_pcm(0.5, 88_200));
        let mut open = PadConfig::for_key(46);
        open.choke = 1;
        let mut closed = PadConfig::for_key(42);
        closed.choke = 1;
        let mut s = sampler_with(46, open, &[long.clone()]);
        load(&mut s, 42, closed, &[long]);

        process(&mut s, &[note_on(46, 127, 0)], 1_000);
        assert_eq!(s.sounding_on(25), 1, "the open hat never sounded");
        // The closed hat fires; the open hat must die within a few ms.
        process(&mut s, &[note_on(42, 127, 0)], 1_000);
        assert_eq!(s.sounding_on(25), 0, "the open hat survived its choke");
        assert_eq!(s.sounding_on(21), 1, "the closed hat choked itself");
    }

    #[test]
    fn a_pad_outside_the_group_is_left_alone() {
        let long = PadLayer::from_pcm(constant_pcm(0.5, 88_200));
        let mut hat = PadConfig::for_key(42);
        hat.choke = 1;
        let kick = PadConfig::for_key(36); // choke 0
        let mut s = sampler_with(36, kick, &[long.clone()]);
        load(&mut s, 42, hat, &[long]);
        process(&mut s, &[note_on(36, 127, 0)], 256);
        process(&mut s, &[note_on(42, 127, 0)], 1_000);
        assert_eq!(s.sounding_on(15), 1, "the kick was choked by a group it is not in");
    }

    #[test]
    fn hammering_every_pad_stays_bounded_and_under_the_cap() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let layer = PadLayer::from_pcm(constant_pcm(0.9, 88_200));
        for note in 21..=108u8 {
            let mut cfg = PadConfig::for_key(note);
            cfg.poly = 8;
            load(&mut s, note, cfg, &[layer.clone()]);
        }
        for round in 0..5 {
            for note in 21..=108u8 {
                let (l, r) = process(&mut s, &[note_on(note, 127, 0)], 64);
                for x in l.iter().chain(r.iter()) {
                    assert!(x.is_finite(), "round {round} note {note} went non-finite");
                    assert!(x.abs() <= 1.0, "round {round} note {note} left the rails: {x}");
                }
                assert!(
                    s.active_voices() <= ACTIVE_CAP,
                    "cap breached: {} active",
                    s.active_voices()
                );
                assert!(s.sounding_voices() <= VOICE_POOL);
            }
        }
    }

    #[test]
    fn cc_120_silences_one_shots_fast() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 441_000)); // 10 s
        let mut s = sampler_with(60, PadConfig::for_key(60), &[layer.clone()]);
        load(&mut s, 62, PadConfig::for_key(62), &[layer]);
        process(&mut s, &[note_on(60, 127, 0), note_on(62, 127, 0)], 512);
        assert_eq!(s.sounding_voices(), 2);
        // All sound off, then 10 ms: the 3 ms fades are long gone.
        let (l, _) = process(&mut s, &[cc(120, 0)], 441);
        assert_eq!(s.sounding_voices(), 0, "a one-shot survived all-sound-off");
        let max_jump = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(max_jump < 0.05, "the stop itself clicked: {max_jump}");
    }

    #[test]
    fn cc_123_releases_gates_and_leaves_one_shots_alone() {
        let layer = PadLayer::from_pcm(constant_pcm(0.5, 441_000));
        let mut gate = PadConfig::for_key(60);
        gate.trig = TrigMode::Gate;
        gate.release_ms = 10.0;
        let shot = PadConfig::for_key(62);
        let mut s = sampler_with(60, gate, &[layer.clone()]);
        load(&mut s, 62, shot, &[layer]);
        process(&mut s, &[note_on(60, 127, 0), note_on(62, 127, 0)], 512);
        process(&mut s, &[cc(123, 0)], 8_192);
        assert_eq!(s.sounding_on(39), 0, "the gate ignored all-notes-off");
        assert_eq!(s.sounding_on(41), 1, "the one-shot obeyed a note-off");
    }

    #[test]
    fn a_finished_once_audition_releases_its_audio() {
        // The parked PreviewLayer was the audit's other Arc leak: a Once
        // that ended kept its buffer claimed for the life of the plugin.
        let pcm = sine_pcm(0.5, 2_000);
        let mut s = sampler_with(60, PadConfig::for_key(60), &[PadLayer::from_pcm(Arc::clone(&pcm))]);
        let before = Arc::strong_count(&pcm);
        s.set_sampler_preview(Some(&preview(PadLayer::from_pcm(Arc::clone(&pcm)), PreviewMode::Once)));
        assert!(Arc::strong_count(&pcm) > before, "the audition never took the buffer");
        // 2,000 frames of audio, then silence: run well past the end.
        for _ in 0..10 {
            process(&mut s, &[], 512);
        }
        assert_eq!(
            Arc::strong_count(&pcm),
            before,
            "the finished audition still holds the buffer"
        );
    }

    #[test]
    fn the_dc_blocker_heals_after_a_nan_instead_of_latching() {
        // The audit's C1, at the engine layer: the decoder now guards its
        // door, but PCM reaches the blocker from more places than the
        // decoder, and one bad sample must cost one sample, not the track.
        let mut bad = vec![0.5f32; 4_410];
        bad[100] = f32::NAN;
        let poisoned = Arc::new(SamplePcm { data: bad, channels: 1, sample_rate: SR as f32 });
        let mut s = sampler_with(60, PadConfig::for_key(60), &[PadLayer::from_pcm(poisoned)]);
        load(&mut s, 62, PadConfig::for_key(62), &[PadLayer::from_pcm(sine_pcm(0.5, 44_100))]);

        process(&mut s, &[note_on(60, 127, 0)], 2_048);
        let (l, _) = process(&mut s, &[cc(120, 0)], 1_024);
        assert!(l.iter().all(|x| x.is_finite()), "the NaN escaped the plugin");

        // The clean pad afterwards must speak — this exact sequence used
        // to render 0.000000 forever.
        let (l, _) = process(&mut s, &[note_on(62, 127, 0)], 2_048);
        assert!(peak(&l[500..]) > 0.05, "the track stayed silent after the NaN: {}", peak(&l[500..]));
    }

    #[test]
    fn swapping_a_pad_mid_note_lets_the_voice_finish_on_the_old_sound() {
        // A voice keeps its own Arc: replacing the pad's layers while it
        // plays must neither cut it nor re-point it at the new buffer.
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 441_000));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[layer]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(s.sounding_on(39), 1);

        s.set_sampler_pad(39, &PadConfig::for_key(60), &[]); // pad emptied
        let (l, _) = process(&mut s, &[], 1_500);
        assert_eq!(s.sounding_on(39), 1, "the swap cut a playing voice");
        assert!(peak(&l[500..]) > 0.3, "the old sound went quiet after the swap");

        // But a fresh hit on the emptied pad is silence.
        s.reset();
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 512);
        assert_eq!(peak(&l), 0.0, "an emptied pad still speaks");
    }

    #[test]
    fn reset_silences_everything_immediately() {
        let layer = PadLayer::from_pcm(constant_pcm(0.5, 441_000));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[layer]);
        process(&mut s, &[note_on(60, 127, 0)], 512);
        s.reset();
        assert_eq!(s.sounding_voices(), 0);
        let (l, _) = process(&mut s, &[], 512);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn a_dc_offset_is_bled_off_the_output() {
        // A buffer that IS a DC offset: all +0.8.
        let layer = PadLayer::from_pcm(constant_pcm(0.8, 441_000));
        let mut cfg = PadConfig::for_key(60);
        cfg.trig = TrigMode::Gate;
        let mut s = sampler_with(60, cfg, &[layer]);
        // Half a second in, the blocker has pulled the standing offset
        // towards zero.
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 22_050);
        let tail_mean: f32 =
            l[20_000..].iter().copied().sum::<f32>() / (l.len() - 20_000) as f32;
        assert!(tail_mean.abs() < 0.05, "DC survived: mean {tail_mean}");
    }

    #[test]
    fn the_level_knob_defaults_to_unity_and_scales() {
        let layer = PadLayer::from_pcm(sine_pcm(0.4, 44_100));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[layer]);
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        let at_default = peak(&l[500..1_500]);
        s.reset();
        s.set_parameter(P_LEVEL, 0.4); // half the default
        let (l, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        let at_half = peak(&l[500..1_500]);
        assert!(
            (at_half - at_default * 0.5).abs() < 0.01,
            "level 0.4 gave {at_half} against {at_default} at default"
        );
    }

    #[test]
    fn mono_output_still_carries_the_signal() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 44_100));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[layer]);
        let mut mono = vec![0.0f32; 1_500];
        {
            let mut outs: Vec<&mut [f32]> = vec![&mut mono];
            s.process(&[], &mut outs, &[note_on(60, 127, 0)]);
        }
        assert!(peak(&mono[500..]) > 0.2, "mono fallback lost the audio");
    }

    #[test]
    fn process_never_allocates_even_while_spawning_voices() {
        let layer = PadLayer::from_pcm(constant_pcm(0.5, 88_200));
        let mut s = Sampler::new();
        s.init(SR, 512);
        for note in 21..=108u8 {
            let mut cfg = PadConfig::for_key(note);
            cfg.poly = 4;
            cfg.choke = 1;
            // Half the bed cycling, half stacking: the rotation is asked
            // once per hit inside `note_on`, so it is inside the claim.
            cfg.cycle = note % 2 == 0;
            load(&mut s, note, cfg, &[layer.clone(), layer.clone()]);
        }
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        let events: Vec<MidiEvent> = (21..=68u8).map(|n| note_on(n, 127, 0)).collect();
        let offs: Vec<MidiEvent> = (21..=68u8).map(|n| note_off(n, 256)).collect();
        let allocations = allocations_during(|| {
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &events);
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &offs);
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &[cc(120, 0)]);
        });
        assert_eq!(allocations, 0, "process reached the allocator");
    }

    #[test]
    fn set_sampler_pad_on_the_audio_thread_does_not_allocate() {
        // The command handler calls this while the callback runs: copying
        // a config and re-pointing eight Arcs must stay off the allocator.
        let mut s = Sampler::new();
        s.init(SR, 512);
        let layers: Vec<PadLayer> =
            (0..8).map(|_| PadLayer::from_pcm(constant_pcm(0.5, 44_100))).collect();
        let cfg = PadConfig::for_key(60);
        load(&mut s, 60, cfg, &layers); // slots occupied, so replacement also drops
        let allocations = allocations_during(|| {
            s.set_sampler_pad(39, &cfg, &layers);
            s.set_sampler_pad(39, &cfg, &[]);
        });
        assert_eq!(allocations, 0, "a pad edit reached the allocator");
    }

    // ── The audition ──

    fn preview(layer: PadLayer, mode: PreviewMode) -> PreviewLayer {
        PreviewLayer { config: PadConfig::for_key(60), layer, mode }
    }

    /// A two-layer pad is the test, because it is the one that can be got
    /// wrong: the audition must sound the layer it was handed and nothing
    /// else on the pad, and it must not be a note-on in disguise.
    #[test]
    fn an_audition_sounds_one_layer_and_not_the_pad() {
        let quiet = PadLayer::from_pcm(sine_pcm(0.2, 44_100));
        let loud = PadLayer::from_pcm(sine_pcm(0.8, 44_100));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[quiet.clone(), loud.clone()]);

        // Playing the pad sums both layers; auditioning one gives one.
        let (both, _) = process(&mut s, &[note_on(60, 127, 0)], 1_500);
        let stacked = peak(&both[500..1_500]);
        s.reset();

        s.set_sampler_preview(Some(&preview(quiet, PreviewMode::Once)));
        let (l, r) = process(&mut s, &[], 1_500);
        let heard = peak(&l[500..1_500]);
        let expected = 0.2 * core::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (heard - expected).abs() < 0.02,
            "the audition played {heard}, not the quiet layer's {expected} (the pad is {stacked})",
        );
        assert!((peak(&r[500..1_500]) - expected).abs() < 0.02);
        // And it is not a hit: nothing on the pad is sounding.
        assert_eq!(s.sounding_on(39), 0, "the audition took a voice from the pool");
        assert_eq!(s.preview.sounding(), 1);

        // The other layer, on the same pad, auditions as itself.
        s.reset();
        s.set_sampler_preview(Some(&preview(loud, PreviewMode::Once)));
        let (l, _) = process(&mut s, &[], 1_500);
        let expected = 0.8 * core::f32::consts::FRAC_1_SQRT_2;
        assert!((peak(&l[500..1_500]) - expected).abs() < 0.03, "{}", peak(&l[500..1_500]));
    }

    /// An audition plays the layer's *trimmed* region, which is the whole
    /// point of the strip that sends it.
    #[test]
    fn an_audition_starts_at_the_trim_and_stops_at_it() {
        // Silence, then a sine: a preview of the second half must speak at
        // once rather than after half a second of nothing.
        let frames = 44_100usize;
        let mut data = vec![0.0f32; frames];
        for (i, s) in data.iter_mut().enumerate().skip(frames / 2) {
            *s = 0.5 * (core::f32::consts::TAU * 220.0 * i as f32 / SR as f32).sin();
        }
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: SR as f32 });
        let mut layer = PadLayer::from_pcm(pcm);
        layer.start_frame = (frames / 2) as u64;
        layer.end_frame = (frames / 2 + 4_410) as u64; // 100 ms of it

        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);
        s.set_sampler_preview(Some(&preview(layer, PreviewMode::Once)));
        let (l, _) = process(&mut s, &[], 8_000);
        assert!(peak(&l[500..1_500]) > 0.2, "the trim start was not honoured");
        // 100 ms is 4 410 samples; well past it there is nothing left.
        assert!(peak(&l[5_500..]) < 0.001, "the trim end was not honoured");
        assert_eq!(s.preview.sounding(), 0, "a one-pass audition never ended");
    }

    /// The seam of a loop is two ramps crossing, not a step and not a hole.
    ///
    /// A sawtooth is the worst case and the clearest one: its region ends at
    /// full positive and begins at full negative, so a hard join would show
    /// a step of the whole waveform. It also stays in the audio band, which
    /// a square-ish buffer would not — the DC blocker is *supposed* to bleed
    /// a standing offset away, and a test that fed it one would be measuring
    /// the blocker.
    #[test]
    fn a_looping_audition_has_no_step_at_its_seam() {
        let frames = 882usize; // 20 ms, so the window below holds many seams
        let data: Vec<f32> =
            (0..frames).map(|i| 1.6 * i as f32 / frames as f32 - 0.8).collect();
        let pcm = Arc::new(SamplePcm { data, channels: 1, sample_rate: SR as f32 });
        let layer = PadLayer::from_pcm(pcm);

        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);
        s.set_sampler_preview(Some(&preview(layer, PreviewMode::Loop)));
        let (l, _) = process(&mut s, &[], 22_050); // half a second: 25 laps
        // It really is still going half a second later.
        assert!(peak(&l[20_000..]) > 0.3, "the loop stopped: {}", peak(&l[20_000..]));
        // And nothing anywhere near the jump a hard seam would make: the
        // full 1.6 swing is 1.13 after the centre pan law, while the ramp
        // itself moves 0.0013 per sample.
        let seam = l[1_000..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(seam < 0.1, "the loop seam stepped by {seam}");
        // Nor is the seam a hole. Fading out and back in would take every
        // lap through silence; the overlap keeps a level up throughout.
        let quietest = l[1_000..20_000].chunks(441).map(peak).fold(f32::MAX, f32::min);
        assert!(quietest > 0.2, "the loop dropped to {quietest} at a seam");
    }

    /// Off is a fade, and it is over inside the kill fade's own length.
    #[test]
    fn off_kills_an_audition_inside_the_fade() {
        let layer = PadLayer::from_pcm(sine_pcm(0.8, 441_000)); // 10 s
        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);
        s.set_sampler_preview(Some(&preview(layer, PreviewMode::Loop)));
        process(&mut s, &[], 1_000);
        assert_eq!(s.preview.sounding(), 1);

        s.set_sampler_preview(None);
        // 3 ms at 44.1 kHz is ~132 samples; 441 is 10 ms.
        let (l, _) = process(&mut s, &[], 4_410);
        assert_eq!(s.preview.sounding(), 0, "the audition survived being turned off");
        let jump = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(jump < 0.05, "turning it off clicked: {jump}");
        // What is left at the far end of that hundred milliseconds is the
        // DC blocker's own ring-down, which outlives any fade by design and
        // is four orders of magnitude below the signal.
        assert!(peak(&l[4_000..]) < 1e-3, "something kept sounding: {}", peak(&l[4_000..]));
        // And off again is not a fault.
        s.set_sampler_preview(None);
    }

    /// The nudge run: one audition per press, and they do not pile up.
    #[test]
    fn re_auditioning_retriggers_rather_than_stacking() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 441_000));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);
        for _ in 0..12 {
            s.set_sampler_preview(Some(&preview(layer.clone(), PreviewMode::Once)));
            let (l, _) = process(&mut s, &[], 2_205); // 50 ms between presses
            // Two passes of the same sine in phase would double the level.
            let expected = 0.5 * core::f32::consts::FRAC_1_SQRT_2;
            assert!(peak(&l) < expected * 1.6, "auditions stacked: {}", peak(&l));
            let jump = l.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            assert!(jump < 0.05, "a retrigger clicked: {jump}");
        }
        process(&mut s, &[], 1_000);
        assert_eq!(s.preview.sounding(), 1, "a run of presses left voices behind");
    }

    /// A retrigger faster than the kill fade — three inside three
    /// milliseconds — has nowhere free to start and must take the quietest
    /// voice rather than panic or stack.
    #[test]
    fn a_retrigger_storm_stays_bounded() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 441_000));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);
        for _ in 0..40 {
            s.set_sampler_preview(Some(&preview(layer.clone(), PreviewMode::Once)));
            let (l, r) = process(&mut s, &[], 16); // a third of a millisecond
            for x in l.iter().chain(r.iter()) {
                assert!(x.is_finite() && x.abs() <= 1.0, "the storm left the rails: {x}");
            }
        }
        assert!(s.preview.sounding() <= preview::PREVIEW_VOICES);
        process(&mut s, &[], 441);
        assert_eq!(s.preview.sounding(), 1);
    }

    /// The audition obeys the two things that silence everything else: the
    /// panic gesture and a reset. A loop that survived either would be the
    /// one sound in the box with no key that stops it.
    #[test]
    fn all_sound_off_and_reset_both_end_an_audition() {
        let layer = PadLayer::from_pcm(sine_pcm(0.8, 441_000));
        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);

        s.set_sampler_preview(Some(&preview(layer.clone(), PreviewMode::Loop)));
        process(&mut s, &[], 512);
        process(&mut s, &[cc(120, 0)], 441);
        assert_eq!(s.preview.sounding(), 0, "a loop survived all-sound-off");
        // Nothing relights it: past the DC blocker's ring-down there is
        // silence, however long the loop had left to run.
        let (l, _) = process(&mut s, &[], 4_410);
        assert!(peak(&l[4_000..]) < 1e-3, "the loop relit itself after the panic");

        s.set_sampler_preview(Some(&preview(layer, PreviewMode::Loop)));
        process(&mut s, &[], 512);
        s.reset();
        assert_eq!(s.preview.sounding(), 0, "a loop survived a reset");
        // A reset zeroes the blockers too, so this one really is silence.
        let (l, _) = process(&mut s, &[], 4_410);
        assert_eq!(peak(&l), 0.0);
    }

    /// An audition of a layer with no frames behind it is silence, not a
    /// panic — the same answer the pad table gives an empty buffer.
    #[test]
    fn an_empty_buffer_auditions_as_silence() {
        let empty = Arc::new(SamplePcm { data: vec![], channels: 2, sample_rate: SR as f32 });
        let mut s = sampler_with(60, PadConfig::for_key(60), &[]);
        s.set_sampler_preview(Some(&preview(PadLayer::from_pcm(empty), PreviewMode::Loop)));
        let (l, _) = process(&mut s, &[], 2_048);
        assert_eq!(peak(&l), 0.0);
        assert_eq!(s.preview.sounding(), 0);
    }

    /// A pad with a fast envelope must not shape the audition: the strip
    /// asks "what is between the markers", and a 20 ms decay would answer
    /// with a tick whatever the markers said.
    #[test]
    fn an_audition_ignores_the_pads_envelope() {
        let layer = PadLayer::from_pcm(sine_pcm(0.5, 44_100));
        let mut cfg = PadConfig::for_key(60);
        cfg.attack_ms = 500.0;
        cfg.decay_ms = 20.0;
        cfg.sustain = 0.0;
        let mut s = sampler_with(60, cfg, &[]);
        s.set_sampler_preview(Some(&PreviewLayer {
            config: cfg,
            layer,
            mode: PreviewMode::Once,
        }));
        let (l, _) = process(&mut s, &[], 22_050);
        // Half a second in, a pad with that envelope is long dead and the
        // audition is still at its own level.
        let expected = 0.5 * core::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (peak(&l[20_000..]) - expected).abs() < 0.02,
            "the pad's envelope shaped the audition: {}",
            peak(&l[20_000..]),
        );
    }

    /// The command handler runs this on the audio thread, so starting,
    /// looping and stopping an audition must all stay off the allocator.
    #[test]
    fn an_audition_never_reaches_the_allocator() {
        let mut s = Sampler::new();
        s.init(SR, 512);
        let short = PadLayer::from_pcm(constant_pcm(0.5, 441)); // 10 ms: laps often
        let long = PadLayer::from_pcm(constant_pcm(0.5, 44_100));
        let mut l = vec![0.0f32; 512];
        let mut r = vec![0.0f32; 512];
        // Warm the voices: the first `start` on each slot fills its `Option`.
        s.set_sampler_preview(Some(&preview(long.clone(), PreviewMode::Loop)));
        {
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &[]);
        }
        let allocations = allocations_during(|| {
            s.set_sampler_preview(Some(&preview(short.clone(), PreviewMode::Loop)));
            for _ in 0..8 {
                let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                s.process(&[], &mut outs, &[]);
            }
            s.set_sampler_preview(Some(&preview(long.clone(), PreviewMode::Once)));
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &[]);
            s.set_sampler_preview(None);
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
            s.process(&[], &mut outs, &[]);
        });
        assert_eq!(allocations, 0, "the audition reached the allocator");
    }

    #[test]
    fn panel_metadata_is_complete_and_lowercase() {
        let s = Sampler::new();
        assert_eq!(s.parameter_count(), PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            let info = s.parameter_info(i).unwrap();
            assert_eq!(info.name, PARAM_NAMES[i]);
            assert_eq!(info.default, PARAM_DEFAULTS[i]);
        }
        assert!(s.parameter_info(PARAM_COUNT).is_none());
        for name in PARAM_NAMES {
            assert_eq!(name, name.to_lowercase());
            assert!(name.len() <= 8, "{name} will not fit a knob label");
        }
    }
}
