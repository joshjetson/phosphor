//! One playing sample.
//!
//! A voice reads a trimmed region of a shared buffer through a cubic
//! (Catmull-Rom) interpolator at a rate that combines the pad's pitch, the
//! layer's tune, keytracking, and the ratio between the recording's native
//! rate and the engine's. Linear interpolation was rejected up front: it
//! aliases audibly above about +7 semitones, and ±48 is on the panel.
//!
//! Three multipliers shape the amplitude, kept separate on purpose:
//!
//! * the ADSR — the *musical* envelope, the one on the panel;
//! * the edge fade — 2 ms of real time at the region boundaries, so a trim
//!   point dropped mid-waveform never clicks. Not a knob: it is the
//!   difference between "a sampler" and "broken", and it applies to both
//!   ends whichever direction the region plays;
//! * the kill fade — 3 ms, used when a voice is cut by poly, choke, a
//!   steal, or a stop. The replacement voice starts immediately in another
//!   slot; the dying one just fades under it.

use std::sync::Arc;

use phosphor_plugin::sample::{PadConfig, SamplePcm, TrigMode};

/// Edge fade at the trim boundaries, in milliseconds of *engine* time.
/// Measured in engine samples rather than source frames so a +48 st layer,
/// burning through source frames sixteen times as fast, still gets the
/// full 2 ms of real time.
pub(crate) const DECLICK_MS: f32 = 2.0;

/// Fade applied to a voice cut by poly, choke, steal, or stop.
pub(crate) const KILL_MS: f32 = 3.0;

/// Floor for the amp release. A zero release is a click with a knob on it.
pub(crate) const MIN_RELEASE_MS: f32 = 5.0;

/// `ln(1000)`: the decay/release one-poles are sized to cover 60 dB in the
/// time the panel names.
const LN_1000: f32 = 6.907_755;

/// Envelope level below which a decaying stage counts as finished.
const ENV_FLOOR: f32 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Dead,
}

/// Everything resolved about a layer at the moment it fires. The engine
/// builds this from its pad table; the voice keeps its own `Arc` so a
/// layer swapped out mid-note finishes on the buffer it started with.
pub(crate) struct LayerTrigger<'a> {
    pub pcm: &'a Arc<SamplePcm>,
    pub gain: f32,
    pub pan: f32,
    pub tune_st: i8,
    pub tune_cents: i8,
    /// Trimmed region in frames, already clamped: `start < end <= frames`.
    pub start: f64,
    pub end: f64,
    pub reverse: bool,
}

pub(crate) struct SamplerVoice {
    pcm: Option<Arc<SamplePcm>>,
    /// Pad index (0..88) this voice belongs to, for poly and choke.
    pub(crate) pad: usize,
    /// The note-on gesture that fired it. All layers of one hit share a
    /// hit id, so "poly 2" means two *hits*, not two voices.
    pub(crate) hit: u64,
    pub(crate) age: u64,
    trig: TrigMode,

    // Playback head. `pos` is in source frames, fractional.
    pos: f64,
    rate: f64,
    dir: f64,
    start: f64,
    end: f64,
    channels: usize,
    stereo_source: bool,

    // Envelope.
    stage: EnvStage,
    env: f32,
    att_step: f32,
    dec_coef: f32,
    sus: f32,
    rel_coef: f32,

    // Edge fade, in engine samples.
    elapsed: f64,
    fade_len: f64,
    inv_rate: f64,

    // Gains latched at trigger.
    gain_l: f32,
    gain_r: f32,

    // Kill fade.
    killing: bool,
    kill: f32,
    kill_step: f32,
}

impl SamplerVoice {
    pub(crate) fn new() -> Self {
        Self {
            pcm: None,
            pad: 0,
            hit: 0,
            age: 0,
            trig: TrigMode::OneShot,
            pos: 0.0,
            rate: 1.0,
            dir: 1.0,
            start: 0.0,
            end: 0.0,
            channels: 1,
            stereo_source: false,
            stage: EnvStage::Dead,
            env: 0.0,
            att_step: 1.0,
            dec_coef: 0.0,
            sus: 1.0,
            rel_coef: 0.0,
            elapsed: 0.0,
            fade_len: 1.0,
            inv_rate: 1.0,
            gain_l: 0.0,
            gain_r: 0.0,
            killing: false,
            kill: 1.0,
            kill_step: 1.0,
        }
    }

