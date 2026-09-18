//! Rendering a captured performance into a pad's audio — offline,
//! deterministic, and never on the audio thread.
//!
//! This is the audio-domain sibling of
//! [`render_clip_through_rack`](crate::state::render_clip_through_rack):
//! the same block-loop skeleton, with an instrument on the end of it and
//! samples coming out instead of notes. Where that one answers "what would
//! the rack print", this one answers "what did the player just play", and
//! the answer is a buffer.
//!
//! # Why it is here and not in the front end
//!
//! It needs a plugin factory, and the factory lives in
//! [`crate::instrument`] — moved down from the front end for exactly this,
//! because a render is application work with a plugin in the middle of it,
//! not drawing. Keeping it in this crate is also what lets the timing,
//! trim and root rules be tested without a terminal.
//!
//! # Determinism
//!
//! Every instance is built fresh, initialised at the engine's rate, reset,
//! and fed fixed-size blocks. Two renders of the same capture through the
//! same panel are the same samples — which is what makes "record it again
//! and compare" a thing a player can do, and what lets the tests assert
//! equality rather than approximate it.
//!
//! # What the player heard
//!
//! The captured stream runs through fresh copies of the track's MIDI
//! effects before it reaches the instrument, because that is the signal
//! path the performance went down while it was being played. A take from a
//! track with a chord device on it contains the chords.

use std::sync::Arc;

use phosphor_core::fx::db_to_gain;
use phosphor_plugin::sample::{PhraseEvent, SamplePcm};
use phosphor_plugin::MidiEvent;

use crate::state::{InstrumentType, MidiFxInstance};

use super::capture::{RenderEvent, Tail, TakePlan};

/// The block every offline render is cut into. The same 256 the MIDI
/// render uses: small enough that an event lands within a millisecond of
/// its stamp, large enough that a minute of audio is not a million calls.
const BLOCK: usize = 256;

/// Where a take's leading silence is judged from. Anything under this is
/// the room, not the note.
const LEAD_FLOOR_DB: f32 = -48.0;

/// How far back off the first real sample the start is pulled. An attack
/// is a transient with a foot on it, and cutting on the exact threshold
/// crossing shaves the foot off — which is audible as a click and as a
/// weaker hit.
const LEAD_BACKOFF_MS: f32 = 8.0;

/// Where a take's tail is judged from — quieter than the lead, because a
/// release fading into nothing is part of the sound.
const TAIL_FLOOR_DB: f32 = -60.0;

/// How much room the tail keeps past the last audible sample.
const TAIL_PAD_MS: f32 = 20.0;

/// How quiet the instrument has to go before a free take's tail is judged
/// finished, and for how long.
const SILENCE_DB: f32 = -90.0;
const SILENCE_MS: f32 = 50.0;

/// A rendered take: the audio, where it should be trimmed to, how loud it
/// got, and the root the performance taught.
#[derive(Debug, Clone)]
pub struct RenderedTake {
    pub pcm: Arc<SamplePcm>,
    /// Non-destructive trim: the buffer is whole and these say what plays.
    pub start_frame: u64,
    pub end_frame: u64,
    /// Peak of the whole buffer, linear.
    pub peak: f32,
    /// The pitch every note-on shared, when they shared one.
    pub root: Option<u8>,
}

impl RenderedTake {
    /// How long the trimmed region lasts.
    #[must_use]
    pub fn seconds(&self) -> f32 {
        (self.end_frame - self.start_frame) as f32 / self.pcm.sample_rate.max(1.0)
    }

    /// The peak in decibels, or `None` for a take with no signal in it at
    /// all.
    #[must_use]
    pub fn peak_db(&self) -> Option<f32> {
        (self.peak > 0.0).then(|| 20.0 * self.peak.log10())
    }
}

