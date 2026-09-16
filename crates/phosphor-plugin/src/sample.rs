//! Sample data shared between the application, which loads and edits it,
//! and a sampler plugin, which plays it.
//!
//! These types live here rather than in the DSP crate because they are the
//! *interface*: the app builds them (decoding files, capturing takes), the
//! mixer carries them, and the sampler copies them into its own storage.
//! PCM travels behind an [`Arc`] so that handing a buffer to the audio
//! thread is a refcount increment, never a copy, and so that a future
//! slice/chop feature is just two pads pointing into one buffer.
//!
//! Ownership contract: the side that builds a [`SamplePcm`] (the UI thread)
//! keeps at least one `Arc` to it — in its own state or in undo history —
//! for as long as any plugin might hold one. Under that rule every clone
//! and drop that happens on the audio thread is refcount-only; the actual
//! free always lands on a thread that is allowed to take its time.

use std::sync::Arc;

/// Decoded audio, interleaved, at its native rate.
///
/// `sample_rate` is the rate of the *recording*, not of the engine. A
/// player must resample by `sample_rate / engine_rate` or a 48 kHz file
/// on a 44.1 kHz device plays about 1.5 semitones sharp.
#[derive(Debug, Clone, PartialEq)]
pub struct SamplePcm {
    /// Interleaved samples: `data[frame * channels + channel]`.
    pub data: Vec<f32>,
    /// 1 (mono) or 2 (stereo).
    pub channels: u16,
    /// Native rate of the recording in Hz.
    pub sample_rate: f32,
}

impl SamplePcm {
    /// Number of frames (samples per channel).
    pub fn frames(&self) -> u64 {
        (self.data.len() / usize::from(self.channels.max(1))) as u64
    }
}

/// What a key press means to a pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrigMode {
    /// Key release is ignored; the trimmed region plays to its end.
    OneShot,
    /// Sounds while the key is held; release runs the amp release.
    Gate,
}

/// Everything a pad is, apart from its layers.
///
/// Values are in natural units, not normalized: this struct bypasses the
/// parameter system entirely and is delivered whole, so there is nothing
/// to be gained from 0..1 and a lot of clarity to be lost.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadConfig {
    pub trig: TrigMode,
    /// Simultaneous hits allowed on this pad, 1..=8. A new hit past the
    /// limit cuts the oldest one (with a fade — never a truncation).
    pub poly: u8,
    /// Mute group, 0 = none. A hit kills every sounding voice on *other*
    /// pads that share its group: the closed hat stopping the open one.
    pub choke: u8,
    /// Coarse pitch in semitones, ±48.
    pub pitch_st: i8,
    /// Fine pitch in cents, ±50.
    pub pitch_cents: i8,
    pub attack_ms: f32,
    pub decay_ms: f32,
    /// Sustain level, 0..1.
    pub sustain: f32,
    /// Floored at 5 ms by the engine: a zero release is a click.
    pub release_ms: f32,
    /// Pad gain, linear.
    pub level: f32,
    /// Pad pan, -1..1.
    pub pan: f32,
    /// Reference key: playing `root` sounds the sample untransposed when
    /// `keytrack` is on. Defaults to the pad's own key.
    pub root: u8,
    /// When on, the played key transposes the sample by its distance from
    /// `root` — the chromatic half of the sampler. Off, every key on the
    /// pad sounds identical.
    pub keytrack: bool,
}

impl PadConfig {
    /// The defaults a fresh pad gets: a one-shot drum pad rooted at its
    /// own key. `poly: 1` is deliberate — retrigger-cuts-previous is why
    /// MPC hats sound tight, and a 2-second 808 played in 16ths must not
    /// stack sixteen copies of itself.
    pub fn for_key(note: u8) -> Self {
        Self {
            trig: TrigMode::OneShot,
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
            root: note,
            keytrack: false,
        }
    }
}

