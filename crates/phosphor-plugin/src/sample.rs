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
//!
//! # One table of limits, here
//!
//! The pad geometry ([`NUM_PADS`], [`PAD_BASE_NOTE`], [`MAX_LAYERS`]), the
//! travel of every number on a pad ([`POLY_RANGE`], [`TUNE_ST_RANGE`] and the
//! rest), and the clamps that apply them ([`clamp_config`], [`clamp_layer`],
//! [`trim_region`], [`repair_vel`]) all live here, in the crate both sides
//! already depend on. They used to live in three places — the knob travel,
//! the session loader and the engine's delivery — and had already drifted on
//! three fields. Two clamps for one value is how the app comes to promise a
//! pitch the engine does not play.

use std::ops::RangeInclusive;
use std::sync::Arc;

// ── The pad geometry ──

/// One pad per piano key, A0..C8.
pub const NUM_PADS: usize = 88;

/// MIDI note of the lowest pad (A0).
pub const PAD_BASE_NOTE: u8 = 21;

/// Layer slots per pad. The UI enforces the same cap so that the player
/// hears "pad full" instead of a silently dropped ninth layer; the engine
/// enforces it again because a delivered list is not to be trusted.
pub const MAX_LAYERS: usize = 8;

/// The pad this note addresses, if any.
///
/// Notes off the 88-key bed are ignored rather than wrapped — a pad map that
/// aliases is worse than one that is silent at the extremes.
#[must_use]
pub fn pad_index(note: u8) -> Option<usize> {
    let last = PAD_BASE_NOTE + (NUM_PADS as u8 - 1);
    (PAD_BASE_NOTE..=last).contains(&note).then(|| usize::from(note - PAD_BASE_NOTE))
}

// ── The travel of every number on a pad ──

/// Simultaneous hits a pad allows. One is the default and one is the floor:
/// a poly of zero is room that cutting cannot make.
pub const POLY_RANGE: RangeInclusive<u8> = 1..=8;

/// The highest mute group. Zero is "no group", so this is a ceiling rather
/// than a range.
pub const CHOKE_MAX: u8 = 8;

/// Coarse pitch travel in semitones — a pad's and a layer's both, four
/// octaves either way.
pub const TUNE_ST_RANGE: RangeInclusive<i8> = -48..=48;

/// Fine pitch travel in cents, half a semitone either way.
pub const PITCH_CENTS_RANGE: RangeInclusive<i8> = -50..=50;

/// The top of every linear gain on a pad: a pad's level, a layer's, and a
/// phrase's velocity scale. +12 dB, which is also the ceiling a normalize
/// can reach — a gain past the end of the knob's travel is one the player
/// cannot turn back down by hand.
pub const MAX_GAIN: f32 = 4.0;

/// Pan travel, hard left to hard right.
pub const PAN_RANGE: RangeInclusive<f32> = -1.0..=1.0;

/// Bring a semitone offset inside [`TUNE_ST_RANGE`], from whatever width it
/// was worked out in.
///
/// Zone lending adds two roots together and the knobs step in `i32`, so the
/// arithmetic is wider than the field it lands in. This is the one door back
/// down, and it is what keeps the app from promising a pitch the engine
/// clamps away — the audit's C3, which was two clamps with two answers.
#[must_use]
pub fn clamp_tune_st(semitones: i32) -> i8 {
    semitones.clamp(i32::from(*TUNE_ST_RANGE.start()), i32::from(*TUNE_ST_RANGE.end())) as i8
}

/// The same for a fine offset, against [`PITCH_CENTS_RANGE`].
#[must_use]
pub fn clamp_pitch_cents(cents: i32) -> i8 {
    cents.clamp(i32::from(*PITCH_CENTS_RANGE.start()), i32::from(*PITCH_CENTS_RANGE.end())) as i8
}

/// Repair a delivered velocity window.
///
/// Inverted ranges are the interesting case: a UI that let a player drag the
/// low edge past the high one would otherwise deliver a sound that can never
/// be heard, so `hi` is lifted to `lo` rather than the pair being refused.
/// Shared by layers and phrases because a velocity window means the same
/// thing to both, and two repairs would eventually disagree.
#[must_use]
pub fn repair_vel(lo: u8, hi: u8) -> (u8, u8) {
    let lo = lo.min(127);
    (lo, hi.min(127).max(lo))
}