/// Render `plan` through `rack` and then `instrument`, at the engine's
/// rate.
///
/// `params` is the instrument's whole panel, in its own order — the same
/// block a track carries, replayed into the fresh instance so the take
/// sounds like the thing that was under the player's hands.
#[must_use]
pub fn render_take(
    plan: &TakePlan,
    rack: &[MidiFxInstance],
    instrument: InstrumentType,
    params: &[f32],
) -> RenderedTake {
    let events = through_midi_rack(plan, rack);
    let total = total_frames(plan);
    let pcm = through_instrument(&events, plan, total, instrument, params);
    let peak = peak_of(&pcm.data);
    let (start_frame, end_frame) = match plan.tail {
        // A bar take keeps its exact window: its start is a bar line and
        // its end is the loop point, and "helpfully" moving either would
        // break the one thing the shape is for.
        Tail::Cut => (0, pcm.frames()),
        Tail::ToSilence { .. } => auto_trim(&pcm),
    };
    RenderedTake {
        pcm: Arc::new(pcm),
        start_frame,
        end_frame,
        peak,
        root: single_pitch(&plan.events),
    }
}

/// A captured performance kept as notes: what a phrase layer is made of.
#[derive(Debug, Clone)]
pub struct RenderedPhrase {
    /// The performance, chain-baked, in time order.
    pub events: Arc<[PhraseEvent]>,
    /// How long the pad plays before the phrase is over, in engine frames.
    pub frames: u64,
    /// The engine rate those frames were counted at — what lets the runner
    /// play them back at the speed they were played, on any device.
    pub sample_rate: f32,
    /// The pitch every note-on shared, when they shared one.
    pub root: Option<u8>,
}

impl RenderedPhrase {
    /// How long it plays, in seconds at the rate it was captured at.
    #[must_use]
    pub fn seconds(&self, rate: f32) -> f32 {
        self.frames as f32 / rate.max(1.0)
    }

    /// Notes in it — what tells a played phrase from an empty window.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.events.iter().filter(|e| e.status & 0xF0 == 0x90 && e.data2 > 0).count()
    }
}

/// Keep `plan` as notes rather than rendering it to audio.
///
/// The audio path's first half, stopped one step early: the capture goes
/// through fresh copies of the track's MIDI effects and then *is* the take,
/// because that is the signal path the performance went down while it was
/// being played. A phrase recorded on a track with an arpeggiator on it
/// holds the arpeggio — the same promise
/// [`render_take`] makes, kept by the same code.
///
/// No tail: a phrase is note data, and the notes a device was still holding
/// were already closed at the window by the rack. What plays the release is
/// the child instrument, live.
#[must_use]
pub fn render_phrase(plan: &TakePlan, rack: &[MidiFxInstance]) -> RenderedPhrase {
    let events = through_midi_rack(plan, rack)
        .into_iter()
        .map(|e| PhraseEvent {
            frame: e.frame,
            status: e.status,
            data1: e.data1,
            data2: e.data2,
        })
        .collect();
    RenderedPhrase {
        events,
        frames: plan.frames.max(1),
        // Stamped here, where the rate the frames were counted at is a fact
        // rather than a guess: the plan was built against this engine.
        sample_rate: plan.sample_rate,
        // Read from the plan rather than from the baked stream, for the
        // reason [`single_pitch`] gives: a chord device turning one key
        // into four does not make the performance a chord.
        root: single_pitch(&plan.events),
    }
}

/// The pitch every note-on in the take shares, when they share one.
///
/// Read from the plan rather than from the raw arrival stream: the plan is
/// the performance with the window applied, so a note played before a bar
/// take opened does not get a vote on the root. Read before the rack,
/// because a chord device turning one key into four does not make the
/// performance a chord — the player played one note.
fn single_pitch(events: &[RenderEvent]) -> Option<u8> {
    let mut pitch = None;
    for event in events.iter().filter(|e| e.status == 0x90 && e.data2 > 0) {
        match pitch {
            None => pitch = Some(event.data1),
            Some(first) if first == event.data1 => {}
            Some(_) => return None,
        }
    }
    pitch
}

/// Frames the finished buffer holds: the window, plus whatever the tail
/// policy allows, and never more than the WAV loader would take back.
fn total_frames(plan: &TakePlan) -> u64 {
    let tail = match plan.tail {
        Tail::Cut => 0,
        Tail::ToSilence { max_frames } => max_frames,
    };
    plan.frames.saturating_add(tail).min(u64::from(super::wav::MAX_FRAMES))
}