/// One sound stacked on a pad.
///
/// Trim is non-destructive: `start_frame..end_frame` select into the
/// shared buffer and the buffer is never rewritten. `reverse` plays the
/// trimmed region backwards — the region, not the whole file, so the trim
/// points keep their meaning either way.
#[derive(Debug, Clone)]
pub struct PadLayer {
    pub pcm: Arc<SamplePcm>,
    /// Layer gain, linear.
    pub gain: f32,
    /// Layer pan, -1..1, combined with the pad's.
    pub pan: f32,
    /// Per-layer tuning on top of the pad's, ±48 semitones.
    pub tune_st: i8,
    /// ±50 cents.
    pub tune_cents: i8,
    /// First frame of the playable region.
    pub start_frame: u64,
    /// One past the last playable frame; clamped to the buffer.
    pub end_frame: u64,
    pub reverse: bool,
    pub mute: bool,
    /// Velocity range this layer answers to, inclusive. Full range by
    /// default; here from day one so velocity-switched layers never need
    /// a session migration.
    pub vel_lo: u8,
    pub vel_hi: u8,
}

impl PadLayer {
    /// A layer covering the whole buffer at unity.
    pub fn from_pcm(pcm: Arc<SamplePcm>) -> Self {
        let end_frame = pcm.frames();
        Self {
            pcm,
            gain: 1.0,
            pan: 0.0,
            tune_st: 0,
            tune_cents: 0,
            start_frame: 0,
            end_frame,
            reverse: false,
            mute: false,
            vel_lo: 0,
            vel_hi: 127,
        }
    }
}

/// How long an auditioned layer plays for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    /// The trimmed region once, then silence.
    Once,
    /// The trimmed region round and round, its seams lapped, until the UI
    /// says stop. What the trim strip plays while an edge is being found.
    Loop,
}

/// One layer sounded on its own, for the UI to listen to.
///
/// The whole layer travels rather than an index into the pad table, and the
/// reason is the missing-file rule: a pad's engine-side layers are only the
/// ones that have audio behind them, so the third row of the list and the
/// third slot in the engine are not the same layer whenever a file has gone
/// missing above them. An index would audition the wrong sound in exactly
/// the situation a player is trying to sort out.
///
/// The `Arc` inside `layer` is a refcount handle like every other one that
/// crosses to the audio thread: the UI keeps its own reference, so the
/// clone and the drop on the far side never reach the allocator.
#[derive(Debug, Clone)]
pub struct PreviewLayer {
    /// The pad the layer sits on — its pitch, level and pan are part of
    /// what the sound *is*, so they travel. Its envelope does not; see
    /// the sampler's preview path for why.
    pub config: PadConfig,
    pub layer: PadLayer,
    pub mode: PreviewMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_counts_per_channel() {
        let stereo = SamplePcm { data: vec![0.0; 10], channels: 2, sample_rate: 44_100.0 };
        assert_eq!(stereo.frames(), 5);
        let mono = SamplePcm { data: vec![0.0; 10], channels: 1, sample_rate: 44_100.0 };
        assert_eq!(mono.frames(), 10);
    }

    #[test]
    fn zero_channels_cannot_divide_by_zero() {
        let broken = SamplePcm { data: vec![0.0; 10], channels: 0, sample_rate: 44_100.0 };
        assert_eq!(broken.frames(), 10);
    }

    #[test]
    fn a_fresh_pad_is_a_drum_pad_rooted_at_its_key() {
        let pad = PadConfig::for_key(60);
        assert_eq!(pad.trig, TrigMode::OneShot);
        assert_eq!(pad.poly, 1);
        assert_eq!(pad.root, 60);
        assert!(!pad.keytrack);
    }

    #[test]
    fn a_layer_from_pcm_spans_the_whole_buffer() {
        let pcm = Arc::new(SamplePcm { data: vec![0.0; 8], channels: 2, sample_rate: 48_000.0 });
        let layer = PadLayer::from_pcm(pcm);
        assert_eq!(layer.start_frame, 0);
        assert_eq!(layer.end_frame, 4);
        assert_eq!((layer.vel_lo, layer.vel_hi), (0, 127));
        assert!(!layer.mute);
    }
}
