//! The replay clip: the notes that play a chop back the way it was
//! recorded.
//!
//! One note per slice, on the slice's key, at the time the slice began in
//! the recording and as long as the slice lasts. Played as it lands, the
//! clip *is* the recording again; then every note is a hit that can be
//! moved, dropped, doubled or swapped for another key — the reason players
//! chop a break in the first place.
//!
//! # Time
//!
//! Seconds become ticks at the tempo the song is at now. The slices play at
//! their own speed whatever the tempo — the sampler does not stretch them —
//! so this is the one placement under which the replay sounds exactly like
//! the recording. A break recorded at another tempo still plays right; it
//! only lines up with the song's bars when the song is at its tempo, which
//! is what [`Replay::bars_exact`] is for telling the player.
//!
//! The first slice is time zero, not the recording's first frame: the clip
//! starts on a bar line, and the first hit belongs on it rather than after
//! whatever lead-in the recording had.
//!
//! # Velocity
//!
//! How loud each slice's first 50 ms is, against the loudest slice: the
//! loudest plays at 127, and each 10 dB quieter is 29 less, down to a floor.
//! The sampler scales a voice by its velocity, so a ghost note replays as a
//! ghost note — and a velocity-switched pad would pick the same layer the
//! drummer's hand did.

use phosphor_core::clip::NoteSnapshot;
use phosphor_core::transport::Transport;
use phosphor_plugin::sample::SamplePcm;

use super::bands::mono_at;
use super::markers::ms_to_frames;
use crate::sampler::SamplerState;
use crate::state::Clip;
use crate::timeline::{bars_covering, next_free_bar, TICKS_PER_BAR};

/// How much of a slice its loudness is judged over — the hit, not its tail.
const ATTACK_MS: f32 = 50.0;

/// Velocity lost per decibel under the loudest slice.
const VELOCITY_PER_DB: f32 = 2.9;

/// No replayed note quieter than this: a slice that is nearly silent is
/// still a slice the player chose to keep, and a velocity of 1 is a note
/// nobody hears.
const VELOCITY_FLOOR: u8 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    /// Where on the timeline the clip goes: the first bar at or after the
    /// playhead with room for it.
    pub start_tick: i64,
    /// Whole bars, long enough for the last slice to finish.
    pub length_ticks: i64,
    pub notes: Vec<NoteSnapshot>,
    /// How many bars the chop spans at this tempo, unrounded — `1.0` when a
    /// one-bar break is at the song's tempo, `1.33` when it is a 90 BPM bar
    /// in a 120 BPM song: slower playing lasts longer.
    pub bars_exact: f64,
}

impl Replay {
    /// The bar a player would call it, counting from one.
    pub fn bar(&self) -> i64 {
        self.start_tick / TICKS_PER_BAR + 1
    }

    /// The clip, ready to be placed.
    pub fn clip(&self) -> Clip {
        Clip::of_notes(self.start_tick, self.length_ticks, self.notes.clone())
    }
}

/// Loudness of the first [`ATTACK_MS`] of a slice, in dB. `None` when it is
/// silent through.
fn attack_db(pcm: &SamplePcm, (start, end): (u64, u64)) -> Option<f32> {
    let span = ms_to_frames(ATTACK_MS, pcm.sample_rate).max(1);
    let to = end.min(start + span).max(start + 1);
    let sum: f64 = (start..to).map(|f| f64::from(mono_at(pcm, f)).powi(2)).sum();
    let mean = sum / (to - start) as f64;
    (mean > 0.0).then(|| (10.0 * mean.log10()) as f32)
}

/// The velocity of a slice `below` dB under the loudest.
fn velocity(below: Option<f32>) -> u8 {
    match below {
        Some(db) => (127.0 - db.max(0.0) * VELOCITY_PER_DB)
            .round()
            .clamp(f32::from(VELOCITY_FLOOR), 127.0) as u8,
        None => VELOCITY_FLOOR,
    }
}