/// The capture, through fresh copies of the track's MIDI effects.
///
/// An empty or entirely bypassed rack is the identity — the same rule the
/// clip render follows, and the reason a track with no devices costs
/// nothing here.
fn through_midi_rack(plan: &TakePlan, rack: &[MidiFxInstance]) -> Vec<RenderEvent> {
    use phosphor_core::midi_fx::MidiFxContext;

    let mut chain = crate::state::build_offline_rack(rack, plan.sample_rate, BLOCK);
    if chain.is_empty() {
        return plan.events.clone();
    }

    let ticks_per_sample = (plan.tempo_bpm * phosphor_core::transport::Transport::PPQ as f64)
        / (60.0 * f64::from(plan.sample_rate));
    let blocks = plan.frames.div_ceil(BLOCK as u64);
    let mut out: Vec<RenderEvent> = Vec::with_capacity(plan.events.len() * 2);
    let mut feed: Vec<MidiEvent> = Vec::with_capacity(BLOCK);
    let mut buf_a: Vec<MidiEvent> = Vec::with_capacity(1024);
    let mut buf_b: Vec<MidiEvent> = Vec::with_capacity(1024);
    let mut cursor = 0usize;

    for block in 0..blocks {
        let start = block * BLOCK as u64;
        let end = start + BLOCK as u64;
        feed.clear();
        while cursor < plan.events.len() && plan.events[cursor].frame < end {
            let event = plan.events[cursor];
            feed.push(MidiEvent {
                sample_offset: (event.frame - start) as u32,
                status: event.status,
                data1: event.data1,
                data2: event.data2,
            });
            cursor += 1;
        }
        let ctx = MidiFxContext {
            sample_rate: plan.sample_rate,
            tempo_bpm: plan.tempo_bpm,
            // A free take was played with the transport stopped, so the
            // devices free-run on their own sample clock exactly as they
            // did while the player was listening. A bar take was played
            // against the transport, so they lock to its grid.
            playing: plan.start_tick.is_some(),
            num_frames: BLOCK as u32,
            block_start_tick: plan.start_tick.unwrap_or(0)
                + (start as f64 * ticks_per_sample) as i64,
            ticks_per_sample,
        };
        buf_a.clear();
        buf_a.extend_from_slice(&feed);
        for fx in &mut chain {
            buf_b.clear();
            fx.process(&buf_a, &mut buf_b, &ctx);
            std::mem::swap(&mut buf_a, &mut buf_b);
        }
        for event in &buf_a {
            if !matches!(event.status & 0xF0, 0x90 | 0x80) {
                continue;
            }
            out.push(RenderEvent {
                frame: start + u64::from(event.sample_offset),
                status: event.status,
                data1: event.data1,
                data2: event.data2,
            });
        }
    }
    // Anything the chain is still sounding closes at the window, or it
    // hangs into a tail nobody played.
    buf_a.clear();
    for fx in &mut chain {
        fx.flush(&mut buf_a);
    }
    let last = plan.frames.saturating_sub(1);
    for event in &buf_a {
        if event.status & 0xF0 == 0x80 {
            out.push(RenderEvent {
                frame: last,
                status: event.status,
                data1: event.data1,
                data2: event.data2,
            });
        }
    }
    out.sort_by_key(|e| (e.frame, e.status));
    out
}

