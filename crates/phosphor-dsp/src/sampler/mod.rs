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

mod pad;
mod voice;

pub use pad::{pad_index, MAX_LAYERS, NUM_PADS, PAD_BASE_NOTE};

use std::sync::Arc;

use phosphor_plugin::sample::{PadConfig, PadLayer};
use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

use crate::level::soft_saturate;
use pad::Pad;
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
            dc_l: DcBlocker::new(),
            dc_r: DcBlocker::new(),
        }
    }

    fn note_on(&mut self, note: u8, vel: u8) {
        let Some(pad_idx) = pad_index(note) else { return };
        let depth = self.params[P_VEL];
        let v = f32::from(vel) / 127.0;
        let vel_gain = 1.0 + depth * (v * v - 1.0);

        let (poly, choke) = {
            let cfg = &self.pads[pad_idx].config;
            (cfg.poly, cfg.choke)
        };

        // Choke: this hit silences the rest of its mute group.
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
        }

        // Poly: make room for this hit among the pad's own.
        self.enforce_poly(pad_idx, poly);

        self.hit_counter += 1;
        let hit = self.hit_counter;

        for layer_idx in 0..MAX_LAYERS {
            let Some(trigger) = ({
                let pad = &self.pads[pad_idx];
                pad.layers[layer_idx].as_ref().and_then(|slot| {
                    if slot.mute || vel < slot.vel_lo || vel > slot.vel_hi {
                        None
                    } else {
                        Some(voice::LayerTrigger {
                            pcm: &slot.pcm,
                            gain: slot.gain,
                            pan: slot.pan,
                            tune_st: slot.tune_st,
                            tune_cents: slot.tune_cents,
                            start: slot.start,
                            end: slot.end,
                            reverse: slot.reverse,
                        })
                    }
                })
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

    /// Cut the oldest hits on a pad until a new one fits under `poly`.
    fn enforce_poly(&mut self, pad_idx: usize, poly: u8) {
        loop {
            let mut hits = 0usize;
            let mut oldest = u64::MAX;
            for i in 0..self.voices.len() {
                let v = &self.voices[i];
                if !v.is_active() || v.pad != pad_idx {
                    continue;
                }
                let seen = self.voices[..i]
                    .iter()
                    .any(|e| e.is_active() && e.pad == pad_idx && e.hit == v.hit);
                if !seen {
                    hits += 1;
                    oldest = oldest.min(v.hit);
                }
            }
            if hits < usize::from(poly) {
                return;
            }
            for v in &mut self.voices {
                if v.is_active() && v.pad == pad_idx && v.hit == oldest {
                    v.kill();
                }
            }
        }
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

    fn note_off(&mut self, note: u8) {
        let Some(pad_idx) = pad_index(note) else { return };
        for v in &mut self.voices {
            if v.pad == pad_idx {
                v.note_off();
            }
        }
    }

    fn kill_all(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
    }

    fn release_all(&mut self) {
        for v in &mut self.voices {
            v.note_off();
        }
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

    fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
        self.sample_rate = sample_rate;
        self.voices = (0..VOICE_POOL).map(|_| SamplerVoice::new()).collect();
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

        for i in 0..buf_len {
            while ei < event_count && midi_events[event_indices[ei]].sample_offset as usize <= i {
                let ev = &midi_events[event_indices[ei]];
                match ev.status & 0xF0 {
                    0x90 => {
                        if ev.data2 > 0 {
                            self.note_on(ev.data1, ev.data2);
                        } else {
                            self.note_off(ev.data1);
                        }
                    }
                    0x80 => self.note_off(ev.data1),
                    0xB0 => match ev.data1 {
                        // All sound off: the panic gesture and the
                        // transport's stop edge. A fade, not a truncation
                        // — but a fast one.
                        120 => self.kill_all(),
                        // All notes off: gates release musically. A
                        // one-shot ignores a note-off by definition, and
                        // this is a note-off.
                        123 => self.release_all(),
                        _ => {}
                    },
                    _ => {}
                }
                ei += 1;
            }

            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;
            for v in &mut self.voices {
                if v.is_sounding() {
                    let (l, r) = v.tick();
                    sum_l += l;
                    sum_r += r;
                }
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
        self.dc_l.reset();
        self.dc_r.reset();
        self.age_counter = 0;
        self.hit_counter = 0;
    }

    fn set_sampler_pad(&mut self, pad: u8, config: &PadConfig, layers: &[PadLayer]) {
        if let Some(p) = self.pads.get_mut(usize::from(pad)) {
            p.set(config, layers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::tests::allocations_during;
    use phosphor_plugin::sample::{SamplePcm, TrigMode};

    const SR: f64 = 44_100.0;

    fn note_on(note: u8, vel: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x90, data1: note, data2: vel }
    }

    fn note_off(note: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0x80, data1: note, data2: 0 }
    }

    fn cc(controller: u8, offset: u32) -> MidiEvent {
        MidiEvent { sample_offset: offset, status: 0xB0, data1: controller, data2: 0 }
    }

    fn constant_pcm(value: f32, frames: usize) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data: vec![value; frames], channels: 1, sample_rate: SR as f32 })
    }

    /// A 220 Hz sine. Level tests that go through `process` must use an
    /// audio-band signal: the DC blocker is *supposed* to bleed a
    /// constant buffer to nothing, so a constant is only fit for testing
    /// the blocker itself.
    fn sine_pcm(amp: f32, frames: usize) -> Arc<SamplePcm> {
        let data = (0..frames)
            .map(|i| amp * (core::f32::consts::TAU * 220.0 * i as f32 / SR as f32).sin())
            .collect();
        Arc::new(SamplePcm { data, channels: 1, sample_rate: SR as f32 })
    }

    fn sampler_with(pad_note: u8, config: PadConfig, layers: &[PadLayer]) -> Sampler {
        let mut s = Sampler::new();
        s.init(SR, 512);
        s.set_sampler_pad(pad_note - PAD_BASE_NOTE, &config, layers);
        s
    }

    fn load(s: &mut Sampler, pad_note: u8, config: PadConfig, layers: &[PadLayer]) {
        s.set_sampler_pad(pad_note - PAD_BASE_NOTE, &config, layers);
    }

    /// Run one stereo block and return (left, right).
    fn process(s: &mut Sampler, events: &[MidiEvent], n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        {
            let mut outs: Vec<&mut [f32]> = vec![&mut l, &mut r];
            s.process(&[], &mut outs, events);
        }
        (l, r)
    }

    fn peak(buf: &[f32]) -> f32 {
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