/// The replay of `slices` of `pcm`, landed from pad `first`, at `bpm`, for
/// a track holding `clips` with the playhead at `playhead`. `None` when
/// there is nothing to replay.
pub fn replay(
    pcm: &SamplePcm,
    slices: &[(u64, u64)],
    first: usize,
    bpm: f64,
    playhead: i64,
    clips: &[Clip],
) -> Option<Replay> {
    let origin = slices.first()?.0;
    let ticks_per_frame =
        bpm.clamp(1.0, 1_000.0) / 60.0 * Transport::PPQ as f64 / f64::from(pcm.sample_rate.max(1.0));
    let to_ticks = |frames: u64| (frames as f64 * ticks_per_frame).round() as i64;

    let loudness: Vec<Option<f32>> = slices.iter().map(|&s| attack_db(pcm, s)).collect();
    let loudest = loudness.iter().flatten().copied().fold(f32::MIN, f32::max);

    let notes: Vec<NoteSnapshot> = slices
        .iter()
        .zip(&loudness)
        .enumerate()
        .map(|(k, (&(start, end), db))| NoteSnapshot {
            note: SamplerState::note_of_pad(first + k),
            velocity: velocity(db.map(|db| loudest - db)),
            start_tick: to_ticks(start - origin),
            duration_ticks: to_ticks(end - start).max(1),
            muted: false,
        })
        .collect();

    let span = notes.iter().map(|n| n.start_tick + n.duration_ticks).max()?;
    let length_ticks = bars_covering(span).max(1) * TICKS_PER_BAR;
    Some(Replay {
        start_tick: next_free_bar(clips, playhead, length_ticks),
        length_ticks,
        notes,
        bars_exact: span as f64 / TICKS_PER_BAR as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Two seconds of a steady tone, then silence: loud where asked.
    fn pcm(loud: &[(usize, usize, f32)]) -> SamplePcm {
        let mut data = vec![0.0f32; RATE as usize * 4];
        for &(from, to, gain) in loud {
            for (i, s) in data[from..to].iter_mut().enumerate() {
                *s = gain * (std::f32::consts::TAU * 440.0 * i as f32 / RATE).sin();
            }
        }
        SamplePcm { data, channels: 1, sample_rate: RATE }
    }

    fn c2() -> usize {
        SamplerState::pad_of_note(36).unwrap()
    }

    /// At 120 BPM a beat is half a second: 24,000 frames. Slices a beat
    /// apart replay a beat apart, on keys upward from C2.
    #[test]
    fn each_slice_replays_on_its_key_at_its_own_time() {
        let source = pcm(&[(0, RATE as usize * 2, 0.5)]);
        let slices = [(1_000, 25_000), (25_000, 49_000), (49_000, 97_000)];
        let r = replay(&source, &slices, c2(), 120.0, 0, &[]).unwrap();
        let beat = Transport::PPQ;
        assert_eq!(r.notes.iter().map(|n| n.note).collect::<Vec<_>>(), vec![36, 37, 38]);
        assert_eq!(r.notes.iter().map(|n| n.start_tick).collect::<Vec<_>>(), vec![0, beat, 2 * beat]);
        assert_eq!(r.notes.iter().map(|n| n.duration_ticks).collect::<Vec<_>>(), vec![beat, beat, 2 * beat]);
        assert_eq!(r.length_ticks, TICKS_PER_BAR, "four beats round to one bar");
        assert!((r.bars_exact - 1.0).abs() < 1e-9);
        assert_eq!(r.bar(), 1);
    }

    /// A 90 BPM bar lasts 2.67 s, which in a 120 BPM song is a bar and a
    /// third — said, and the clip rounds up to whole bars so it loops on the
    /// bar line.
    #[test]
    fn a_break_at_another_tempo_is_measured_honestly() {
        let source = pcm(&[(0, RATE as usize * 3, 0.5)]);
        let bar_at_90 = (RATE * 60.0 / 90.0 * 4.0) as u64; // 128,000 frames
        let r = replay(&source, &[(0, bar_at_90)], c2(), 120.0, 0, &[]).unwrap();
        assert!((r.bars_exact - 4.0 / 3.0).abs() < 1e-3, "{}", r.bars_exact);
        assert_eq!(r.length_ticks, 2 * TICKS_PER_BAR);
    }

    #[test]
    fn a_quieter_slice_replays_softer_and_the_loudest_at_full() {
        let source = pcm(&[(0, 24_000, 0.8), (24_000, 48_000, 0.08), (48_000, 72_000, 0.0)]);
        let slices = [(0, 24_000), (24_000, 48_000), (48_000, 72_000)];
        let r = replay(&source, &slices, c2(), 120.0, 0, &[]).unwrap();
        assert_eq!(r.notes[0].velocity, 127);
        // 20 dB down: 127 - 58.
        assert!((i32::from(r.notes[1].velocity) - 69).abs() <= 1, "{}", r.notes[1].velocity);
        assert_eq!(r.notes[2].velocity, VELOCITY_FLOOR, "a silent slice vanished");
    }

    /// The clip never lands on one already there, and starts on a bar line
    /// at or after the playhead.
    #[test]
    fn it_lands_on_the_next_free_bar() {
        let source = pcm(&[(0, 24_000, 0.5)]);
        let taken = Clip::of_notes(0, TICKS_PER_BAR, Vec::new());
        let r = replay(&source, &[(0, 24_000)], c2(), 120.0, 10, &[taken]).unwrap();
        assert_eq!(r.start_tick, TICKS_PER_BAR);
        assert_eq!(r.bar(), 2);
    }

    #[test]
    fn nothing_to_replay_is_none() {
        assert!(replay(&pcm(&[]), &[], c2(), 120.0, 0, &[]).is_none());
    }

    #[test]
    fn the_clip_carries_the_notes() {
        let source = pcm(&[(0, 24_000, 0.5)]);
        let r = replay(&source, &[(0, 24_000)], c2(), 120.0, 0, &[]).unwrap();
        let clip = r.clip();
        assert_eq!((clip.start_tick, clip.length_ticks), (r.start_tick, r.length_ticks));
        assert_eq!(clip.notes, r.notes);
        assert!(clip.has_content);
    }
}