/// The event stream, through a fresh instrument, into stereo.
fn through_instrument(
    events: &[RenderEvent],
    plan: &TakePlan,
    total: u64,
    instrument: InstrumentType,
    params: &[f32],
) -> SamplePcm {
    let mut plugin = crate::instrument::build_plugin(instrument);
    plugin.init(f64::from(plan.sample_rate), BLOCK);
    for (index, &value) in params.iter().enumerate() {
        plugin.set_parameter(index, value);
    }
    plugin.reset();

    let total = total as usize;
    let mut data = Vec::with_capacity(total * 2);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut feed: Vec<MidiEvent> = Vec::with_capacity(64);
    let mut cursor = 0usize;
    let performance = plan.frames as usize;
    // How long the instrument has been under the silence floor, in frames.
    let mut quiet = 0usize;
    let silent_for = (SILENCE_MS / 1000.0 * plan.sample_rate) as usize;
    let floor = db_to_gain(SILENCE_DB);

    let mut frame = 0usize;
    while frame < total {
        let block = BLOCK.min(total - frame);
        feed.clear();
        while cursor < events.len() && (events[cursor].frame as usize) < frame + block {
            let event = events[cursor];
            feed.push(MidiEvent {
                sample_offset: (event.frame as usize - frame) as u32,
                status: event.status,
                data1: event.data1,
                data2: event.data2,
            });
            cursor += 1;
        }
        {
            // The mixer hands its instruments buffers it does not clear,
            // because an instrument writes its output rather than adding
            // to it. Doing the same here keeps the offline path and the
            // live one the same path.
            let (l, r) = (&mut left[..block], &mut right[..block]);
            let mut outs: [&mut [f32]; 2] = [l, r];
            plugin.process(&[], &mut outs, &feed);
        }
        for i in 0..block {
            data.push(left[i]);
            data.push(right[i]);
        }
        frame += block;

        // Past the performance, a free take runs until the instrument has
        // gone quiet — a release, a delay, a reverb tail — and no further.
        if frame >= performance && matches!(plan.tail, Tail::ToSilence { .. }) {
            let loud = left[..block]
                .iter()
                .chain(right[..block].iter())
                .fold(0.0f32, |m, s| m.max(s.abs()));
            if loud < floor {
                quiet += block;
                if quiet >= silent_for {
                    break;
                }
            } else {
                quiet = 0;
            }
        }
    }

    SamplePcm { data, channels: 2, sample_rate: plan.sample_rate }
}

/// Where the take actually begins and ends.
///
/// Non-destructive: the buffer keeps every sample and these are the marks
/// the trim strip opens on, so a backed-off attack can always be recovered
/// by moving the marker.
fn auto_trim(pcm: &SamplePcm) -> (u64, u64) {
    let frames = pcm.frames();
    let channels = usize::from(pcm.channels.max(1));
    let rate = pcm.sample_rate.max(1.0);
    let lead = db_to_gain(LEAD_FLOOR_DB);
    let tail = db_to_gain(TAIL_FLOOR_DB);

    let loud_at = |frame: u64| -> f32 {
        let base = frame as usize * channels;
        pcm.data[base..base + channels].iter().fold(0.0f32, |m, s| m.max(s.abs()))
    };

    let first = (0..frames).find(|&f| loud_at(f) >= lead);
    let Some(first) = first else { return (0, frames) };
    let last = (0..frames).rev().find(|&f| loud_at(f) >= tail).unwrap_or(frames - 1);

    let backoff = (LEAD_BACKOFF_MS / 1000.0 * rate) as u64;
    let pad = (TAIL_PAD_MS / 1000.0 * rate) as u64;
    let start = first.saturating_sub(backoff);
    let end = (last + 1 + pad).min(frames);
    (start, end.max(start + 1))
}

