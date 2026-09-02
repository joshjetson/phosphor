//! The MIDI-effect layer: what transforms notes before an instrument hears
//! them.
//!
//! Nothing in here makes a sound and nothing in here touches audio. A MIDI
//! effect is handed the track's assembled event list for one block — live
//! playing, pattern steps and clip playback alike — and writes a new list.
//! A chord device turns one key into five; an arpeggiator turns a held
//! chord into a run of short notes. The instrument renders whatever comes
//! out, and the recorder taps the stream *upstream* of this layer, so what
//! lands in a clip is always what the player actually played. Change the
//! device tomorrow and yesterday's take plays through the new sound — the
//! whole reason this is a playback transform and not a capture one.
//!
//! The rules match the audio insert layer's, because they are the same
//! rules: nothing allocates while audio is running (output buffers are
//! pre-sized and never grow), parameters live in natural units, and a
//! bypassed slot passes events through untouched by control flow, not by
//! transforming them into themselves.

use phosphor_plugin::MidiEvent;

use crate::fx::FxParamInfo;

mod arp;
mod chord;

pub use arp::{Arpeggiator, ARP_PARAMS, RATE_LABELS, STYLE_LABELS};
pub use chord::{ChordDevice, UserChord, CHORD_PARAMS, LEARNED_QUALITY, COLOR_LABELS, MAX_USER_CHORDS, MODE_LABELS, NOTE_NAMES, PROG_LABELS, QUALITIES, SCALE_LABELS, VOICING_LABELS};

/// How many MIDI effects one track holds. Two is the canonical chain —
/// a chord device feeding an arpeggiator — and what fits on the panel.
pub const MAX_MIDI_FX_SLOTS: usize = 2;

/// How many events the layer's scratch buffer holds. An arpeggiator over a
/// ten-finger chord at a fast rate multiplies the input; this is far past
/// anything musical, and the chain truncates honestly rather than growing
/// on the audio thread.
pub const MIDI_FX_EVENT_CAPACITY: usize = 1024;

/// What a MIDI effect is told about the block it is about to transform.
#[derive(Clone, Copy)]
pub struct MidiFxContext {
    /// Samples per second.
    pub sample_rate: f32,
    /// The transport's tempo, read once for this block.
    pub tempo_bpm: f64,
    /// Whether the transport is rolling.
    pub playing: bool,
    /// Frames in this block.
    pub num_frames: u32,
    /// The tick the block starts on when the transport is rolling. An
    /// arpeggiator locks its grid to this; stopped, it free-runs on its own
    /// sample clock and this is meaningless.
    pub block_start_tick: i64,
    /// Ticks per sample at this block's tempo.
    pub ticks_per_sample: f64,
}

impl MidiFxContext {
    /// A stopped-transport context, for tests and offline rendering.
    #[must_use]
    pub fn bare(sample_rate: f32, num_frames: u32) -> Self {
        Self {
            sample_rate,
            tempo_bpm: 120.0,
            playing: false,
            num_frames,
            block_start_tick: 0,
            ticks_per_sample: 0.0,
        }
    }
}

/// A MIDI effect in a pre-instrument slot.
///
/// Deliberately not [`crate::fx::Effect`]: an audio effect rewrites sample
/// buffers in place and has no notion of a note; this one reads an event
/// list and writes another, and its whole job is notes. Sharing a trait
/// would give every equalizer a note-off book it does not keep.
///
/// **Real-time contract.** `process`, `reset` and `flush` run on the audio
/// thread: no allocation, no locks, no logging. `out` arrives with
/// capacity; push past it and the push is the allocation this layer
/// forbids, so effects check [`Vec::capacity`] and drop late events instead.
pub trait MidiEffect: Send {
    /// The stable name this effect is stored under in a session file.
    fn name(&self) -> &'static str;

    /// Build everything. Called once, off the audio thread, before the
    /// effect reaches a slot.
    fn init(&mut self, sample_rate: f64, max_block: usize);

    /// Transform one block. `input` is the track's assembled, sorted event
    /// list; the effect pushes its whole output — including anything it
    /// passes through unchanged — into `out`, which arrives empty.
    fn process(&mut self, input: &[MidiEvent], out: &mut Vec<MidiEvent>, ctx: &MidiFxContext);

    /// Note-offs for everything this effect is currently sounding, pushed
    /// into `out`. Called when the slot is bypassed or removed, so a
    /// generated chord does not hang under an instrument that never gets
    /// the off. Real-time.
    fn flush(&mut self, out: &mut Vec<MidiEvent>);

    /// Drop all state silently: held keys, latch, pattern position. The
    /// panic path — the instruments are being silenced anyway, so no offs
    /// are owed. Real-time.
    fn reset(&mut self);

    fn parameter_count(&self) -> usize;

    /// What parameter `index` is, or `None` if there is no such parameter.
    fn parameter_info(&self, index: usize) -> Option<FxParamInfo>;

    /// The current value of a parameter, in its natural unit.
    fn get_parameter(&self, index: usize) -> f32;

    /// Set a parameter, in its natural unit. Out-of-range values are the
    /// effect's to clamp; an unknown index is ignored.
    fn set_parameter(&mut self, index: usize, value: f32);

    /// Hand the effect a user progression, for effects that hold one. A
    /// default no-op, because most effects have no notion of a chord list.
    /// Real-time: the effect copies into storage it already owns.
    fn set_progression(&mut self, _chords: &[chord::UserChord]) {}
}

/// One slot: an effect and its bypass.
pub struct MidiFxSlot {
    pub fx: Box<dyn MidiEffect>,
    pub bypassed: bool,
}

/// Build a MIDI effect by its stable session name.
#[must_use]
pub fn build_midi_fx(name: &str) -> Option<Box<dyn MidiEffect>> {
    match name {
        "arp" => Some(Box::new(Arpeggiator::new())),
        "chord" => Some(Box::new(ChordDevice::new())),
        _ => None,
    }
}