    /// Fire the voice. `vel_gain` is the velocity curve already applied by
    /// the engine, so the voice never needs the depth parameter.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        &mut self,
        note: u8,
        pad: usize,
        hit: u64,
        age: u64,
        cfg: &PadConfig,
        layer: &LayerTrigger,
        vel_gain: f32,
        engine_sr: f64,
    ) {
        let pcm = layer.pcm;
        self.pcm = Some(Arc::clone(pcm));
        self.pad = pad;
        self.hit = hit;
        self.age = age;
        self.trig = cfg.trig;
        self.channels = usize::from(pcm.channels.max(1));
        self.stereo_source = pcm.channels >= 2;

        // Pitch: pad coarse/fine + layer tune + keytracking, then the
        // native-rate ratio. Without the last term a 48 kHz file on a
        // 44.1 kHz device is 1.5 semitones sharp.
        let keytrack = if cfg.keytrack { f64::from(note) - f64::from(cfg.root) } else { 0.0 };
        let semis = f64::from(cfg.pitch_st) + f64::from(layer.tune_st) + keytrack
            + (f64::from(cfg.pitch_cents) + f64::from(layer.tune_cents)) / 100.0;
        let ratio = (semis / 12.0).exp2();
        self.rate = ratio * f64::from(pcm.sample_rate) / engine_sr;
        self.inv_rate = 1.0 / self.rate.max(1e-9);

        self.start = layer.start;
        self.end = layer.end;
        self.dir = if layer.reverse { -1.0 } else { 1.0 };
        self.pos = if layer.reverse { (layer.end - 1.0).max(layer.start) } else { layer.start };

        // Envelope. Times are floored at one sample so a zero on the
        // panel is instant, not a division by zero; the edge fade is what
        // keeps "instant" from clicking.
        let sr = engine_sr as f32;
        self.att_step = 1.0 / (cfg.attack_ms.max(0.0) * 1e-3 * sr).max(1.0);
        self.dec_coef = (-LN_1000 / (cfg.decay_ms.max(0.0) * 1e-3 * sr).max(1.0)).exp();
        self.sus = cfg.sustain.clamp(0.0, 1.0);
        self.rel_coef =
            (-LN_1000 / (cfg.release_ms.max(MIN_RELEASE_MS) * 1e-3 * sr).max(1.0)).exp();
        self.stage = EnvStage::Attack;
        self.env = 0.0;

        self.elapsed = 0.0;
        self.fade_len = (DECLICK_MS as f64 * 1e-3 * engine_sr).max(1.0);

        // Gains. A mono source pans with the equal-power law, which puts
        // it at -3 dB per side in the centre — the law that makes a mono
        // pad sit level with the stereo pad beside it. A stereo source
        // gets a balance instead: attenuate the far side, never boost.
        let total = layer.gain * cfg.level * vel_gain;
        let p = (cfg.pan + layer.pan).clamp(-1.0, 1.0);
        if self.stereo_source {
            self.gain_l = total * if p > 0.0 { 1.0 - p } else { 1.0 };
            self.gain_r = total * if p < 0.0 { 1.0 + p } else { 1.0 };
        } else {
            let theta = (p + 1.0) * core::f32::consts::FRAC_PI_4;
            self.gain_l = total * theta.cos();
            self.gain_r = total * theta.sin();
        }

        self.killing = false;
        self.kill = 1.0;
        self.kill_step = 1.0 / (KILL_MS as f64 * 1e-3 * engine_sr).max(1.0) as f32;
    }

    /// The key came up. Gates release; one-shots ignore it by definition.
    pub(crate) fn note_off(&mut self) {
        if self.trig == TrigMode::Gate
            && self.stage != EnvStage::Dead
            && self.stage != EnvStage::Release
        {
            self.stage = EnvStage::Release;
        }
    }

    /// Cut the voice with the 3 ms fade. Idempotent.
    pub(crate) fn kill(&mut self) {
        if self.stage != EnvStage::Dead {
            self.killing = true;
        }
    }

    /// Stop instantly, no fade. For `reset()` only, where the whole
    /// output is being silenced anyway.
    pub(crate) fn silence(&mut self) {
        self.stage = EnvStage::Dead;
        self.pcm = None;
    }

    /// Sounding at all, kill fade included.
    pub(crate) fn is_sounding(&self) -> bool {
        self.stage != EnvStage::Dead
    }

    /// Counts toward poly and the voice cap. A killing voice is already
    /// spoken for — it must not block its own replacement.
    pub(crate) fn is_active(&self) -> bool {
        self.stage != EnvStage::Dead && !self.killing
    }

    pub(crate) fn is_killing(&self) -> bool {
        self.killing && self.stage != EnvStage::Dead
    }

    /// How many engine samples are left before the head leaves the region,
    /// whichever direction it is travelling.
    ///
    /// What a loop preview schedules its next lap against: the lap has to
    /// start while this voice still has its closing edge fade to run, or the
    /// seam is a fade to silence and back rather than two ramps crossing.
    pub(crate) fn samples_to_exit(&self) -> f64 {
        if self.stage == EnvStage::Dead {
            return 0.0;
        }
        let to_exit = if self.dir > 0.0 { self.end - self.pos } else { self.pos - self.start };
        (to_exit * self.inv_rate).max(0.0)
    }

    /// Engine samples since the trigger. Reads as "has this voice been
    /// playing long enough to be worth lapping".
    pub(crate) fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// The edge fade's length in engine samples, as this voice was started.
    pub(crate) fn fade_len(&self) -> f64 {
        self.fade_len
    }

    /// Render one sample. Returns silence once dead.
    pub(crate) fn tick(&mut self) -> (f32, f32) {
        if self.stage == EnvStage::Dead {
            return (0.0, 0.0);
        }
        let Some(pcm) = self.pcm.as_ref() else {
            self.stage = EnvStage::Dead;
            return (0.0, 0.0);
        };

        let (raw_l, raw_r) = read_cubic(
            pcm,
            self.pos,
            self.start,
            self.end,
            self.channels,
            self.stereo_source,
        );

        // Edge fade, both measured in engine samples: elapsed since the
        // trigger against the way in, distance to the exit boundary
        // against the way out. `min` of the two handles a region shorter
        // than two fades without a special case.
        let to_exit = if self.dir > 0.0 { self.end - self.pos } else { self.pos - self.start };
        let fade_in = (self.elapsed / self.fade_len).min(1.0) as f32;
        let fade_out = ((to_exit * self.inv_rate) / self.fade_len).clamp(0.0, 1.0) as f32;
        let edge = fade_in.min(fade_out);

        let out = raw_l * self.env * edge * self.kill;
        let out_r = raw_r * self.env * edge * self.kill;
        let result = (out * self.gain_l, out_r * self.gain_r);

        // Advance the head; leaving the region ends the voice (the edge
        // fade has already taken it to zero on the way there).
        self.pos += self.rate * self.dir;
        self.elapsed += 1.0;
        if self.pos >= self.end || self.pos < self.start {
            self.stage = EnvStage::Dead;
            return result;
        }

        // Envelope.
        match self.stage {
            EnvStage::Attack => {
                self.env += self.att_step;
                if self.env >= 1.0 {
                    self.env = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                self.env = self.sus + (self.env - self.sus) * self.dec_coef;
                if (self.env - self.sus).abs() < 1e-4 {
                    self.env = self.sus;
                    self.stage = EnvStage::Sustain;
                }
                if self.env < ENV_FLOOR && self.sus < ENV_FLOOR {
                    self.stage = EnvStage::Dead;
                }
            }
            EnvStage::Sustain => {
                if self.sus < ENV_FLOOR {
                    self.stage = EnvStage::Dead;
                }
            }
            EnvStage::Release => {
                self.env *= self.rel_coef;
                if self.env < ENV_FLOOR {
                    self.stage = EnvStage::Dead;
                }
            }
            EnvStage::Dead => {}
        }

        // Kill fade rides on top of whatever the envelope is doing.
        if self.killing {
            self.kill -= self.kill_step;
            if self.kill <= 0.0 {
                self.kill = 0.0;
                self.stage = EnvStage::Dead;
            }
        }

        result
    }
}