/// The loudest sample in a buffer, linear.
#[must_use]
pub fn peak_of(data: &[f32]) -> f32 {
    data.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::capture::{CapturedEvent, TakeCapture};
    use crate::state::MidiFxType;

    const SR: f32 = 44_100.0;

    fn plan_of(events: &[(u64, u8, bool)], disarm: u64) -> TakePlan {
        let mut capture = TakeCapture::free(120.0, SR);
        for &(micros, note, on) in events {
            capture.note(CapturedEvent {
                micros,
                note,
                velocity: if on { 100 } else { 0 },
                on,
            });
        }
        capture.close(disarm).expect("a played take")
    }

    /// The same performance through the same instrument twice is the same
    /// audio — sample for sample, not nearly.
    #[test]
    fn a_render_is_the_same_twice() {
        let plan = plan_of(&[(0, 60, true), (200_000, 60, false)], 400_000);
        let params = crate::preset::defaults(InstrumentType::Synth);
        let a = render_take(&plan, &[], InstrumentType::Synth, &params);
        let b = render_take(&plan, &[], InstrumentType::Synth, &params);
        assert_eq!(a.pcm.data, b.pcm.data, "two renders of one take differed");
        assert!(a.peak > 0.0, "the render made no sound at all");
    }

    /// The panel is the render's, not the instrument's defaults: a take made
    /// after the player shut the filter is a darker take.
    ///
    /// The seam this pins is the one the `[inst]` panel edits through in
    /// source mode — the mode's own copy of the panel, handed to this
    /// function when the take lands.
    #[test]
    fn the_panel_handed_in_is_the_panel_the_take_is_rendered_through() {
        let plan = plan_of(&[(0, 60, true), (200_000, 60, false)], 400_000);
        let open = crate::preset::defaults(InstrumentType::Synth);
        let mut shut = open.clone();
        shut[phosphor_dsp::synth::P_CUTOFF] = 0.0;

        let a = render_take(&plan, &[], InstrumentType::Synth, &open);
        let b = render_take(&plan, &[], InstrumentType::Synth, &shut);
        assert_ne!(a.pcm.data, b.pcm.data, "the panel never reached the instrument");
        assert!(b.peak < a.peak, "shutting the filter did not make the take darker");
    }

    /// The rack runs. One key through a chord device is a chord, and a
    /// chord is not the same audio as one note.
    #[test]
    fn the_midi_rack_runs_before_the_instrument() {
        let plan = plan_of(&[(0, 48, true), (500_000, 48, false)], 700_000);
        let params = crate::preset::defaults(InstrumentType::Synth);
        let bare = render_take(&plan, &[], InstrumentType::Synth, &params);
        let rack = vec![MidiFxInstance::new(MidiFxType::Chord)];
        let chorded = render_take(&plan, &rack, InstrumentType::Synth, &params);
        assert_ne!(bare.pcm.data, chorded.pcm.data, "the chord device never ran");

        // ...and a bypassed device is an absent one.
        let mut off = MidiFxInstance::new(MidiFxType::Chord);
        off.bypass = true;
        let bypassed = render_take(&plan, &[off], InstrumentType::Synth, &params);
        assert_eq!(bare.pcm.data, bypassed.pcm.data, "a bypassed device changed the take");
    }

    /// A note stamped half a second into a free take sounds half a second
    /// into the buffer.
    ///
    /// Found by rendering the take twice — once without the second note —
    /// and asking where the two buffers first disagree. That frame is the
    /// second note's first sample by construction, which beats hunting for
    /// an onset inside the first note's decay.
    #[test]
    fn a_note_lands_where_its_stamp_says() {
        let params = crate::preset::defaults(InstrumentType::Synth);
        let first_only = plan_of(&[(0, 36, true), (100_000, 36, false)], 900_000);
        let both = plan_of(
            &[(0, 36, true), (100_000, 36, false), (500_000, 72, true), (600_000, 72, false)],
            900_000,
        );
        let a = render_take(&first_only, &[], InstrumentType::Synth, &params);
        let b = render_take(&both, &[], InstrumentType::Synth, &params);
        let diverged = a
            .pcm
            .data
            .iter()
            .zip(b.pcm.data.iter())
            .position(|(x, y)| x != y)
            .expect("the second note never sounded")
            / 2;
        let expected = (0.5 * SR) as usize;
        assert!(
            diverged.abs_diff(expected) < (0.005 * SR) as usize,
            "the note landed at frame {diverged}, not near {expected}",
        );
    }

    /// A take that opens on silence is trimmed back to just before its
    /// first sound — and never into it.
    ///
    /// The plan is built by hand: a free take's own zero is its first
    /// note-on, so leading silence only exists when the instrument takes
    /// its time, and this is the rule rather than one instrument's attack.
    #[test]
    fn leading_silence_is_trimmed_back_to_the_attack() {
        let quiet = (0.3 * SR) as u64;
        let plan = TakePlan {
            events: vec![
                RenderEvent { frame: quiet, status: 0x90, data1: 64, data2: 110 },
                RenderEvent { frame: quiet + 10_000, status: 0x80, data1: 64, data2: 0 },
            ],
            frames: quiet + 20_000,
            tail: Tail::ToSilence { max_frames: (2.0 * SR) as u64 },
            start_tick: None,
            tempo_bpm: 120.0,
            sample_rate: SR,
            capped: false,
        };
        let params = crate::preset::defaults(InstrumentType::Synth);
        let take = render_take(&plan, &[], InstrumentType::Synth, &params);
        let floor = db_to_gain(LEAD_FLOOR_DB);
        let first = (0..take.pcm.frames())
            .find(|&f| {
                let i = f as usize * 2;
                take.pcm.data[i].abs().max(take.pcm.data[i + 1].abs()) >= floor
            })
            .expect("nothing audible in the take");
        assert!(first > 0, "this test needs a take that opens on silence");
        assert!(take.start_frame <= first, "the trim cut into the attack");
        let ms = (first - take.start_frame) as f32 / SR * 1000.0;
        assert!((7.0..=10.0).contains(&ms), "the start backed off {ms:.1} ms");
        assert!(take.end_frame <= take.pcm.frames());
        assert!(take.end_frame > take.start_frame);
    }

    /// A bar take keeps its window exactly: the loop point is the whole
    /// point, and a trim would move it.
    #[test]
    fn a_bar_take_is_not_auto_trimmed() {
        let mut capture = TakeCapture::bars(0, 0, 120.0, SR);
        capture.note(CapturedEvent { micros: 300_000, note: 60, velocity: 100, on: true });
        capture.note(CapturedEvent { micros: 600_000, note: 60, velocity: 0, on: false });
        let plan = capture.close(1_900_000).unwrap();
        let params = crate::preset::defaults(InstrumentType::Synth);
        let take = render_take(&plan, &[], InstrumentType::Synth, &params);
        assert_eq!(take.start_frame, 0, "a bar take was trimmed at the front");
        assert_eq!(take.end_frame, take.pcm.frames());
        // One bar of 4/4 at 120 BPM: two seconds, to the sample.
        assert_eq!(take.pcm.frames(), (2.0 * SR) as u64);
    }

    /// The root is learned from a one-finger performance and from nothing
    /// else.
    #[test]
    fn a_one_pitch_performance_teaches_a_root() {
        let params = crate::preset::defaults(InstrumentType::Synth);
        let one = plan_of(&[(0, 41, true), (100_000, 41, false), (200_000, 41, true), (300_000, 41, false)], 500_000);
        assert_eq!(render_take(&one, &[], InstrumentType::Synth, &params).root, Some(41));
        let chord = plan_of(&[(0, 41, true), (1_000, 45, true), (100_000, 41, false), (100_100, 45, false)], 300_000);
        assert_eq!(render_take(&chord, &[], InstrumentType::Synth, &params).root, None);
    }

    /// A free take stops when the instrument does, well short of the two
    /// second cap, and a runaway take never runs past what the loader
    /// would take back.
    #[test]
    fn the_tail_ends_at_silence_and_the_buffer_stays_loadable() {
        let plan = plan_of(&[(0, 60, true), (50_000, 60, false)], 100_000);
        let params = crate::preset::defaults(InstrumentType::Synth);
        let take = render_take(&plan, &[], InstrumentType::Synth, &params);
        let seconds = take.pcm.frames() as f32 / SR;
        assert!(seconds < 2.2, "the tail ran to {seconds:.2}s");
        assert!(seconds > 0.1, "the take is shorter than the performance");
        assert!(take.pcm.frames() <= u64::from(super::super::wav::MAX_FRAMES));
    }

    // ── Phrases ──

    /// The same performance kept as a phrase twice is the same events —
    /// event for event, not nearly, which is what lets a player record the
    /// same bar twice and compare.
    #[test]
    fn a_phrase_is_the_same_twice() {
        let plan = plan_of(&[(0, 60, true), (200_000, 60, false)], 400_000);
        let a = render_phrase(&plan, &[]);
        let b = render_phrase(&plan, &[]);
        assert_eq!(a.events.as_ref(), b.events.as_ref(), "two phrases of one take differed");
        assert_eq!(a.frames, b.frames);
        assert_eq!(a.note_count(), 1);
        // And the events are the performance itself, at its own offsets.
        assert_eq!(a.events[0], PhraseEvent { frame: 0, status: 0x90, data1: 60, data2: 100 });
        assert_eq!(a.events.last().unwrap().status, 0x80, "the key was never lifted");
        assert!(a.seconds(SR) > 0.0);
    }

    /// The rack runs, exactly as it does for audio: one key through a chord
    /// device is a chord, so the phrase holds three notes and not one.
    #[test]
    fn the_midi_rack_is_baked_into_a_phrase() {
        let plan = plan_of(&[(0, 48, true), (500_000, 48, false)], 700_000);
        let bare = render_phrase(&plan, &[]);
        assert_eq!(bare.note_count(), 1);

        let chorded = render_phrase(&plan, &[MidiFxInstance::new(MidiFxType::Chord)]);
        assert!(
            chorded.note_count() > bare.note_count(),
            "the chord device never ran: {} notes",
            chorded.note_count(),
        );
        let pitches: Vec<u8> =
            chorded.events.iter().filter(|e| e.status == 0x90).map(|e| e.data1).collect();
        assert!(pitches.contains(&48), "the key played is not in the chord");
        assert!(pitches.iter().any(|&p| p != 48), "every note of the chord is the root");

        // A bypassed device is an absent one, the audio path's rule.
        let mut off = MidiFxInstance::new(MidiFxType::Chord);
        off.bypass = true;
        assert_eq!(
            render_phrase(&plan, &[off]).events.as_ref(),
            bare.events.as_ref(),
            "a bypassed device changed the phrase",
        );
    }

    /// A phrase's events are in time order, whatever the rack did to them —
    /// the engine's runner walks them forwards and never sorts.
    #[test]
    fn a_phrases_events_arrive_in_time_order() {
        let plan = plan_of(
            &[(0, 36, true), (100_000, 48, true), (150_000, 36, false), (400_000, 48, false)],
            600_000,
        );
        for rack in [Vec::new(), vec![MidiFxInstance::new(MidiFxType::Arp)]] {
            let phrase = render_phrase(&plan, &rack);
            assert!(
                phrase.events.windows(2).all(|w| w[0].frame <= w[1].frame),
                "the events came back out of order",
            );
            assert!(
                phrase.events.iter().all(|e| e.frame < phrase.frames),
                "an event landed past the end of its own phrase",
            );
        }
    }

    /// One key teaches a root and a chord does not, the take's rule — and
    /// it matters more here, because a phrase's only transposition is
    /// measured from it.
    #[test]
    fn a_one_finger_phrase_teaches_a_root() {
        let one = plan_of(&[(0, 41, true), (100_000, 41, false)], 300_000);
        assert_eq!(render_phrase(&one, &[]).root, Some(41));
        let chord =
            plan_of(&[(0, 41, true), (1_000, 45, true), (100_000, 41, false)], 300_000);
        assert_eq!(render_phrase(&chord, &[]).root, None);
        // An arpeggiator turning one key into a run does not make it a run.
        assert_eq!(
            render_phrase(&one, &[MidiFxInstance::new(MidiFxType::Arp)]).root,
            Some(41),
            "the rack got a vote on the root",
        );
    }

    /// A bar-quantised phrase is the window, to the frame: the loop point
    /// is the point, and a phrase one frame short would drift every pass.
    #[test]
    fn a_bar_phrase_is_exactly_its_window() {
        let mut capture = TakeCapture::bars(0, 0, 120.0, SR);
        capture.note(CapturedEvent { micros: 300_000, note: 60, velocity: 100, on: true });
        capture.note(CapturedEvent { micros: 600_000, note: 60, velocity: 0, on: false });
        let plan = capture.close(1_900_000).unwrap();
        let phrase = render_phrase(&plan, &[]);
        // One bar of 4/4 at 120 BPM, at 44.1 kHz: two seconds.
        assert_eq!(phrase.frames, (2.0 * SR) as u64);
        assert!((phrase.seconds(SR) - 2.0).abs() < 1e-6);
    }

    /// The render is stereo at the engine's rate, whatever the instrument
    /// is — a take that claimed the wrong rate would play back sharp.
    #[test]
    fn the_buffer_is_stereo_at_the_engines_rate() {
        let mut plan = plan_of(&[(0, 60, true), (100_000, 60, false)], 200_000);
        plan.sample_rate = 48_000.0;
        let params = crate::preset::defaults(InstrumentType::Rhodes);
        let take = render_take(&plan, &[], InstrumentType::Rhodes, &params);
        assert_eq!(take.pcm.channels, 2);
        assert_eq!(take.pcm.sample_rate, 48_000.0);
        assert_eq!(take.pcm.data.len() as u64, take.pcm.frames() * 2);
    }
}