/// Whether a hit at `vel` falls inside a repaired window, inclusive.
#[inline]
#[must_use]
pub fn in_vel_window(lo: u8, hi: u8, vel: u8) -> bool {
    vel >= lo && vel <= hi
}

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
    /// Monophonic, last-note-wins across the whole instrument: like a
    /// one-shot, release is ignored, but the *next note played on any pad*
    /// cuts it. Only one mono voice ever sounds, and it always yields to
    /// whatever comes next — the behaviour of a monosynth bass or an 808
    /// glide, where a new note is meant to steal the one before it. A
    /// non-mono pad's note still cuts a sounding mono voice; the mono voice
    /// is the one that gives way.
    Mono,
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
    /// Round robin. Off, every layer that answers a hit sounds and they
    /// stack, which is what a layered kick is. On, exactly one of them
    /// answers and the next hit takes the next — the eight-snare pad that
    /// stops sounding like a machine gun.
    ///
    /// Sampled layers only. A phrase is note traffic for a shared instrument
    /// rather than one of several takes of a sound, so phrases are never
    /// rotated: every phrase on the pad fires either way.
    pub cycle: bool,
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
            cycle: false,
        }
    }
}

/// Bring every number on a pad inside the travel its control has.
///
/// The one table. It is called on the way out of a session file and again on
/// delivery to the engine, so a hand-edited `poly: 255` cannot open with a
/// pad the knob cannot reach *and* cannot reach the voice pool either.
///
/// The envelope times are floored at zero and not ceilinged: how long an
/// attack may be is the knob's travel rather than the data model's, and a
/// ceiling here would quietly rewrite a session written by a build whose dial
/// went further. Everything else is a fact about what the engine can play.
#[must_use]
pub fn clamp_config(config: PadConfig) -> PadConfig {
    PadConfig {
        trig: config.trig,
        poly: config.poly.clamp(*POLY_RANGE.start(), *POLY_RANGE.end()),
        choke: config.choke.min(CHOKE_MAX),
        pitch_st: clamp_tune_st(i32::from(config.pitch_st)),
        pitch_cents: clamp_pitch_cents(i32::from(config.pitch_cents)),
        attack_ms: config.attack_ms.max(0.0),
        decay_ms: config.decay_ms.max(0.0),
        sustain: config.sustain.clamp(0.0, 1.0),
        release_ms: config.release_ms.max(0.0),
        level: config.level.clamp(0.0, MAX_GAIN),
        pan: config.pan.clamp(*PAN_RANGE.start(), *PAN_RANGE.end()),
        root: config.root.min(127),
        keytrack: config.keytrack,
        cycle: config.cycle,
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

/// Bring one layer's own numbers inside their travel.
///
/// [`clamp_config`]'s sibling, and its trim is deliberately not here: a trim
/// is clamped against the buffer behind it rather than against a knob, which
/// is [`trim_region`]'s job and the one clamp that can answer "there is
/// nothing playable here at all".
#[must_use]
pub fn clamp_layer(layer: PadLayer) -> PadLayer {
    let (vel_lo, vel_hi) = repair_vel(layer.vel_lo, layer.vel_hi);
    PadLayer {
        gain: layer.gain.clamp(0.0, MAX_GAIN),
        pan: layer.pan.clamp(*PAN_RANGE.start(), *PAN_RANGE.end()),
        tune_st: clamp_tune_st(i32::from(layer.tune_st)),
        tune_cents: clamp_pitch_cents(i32::from(layer.tune_cents)),
        vel_lo,
        vel_hi,
        ..layer
    }
}

/// The playable region of a buffer, both ends pulled inside it.
///
/// **The one place a trim is clamped.** Everything downstream — the voice's
/// interpolator taps, its edge fades, its reverse start position, the strip's
/// waveform, its nudge bounds and its length readout — trusts
/// `start < end <= frames` completely, so `end < start`, a trim past the
/// buffer and an empty buffer are all caught here or not at all. The engine
/// and the trim strip both call it, because two copies of this arithmetic
/// agree right up until the day one of them is edited.
///
/// `None` when there is nothing playable: a buffer with no frames in it.
#[must_use]
pub fn trim_region(pcm: &SamplePcm, start_frame: u64, end_frame: u64) -> Option<(u64, u64)> {
    let frames = pcm.frames();
    if frames == 0 {
        return None;
    }
    let start = start_frame.min(frames - 1);
    // `start <= frames - 1`, so `start + 1 <= frames` and the clamp's bounds
    // are always the right way round.
    Some((start, end_frame.clamp(start + 1, frames)))
}

/// One note event of a recorded performance, at its offset in frames from
/// the start of that performance.
///
/// `status` is a note-on (`0x90`) or a note-off (`0x80`); the channel nibble
/// is ignored by the player, which addresses one instrument. A note-on with
/// `data2 == 0` is a note-off, the convention every MIDI source uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhraseEvent {
    /// Frames since the performance began.
    pub frame: u64,
    pub status: u8,
    /// Note number.
    pub data1: u8,
    /// Velocity.
    pub data2: u8,
}

/// A recorded performance stacked on a pad, played back through the
/// sampler's child instrument instead of being rendered to audio.
///
/// The memory-cheap sibling of a sampled layer: the capture machinery that
/// would have rendered a take to PCM keeps the plan instead, so a
/// four-bar phrase costs a few hundred events rather than a few megabytes.
///
/// # Frame offsets are baked at capture
///
/// `frame` is engine frames, not ticks — the tempo the phrase was played at
/// is baked into it exactly as it is baked into an audio take. A phrase does
/// not follow a tempo change, and that is the settled design rather than a
/// gap: a phrase is a recording, and the sibling it has to sound like is the
/// recording on the pad beside it.
///
/// It does follow a *device* change, which is what [`PadPhrase::sample_rate`]
/// is for: the frames were counted at one rate and may be played back at
/// another, and the sibling on the pad beside it has had that compensation
/// since day one ([`SamplePcm::sample_rate`]).
///
/// Ownership contract is [`SamplePcm`]'s: the side that builds the event
/// list keeps an `Arc` to it for as long as any plugin might hold one, so
/// every clone and drop on the audio thread is refcount-only.
#[derive(Debug, Clone)]
pub struct PadPhrase {
    pub events: Arc<[PhraseEvent]>,
    /// Performance length in frames — how long the pad plays before the
    /// phrase is over, which is not the same as the frame of its last
    /// note-off (a phrase can end in silence).
    pub frames: u64,
    /// Scales the velocity of every note the phrase sends, linear.
    ///
    /// Velocity rather than audio gain because the child instrument is
    /// shared by every phrase on every pad: there is one render per block
    /// for all of them, so there is no per-phrase place to put a fader.
    /// Scaling what is played is the control that survives that.
    pub gain: f32,
    /// When on, notes shift by the distance between the played key and the
    /// pad's root — the chromatic half of a phrase, the same bargain
    /// `keytrack` makes for a sampled layer.
    pub transpose_with_key: bool,
    pub mute: bool,
    /// Velocity range this phrase answers to, inclusive. Full range by
    /// default; here from day one so velocity-switched phrases never need
    /// a session migration.
    pub vel_lo: u8,
    pub vel_hi: u8,
    /// The engine rate the frames above were counted at, in Hz.
    ///
    /// **Zero means unknown**, and unknown plays as-is — which is exactly
    /// what every phrase written before this field existed wants, and the
    /// reason it is a sentinel rather than an `Option`: a session that never
    /// said anything about rates opens and plays the way it always has.
    ///
    /// Known, the runner scales the offsets by `sample_rate / engine_rate`,
    /// so a phrase captured on a 48 kHz device and opened on a 44.1 kHz one
    /// plays at the same speed rather than about 9% slow. The sampled sibling
    /// on the pad beside it has done this since day one; the phrase types
    /// did not, and a kit where the samples are right and the performances
    /// drag is worse than one where both are wrong together.
    pub sample_rate: f32,
}

impl PadPhrase {
    /// A phrase covering its whole event list at unity, answering every
    /// velocity and keyed to the pad it sits on.
    ///
    /// Its capture rate is unknown: the events do not carry one, and a
    /// guess would be a claim. The capture side stamps it — see
    /// [`PadPhrase::sample_rate`].
    pub fn from_events(events: Arc<[PhraseEvent]>, frames: u64) -> Self {
        Self {
            events,
            frames,
            gain: 1.0,
            transpose_with_key: false,
            mute: false,
            vel_lo: 0,
            vel_hi: 127,
            sample_rate: 0.0,
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
    fn a_phrase_from_events_answers_every_velocity_at_unity() {
        let events: Arc<[PhraseEvent]> =
            Arc::from(vec![PhraseEvent { frame: 0, status: 0x90, data1: 60, data2: 100 }]);
        let phrase = PadPhrase::from_events(events, 44_100);
        assert_eq!((phrase.vel_lo, phrase.vel_hi), (0, 127));
        assert_eq!(phrase.gain, 1.0);
        assert!(!phrase.mute);
        assert!(!phrase.transpose_with_key);
        assert_eq!(phrase.frames, 44_100);
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
    fn the_key_bed_maps_and_the_edges_do_not_wrap() {
        assert_eq!(pad_index(PAD_BASE_NOTE), Some(0));
        assert_eq!(pad_index(108), Some(NUM_PADS - 1));
        assert_eq!(pad_index(20), None);
        assert_eq!(pad_index(109), None);
        assert_eq!(pad_index(0), None);
        assert_eq!(pad_index(127), None);
    }

    #[test]
    fn a_wild_config_is_tamed_to_the_travel_of_its_controls() {
        let wild = PadConfig {
            poly: 0,
            choke: 200,
            pitch_st: 127,
            pitch_cents: -120,
            attack_ms: -5.0,
            decay_ms: -1.0,
            sustain: 9.0,
            release_ms: -3.0,
            level: 100.0,
            pan: -7.0,
            root: 250,
            ..PadConfig::for_key(60)
        };
        let tame = clamp_config(wild);
        assert_eq!(tame.poly, *POLY_RANGE.start());
        assert_eq!(tame.choke, CHOKE_MAX);
        assert_eq!(tame.pitch_st, *TUNE_ST_RANGE.end());
        assert_eq!(tame.pitch_cents, *PITCH_CENTS_RANGE.start());
        assert_eq!(tame.attack_ms, 0.0);
        assert_eq!(tame.decay_ms, 0.0);
        assert_eq!(tame.release_ms, 0.0);
        assert_eq!(tame.sustain, 1.0);
        assert_eq!(tame.level, MAX_GAIN);
        assert_eq!(tame.pan, *PAN_RANGE.start());
        assert_eq!(tame.root, 127);
        // A pad already inside its travel comes back untouched, field for
        // field: the clamp is a repair, never an opinion.
        let fresh = PadConfig::for_key(60);
        assert_eq!(clamp_config(fresh), fresh);
    }

    #[test]
    fn a_wild_layer_is_tamed_and_keeps_its_audio() {
        let pcm = Arc::new(SamplePcm { data: vec![0.0; 8], channels: 1, sample_rate: 44_100.0 });
        let wild = PadLayer {
            gain: 900.0,
            pan: 5.0,
            tune_st: 120,
            tune_cents: -120,
            vel_lo: 100,
            vel_hi: 20,
            ..PadLayer::from_pcm(Arc::clone(&pcm))
        };
        let tame = clamp_layer(wild);
        assert_eq!(tame.gain, MAX_GAIN);
        assert_eq!(tame.pan, *PAN_RANGE.end());
        assert_eq!(tame.tune_st, *TUNE_ST_RANGE.end());
        assert_eq!(tame.tune_cents, *PITCH_CENTS_RANGE.start());
        assert!(tame.vel_hi >= tame.vel_lo, "an inverted window survived");
        assert!(Arc::ptr_eq(&tame.pcm, &pcm), "the clamp copied the audio");
    }

    #[test]
    fn a_semitone_offset_lands_inside_the_field_it_is_written_to() {
        // The zone-lending case: two roots four octaves and more apart.
        assert_eq!(clamp_tune_st(21 - 108), *TUNE_ST_RANGE.start());
        assert_eq!(clamp_tune_st(108 - 21), *TUNE_ST_RANGE.end());
        assert_eq!(clamp_tune_st(-7), -7);
        assert_eq!(clamp_pitch_cents(9_000), *PITCH_CENTS_RANGE.end());
    }

    #[test]
    fn a_backwards_or_runaway_trim_is_pulled_inside_the_buffer() {
        let pcm = SamplePcm { data: vec![0.0; 100], channels: 1, sample_rate: 44_100.0 };
        assert_eq!(trim_region(&pcm, 0, 100), Some((0, 100)));
        // End before start: the region collapses to one frame, never inverts.
        assert_eq!(trim_region(&pcm, 90, 10), Some((90, 91)));
        // Both ends past the buffer.
        assert_eq!(trim_region(&pcm, 5_000, 9_000), Some((99, 100)));
        let empty = SamplePcm { data: Vec::new(), channels: 2, sample_rate: 44_100.0 };
        assert_eq!(trim_region(&empty, 0, 0), None);
    }

    #[test]
    fn a_velocity_window_answers_its_own_edges() {
        assert_eq!(repair_vel(100, 20), (100, 100));
        assert_eq!(repair_vel(200, 200), (127, 127));
        assert!(in_vel_window(0, 127, 0));
        assert!(in_vel_window(0, 127, 127));
        assert!(!in_vel_window(64, 127, 63));
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