/// Catmull-Rom read at fractional frame `pos`, taps clamped to the trimmed
/// region so the interpolator never leaks audio the trim cut off.
#[inline]
fn read_cubic(
    pcm: &SamplePcm,
    pos: f64,
    start: f64,
    end: f64,
    channels: usize,
    stereo: bool,
) -> (f32, f32) {
    let lo = start as usize;
    // `end` is exclusive and >= start + 1 by the pad table's clamping.
    let hi = (end as usize).saturating_sub(1).max(lo);
    let base = pos.floor();
    let t = (pos - base) as f32;
    let i = base as usize;

    let clamp = |f: usize| f.clamp(lo, hi);
    let i0 = clamp(i.saturating_sub(1));
    let i1 = clamp(i);
    let i2 = clamp(i + 1);
    let i3 = clamp(i + 2);

    let tap = |frame: usize, ch: usize| -> f32 {
        pcm.data.get(frame * channels + ch).copied().unwrap_or(0.0)
    };

    let interp = |p0: f32, p1: f32, p2: f32, p3: f32| -> f32 {
        let t2 = t * t;
        let t3 = t2 * t;
        0.5 * (2.0 * p1
            + (p2 - p0) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
    };

    if stereo {
        let l = interp(tap(i0, 0), tap(i1, 0), tap(i2, 0), tap(i3, 0));
        let r = interp(tap(i0, 1), tap(i1, 1), tap(i2, 1), tap(i3, 1));
        (l, r)
    } else {
        let m = interp(tap(i0, 0), tap(i1, 0), tap(i2, 0), tap(i3, 0));
        (m, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 44_100.0;

    fn mono_pcm(data: Vec<f32>, sample_rate: f32) -> Arc<SamplePcm> {
        Arc::new(SamplePcm { data, channels: 1, sample_rate })
    }

    fn trigger<'a>(pcm: &'a Arc<SamplePcm>) -> LayerTrigger<'a> {
        LayerTrigger {
            pcm,
            gain: 1.0,
            pan: 0.0,
            tune_st: 0,
            tune_cents: 0,
            start: 0.0,
            end: pcm.frames() as f64,
            reverse: false,
        }
    }

    fn flat_config() -> PadConfig {
        // Instant attack, full sustain: the envelope is a wire, so tests
        // see the interpolator and the edge fade alone.
        let mut cfg = PadConfig::for_key(60);
        cfg.attack_ms = 0.0;
        cfg.sustain = 1.0;
        cfg
    }

    fn run(voice: &mut SamplerVoice, n: usize) -> Vec<f32> {
        (0..n).map(|_| voice.tick().0).collect()
    }

    #[test]
    fn a_constant_buffer_reaches_its_own_level() {
        let pcm = mono_pcm(vec![0.5; 44_100], SR as f32);
        let cfg = flat_config();
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let out = run(&mut v, 2_000);
        // Past both the attack sample and the 2 ms edge fade, the value is
        // the source times the -3 dB centre law.
        let settled = out[500];
        assert!((settled - 0.5 * core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3, "{settled}");
    }

    #[test]
    fn the_edge_fade_removes_the_step_at_a_hard_start() {
        // A buffer that is instantly at full scale: the worst click case.
        let pcm = mono_pcm(vec![1.0; 44_100], SR as f32);
        let cfg = flat_config();
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let out = run(&mut v, 200);
        let max_jump = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // 2 ms at 44.1 kHz is ~88 samples; a smooth ramp to 0.707 moves
        // under 0.01 per sample. A click would be a jump near 0.7.
        assert!(max_jump < 0.02, "step of {max_jump} at the region start");
        assert!(out[0].abs() < 0.02, "first sample {} is not near silence", out[0]);
    }

    #[test]
    fn plus_twelve_semitones_reads_twice_as_fast() {
        let frames = 8_820; // 200 ms
        let pcm = mono_pcm(vec![0.25; frames], SR as f32);
        let mut cfg = flat_config();
        cfg.pitch_st = 12;
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let out = run(&mut v, frames);
        let last = out.iter().rposition(|s| s.abs() > 1e-6).unwrap();
        let expected = frames / 2;
        assert!(
            (last as i64 - expected as i64).abs() < 16,
            "voice died at {last}, expected about {expected}"
        );
    }

    #[test]
    fn a_48k_file_on_a_44_1k_engine_lands_events_at_real_time() {
        // An impulse 100 ms into a 48 kHz recording must come out 100 ms
        // into engine time, or the whole kit plays sharp.
        let native = 48_000.0f32;
        let mut data = vec![0.0f32; 24_000];
        data[4_800] = 1.0; // 100 ms at 48 kHz
        // Surround the impulse so the edge fade is long gone.
        let pcm = mono_pcm(data, native);
        let cfg = flat_config();
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let out = run(&mut v, 8_000);
        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0;
        let expected = (0.1 * SR) as i64; // 100 ms of engine samples
        assert!(
            (peak as i64 - expected).abs() < 8,
            "impulse at {peak}, expected about {expected}"
        );
    }

    #[test]
    fn keytracking_transposes_by_distance_from_root() {
        let frames = 4_410;
        let pcm = mono_pcm(vec![0.25; frames], SR as f32);
        let mut cfg = flat_config();
        cfg.keytrack = true;
        cfg.root = 60;
        let mut v = SamplerVoice::new();
        // An octave above the root: double speed, half duration.
        v.start(72, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let out = run(&mut v, frames);
        let last = out.iter().rposition(|s| s.abs() > 1e-6).unwrap();
        assert!((last as i64 - (frames / 2) as i64).abs() < 16, "died at {last}");
    }

    #[test]
    fn without_keytracking_every_key_is_the_same_speed() {
        let frames = 4_410;
        let pcm = mono_pcm(vec![0.25; frames], SR as f32);
        let cfg = flat_config();
        let mut hi = SamplerVoice::new();
        hi.start(108, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let mut lo = SamplerVoice::new();
        lo.start(21, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let out_hi = run(&mut hi, frames + 100);
        let out_lo = run(&mut lo, frames + 100);
        assert_eq!(
            out_hi.iter().rposition(|s| s.abs() > 1e-6),
            out_lo.iter().rposition(|s| s.abs() > 1e-6),
        );
    }

    #[test]
    fn reverse_plays_the_trimmed_region_backwards() {
        // A ramp: forward playback rises, reverse playback falls.
        let frames = 4_410;
        let data: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        let pcm = mono_pcm(data, SR as f32);
        let cfg = flat_config();

        let mut fwd = SamplerVoice::new();
        fwd.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        let f = run(&mut fwd, 2_000);

        let mut layer = trigger(&pcm);
        layer.reverse = true;
        let mut rev = SamplerVoice::new();
        rev.start(60, 0, 0, 0, &cfg, &layer, 1.0, SR);
        let r = run(&mut rev, 2_000);

        // Compare well inside the fades: forward is quiet early and loud
        // late; reverse is the mirror.
        assert!(f[1_800] > f[300], "forward should rise: {} vs {}", f[1_800], f[300]);
        assert!(r[300] > r[1_800], "reverse should fall: {} vs {}", r[300], r[1_800]);
    }

    #[test]
    fn trim_start_skips_the_cut_material() {
        // First half zeros, second half at 0.5. Trimming to the second
        // half must sound immediately (after the edge fade), not after
        // half the file.
        let frames = 8_820;
        let mut data = vec![0.0f32; frames];
        for s in data.iter_mut().skip(frames / 2) {
            *s = 0.5;
        }
        let pcm = mono_pcm(data, SR as f32);
        let cfg = flat_config();
        let mut layer = trigger(&pcm);
        layer.start = (frames / 2) as f64;
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &layer, 1.0, SR);
        let out = run(&mut v, 600);
        assert!(out[500].abs() > 0.3, "trimmed start is silent: {}", out[500]);
    }

    #[test]
    fn a_gate_voice_releases_and_a_one_shot_does_not() {
        let pcm = mono_pcm(vec![0.5; 44_100], SR as f32);
        let mut cfg = flat_config();
        cfg.release_ms = 20.0;

        cfg.trig = TrigMode::Gate;
        let mut gate = SamplerVoice::new();
        gate.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        run(&mut gate, 1_000);
        gate.note_off();
        // 20 ms release covers 60 dB in ~882 samples; well after that the
        // voice is dead.
        run(&mut gate, 4_000);
        assert!(!gate.is_sounding(), "gate voice survived its release");

        cfg.trig = TrigMode::OneShot;
        let mut shot = SamplerVoice::new();
        shot.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        run(&mut shot, 1_000);
        shot.note_off();
        run(&mut shot, 4_000);
        assert!(shot.is_sounding(), "one-shot obeyed a note-off");
    }

    #[test]
    fn the_release_floor_holds_even_at_zero() {
        let pcm = mono_pcm(vec![0.5; 44_100], SR as f32);
        let mut cfg = flat_config();
        cfg.trig = TrigMode::Gate;
        cfg.release_ms = 0.0;
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        run(&mut v, 1_000);
        let before = v.tick().0;
        v.note_off();
        // The output is computed before the envelope advances, so the
        // sample that shows the release's first step is the second one.
        v.tick();
        let after = v.tick().0;
        // With the 5 ms floor the level moves ~3% per sample; a raw 0 ms
        // release would already be at -60 dB.
        assert!(after > before * 0.9, "release jumped: {before} -> {after}");
    }

    #[test]
    fn a_kill_is_a_fade_not_a_truncation() {
        let pcm = mono_pcm(vec![1.0; 44_100], SR as f32);
        let cfg = flat_config();
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        run(&mut v, 1_000);
        v.kill();
        let out = run(&mut v, 300);
        let max_jump = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_jump < 0.02, "kill stepped by {max_jump}");
        assert!(!v.is_sounding(), "kill fade never finished");
        // 3 ms at 44.1 kHz is ~132 samples.
        let dead_from = out.iter().rposition(|s| s.abs() > 1e-6).unwrap();
        assert!((100..200).contains(&dead_from), "fade lasted {dead_from} samples");
    }

    #[test]
    fn degenerate_regions_do_not_panic() {
        // One frame.
        let pcm = mono_pcm(vec![0.7], SR as f32);
        let cfg = flat_config();
        let mut layer = trigger(&pcm);
        layer.end = 1.0;
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        v.start(60, 0, 0, 0, &cfg, &layer, 1.0, SR);
        run(&mut v, 64);
        assert!(!v.is_sounding());

        // Reversed one-frame region.
        layer.reverse = true;
        v.start(60, 0, 0, 0, &cfg, &layer, 1.0, SR);
        run(&mut v, 64);
        assert!(!v.is_sounding());
    }

    #[test]
    fn extreme_transposition_survives_the_whole_range() {
        let pcm = mono_pcm(vec![0.5; 4_410], SR as f32);
        for st in [-48i8, -12, 0, 12, 48] {
            let mut cfg = flat_config();
            cfg.pitch_st = st;
            cfg.pitch_cents = 50;
            let mut v = SamplerVoice::new();
            v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
            let out = run(&mut v, 20_000);
            assert!(
                out.iter().all(|s| s.is_finite() && s.abs() <= 1.0),
                "±{st} st produced a bad sample"
            );
        }
    }

    #[test]
    fn a_stereo_source_keeps_its_channels_apart() {
        // Left ramps up, right holds at -0.5.
        let frames = 4_410usize;
        let mut data = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            data.push(i as f32 / frames as f32);
            data.push(-0.5);
        }
        let pcm = Arc::new(SamplePcm { data, channels: 2, sample_rate: SR as f32 });
        let cfg = flat_config();
        let layer = LayerTrigger {
            pcm: &pcm,
            gain: 1.0,
            pan: 0.0,
            tune_st: 0,
            tune_cents: 0,
            start: 0.0,
            end: frames as f64,
            reverse: false,
        };
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &layer, 1.0, SR);
        for _ in 0..500 {
            v.tick();
        }
        let (l, r) = v.tick();
        assert!(l > 0.0 && r < 0.0, "channels merged: l={l} r={r}");
    }

    #[test]
    fn pan_law_mono_centre_sits_three_db_down() {
        let pcm = mono_pcm(vec![1.0; 44_100], SR as f32);
        let cfg = flat_config();
        let mut v = SamplerVoice::new();
        v.start(60, 0, 0, 0, &cfg, &trigger(&pcm), 1.0, SR);
        for _ in 0..500 {
            v.tick();
        }
        let (l, r) = v.tick();
        assert!((l - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((r - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    }
}
