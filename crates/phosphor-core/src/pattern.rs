//! Step-sequencer patterns: what the audio thread plays, and the arithmetic
//! that turns it into notes.
//!
//! A pattern is a grid — eight lanes of up to thirty-two steps — that
//! generates MIDI for the instrument on its own track. It is not an
//! instrument: it makes no sound, it makes note events, and the child
//! instrument in the track's plugin slot turns those into audio. TR and
//! Elektron lineage, with the DAW transport as master.
//!
//! # Why the shapes here are what they are
//!
//! **Fixed size and `Copy`.** A pattern crosses to the audio thread whole, as
//! a value inside a [`crate::mixer::MixerCommand`]. No `Vec`, no `Box`, no
//! `Arc`: receiving one is a move into memory that already exists, and the
//! audio thread never reaches the allocator to accept an edit. That costs
//! [`PatternBlock::SIZE`] bytes per queued command, which is the price of
//! never taking a lock or an allocation on the deadline side.
//!
//! **Position-derived, never free-running.** The step under the playhead is
//! `(position / ticks_per_step) mod steps` — a function of the transport's
//! tick position and nothing else. There is no cursor that advances one step
//! per callback, because a cursor drifts: starting playback in bar 5 would
//! sound different from starting in bar 1 and waiting, and that is exactly
//! the invariant clips already hold. Everything else follows from it —
//! starting mid-pattern fires only the onsets that remain, and a loop wrap
//! neither drops nor doubles the first step.
//!
//! **One window, shared with clips.** [`PlaybackWindow`] is the span of song
//! time one callback renders, and both clip playback and pattern playback in
//! `mixer.rs` take their events from the same value. That is what makes "a
//! pattern step and a clip note on the same beat land on the same sample"
//! structural rather than a coincidence two code paths have to keep agreeing
//! on. It lives in this module because the sync guarantee is the reason this
//! module exists at all.
//!
//! # Notes have to end
//!
//! Every note this module starts is written into a [`PendingOffs`] table with
//! the tick its note-off is due at, and the table is drained in tick order as
//! the windows go by. A tied step (`gate` = [`Step::TIE`]) has no due tick at
//! all — it is ended by the lane's next onset, which is the 303 slide feel —
//! and the table is flushed whole at every discontinuity: stop, pause, a
//! position jump, a loop wrap, a pattern switch, a panic. The table holds
//! thirty-two notes and an overflow forces off the *oldest* rather than
//! dropping the new one, because a note that is never turned off is a stuck
//! voice and this project has already shipped one fix for that class of bug.
//!
//! # What is pure and what is not
//!
//! Everything except [`PatternPlayer`] is a pure function of its arguments,
//! and the player's state is four scalars and the pending table. There is no
//! mixer, no sample rate and no plugin anywhere in this file: events come out
//! stamped with the absolute tick they happen at, and turning a tick into a
//! sample offset is [`PlaybackWindow::sample_offset`]'s single job. That is
//! what lets the bounce in `phosphor-app` compile a pattern to a clip through
//! *the same generator* the audio thread runs, which is the only way "the
//! bounce sounds identical to the live pattern" can be a fact rather than a
//! hope.

// ── Sizes ──

/// Lanes in a pattern.
///
/// Eight from day one, and not because eight drum voices is a nice round
/// number: one note per step cannot sequence drums at all. A kick and a
/// closed hat land on the same step in essentially every pattern ever
/// written, so a single-note-per-step grid is not a simpler sequencer, it is
/// a sequencer that cannot play a beat. The *view* may show one lane at a
/// time; the data holds eight.
pub const LANES: usize = 8;

/// The longest a pattern can be. Shorter patterns mask the tail rather than
/// clearing it — see [`PatternBlock::step_count`].
pub const MAX_STEPS: usize = 32;

/// Pattern slots per sequencer track.
pub const SLOTS: usize = 8;

/// Entries in a pattern chain. Each carries a repeat count, so "A×4 B×4 A×3
/// C" is four entries rather than twelve.
pub const MAX_CHAIN: usize = 16;

/// How many sounding notes one track can be holding at once.
///
/// Eight lanes of six-note chords is 48, which this does not cover — and
/// deliberately. The table is a safety net for notes whose offs are still in
/// the future, not a voice allocator; the child instrument has its own. When
/// it overflows, the oldest note is forced off and its slot reused, which is
/// the one behaviour that cannot leave a note sounding forever.
pub const MAX_PENDING_OFFS: usize = 32;

/// The step counts a pattern may be set to.
///
/// 12 and 24 are in the list for triplet and 3/4 feel, and for deliberate
/// polymeter against a 16-step lane — a 12-step pattern against a 4/4 bar
/// walks its accent one beat every bar and comes home after three.
pub const STEP_COUNTS: [u8; 6] = [4, 8, 12, 16, 24, 32];

/// The most global step indices one window will be scanned for.
///
/// A bound on the work rather than a limit anything can reach: at the
/// coarsest rate and the largest block a device may hand us, a window spans
/// two or three steps. Sixty-four is roughly two seconds of audio at 120 BPM,
/// which is two orders of magnitude past any real callback.
const MAX_STEP_SCAN: i64 = 64;

/// The most slot changes one window is allowed to contain.
///
/// A window spanning more than one chain entry means the entries are shorter
/// than a callback, which no musical setting produces.
const MAX_SEGMENTS: usize = 4;

// ── Rate ──

/// How long one step lasts, as a musical division.
///
/// An enum rather than the raw tick count so that a rate cannot be a number
/// nothing plays at, and rather than a `u8` index so that a session or a UI
/// cannot hand the audio thread a rate that does not exist. Ticks per step at
/// 960 PPQ are exact for every entry, triplets included — 960 divides by 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rate {
    Quarter,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
    EighthTriplet,
    SixteenthTriplet,
}

impl Rate {
    /// Every rate, in the order the UI steps through them.
    pub const ALL: [Rate; 6] = [
        Self::Quarter,
        Self::Eighth,
        Self::Sixteenth,
        Self::ThirtySecond,
        Self::EighthTriplet,
        Self::SixteenthTriplet,
    ];

    /// Ticks one step lasts at 960 PPQ.
    #[must_use]
    pub const fn ticks(self) -> i64 {
        match self {
            Self::Quarter => 960,
            Self::Eighth => 480,
            Self::Sixteenth => 240,
            Self::ThirtySecond => 120,
            Self::EighthTriplet => 320,
            Self::SixteenthTriplet => 160,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
            Self::ThirtySecond => "1/32",
            Self::EighthTriplet => "1/8T",
            Self::SixteenthTriplet => "1/16T",
        }
    }

    /// Position in [`Rate::ALL`]. What a session stores, so it is stable.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Quarter => 0,
            Self::Eighth => 1,
            Self::Sixteenth => 2,
            Self::ThirtySecond => 3,
            Self::EighthTriplet => 4,
            Self::SixteenthTriplet => 5,
        }
    }

    /// The rate at `index`, or the default for anything out of range — a
    /// session written by a later build names a rate this one does not have,
    /// and a pattern at the wrong rate is better than a pattern that will not
    /// load.
    #[must_use]
    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    /// One step up or down the list, stopping at the ends.
    #[must_use]
    pub fn stepped(self, delta: i32) -> Self {
        let target = (i32::from(self.index()) + delta).clamp(0, Self::ALL.len() as i32 - 1);
        Self::ALL[target as usize]
    }
}

// ── Switch quantization ──

/// When a queued pattern change takes effect.
///
/// Every one of these is a function of the song position, so the point a
/// switch will happen is known the moment it is queued — which is what lets
/// the UI count down to it rather than saying "soon".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SwitchQuant {
    /// At the end of the pattern currently playing. The default, because it
    /// is the one that keeps the part in phase with itself.
    #[default]
    PatternEnd,
    /// At the next bar line (4/4).
    Bar,
    /// At the next beat.
    Beat,
    /// At the start of the next callback.
    Immediate,
}

impl SwitchQuant {
    pub const ALL: [SwitchQuant; 4] = [Self::PatternEnd, Self::Bar, Self::Beat, Self::Immediate];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PatternEnd => "pattern",
            Self::Bar => "bar",
            Self::Beat => "beat",
            Self::Immediate => "now",
        }
    }

    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::PatternEnd => 0,
            Self::Bar => 1,
            Self::Beat => 2,
            Self::Immediate => 3,
        }
    }

    #[must_use]
    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn stepped(self, delta: i32) -> Self {
        let target = (i32::from(self.index()) + delta).clamp(0, Self::ALL.len() as i32 - 1);
        Self::ALL[target as usize]
    }

    /// The first tick at or after `now` where a switch under this
    /// quantization may happen.
    ///
    /// `pattern_ticks` is the length of the pattern that is playing — the
    /// grid `PatternEnd` counts in. A tick that is already on the grid is
    /// itself the answer, so a switch queued exactly on a bar line takes
    /// effect on that bar line rather than the next.
    #[must_use]
    pub fn boundary(self, now: i64, pattern_ticks: i64) -> i64 {
        let grid = match self {
            Self::PatternEnd => pattern_ticks,
            Self::Bar => crate::transport::Transport::PPQ * 4,
            Self::Beat => crate::transport::Transport::PPQ,
            Self::Immediate => return now,
        };
        if grid <= 0 {
            return now;
        }
        // div_euclid so that a negative position — which nothing produces
        // today, but the transport's position is an i64 — rounds towards the
        // next boundary rather than towards zero.
        (now + grid - 1).div_euclid(grid) * grid
    }
}

// ── Modes ──

/// The scale a pattern's pitch controls walk in.
///
/// Chromatic is "off": every semitone is available and the diatonic chord
/// types have no degree to derive a quality from, so they collapse to major
/// and major seventh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Chromatic,
    Ionian,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Locrian,
}

/// The major scale, which every mode here is a rotation of.
const IONIAN: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

/// Triad quality on each degree of the major scale. Every other mode reads
/// this table at an offset — that is what a mode *is*.
const IONIAN_TRIADS: [Chord; 7] = [
    Chord::Maj,
    Chord::Min,
    Chord::Min,
    Chord::Maj,
    Chord::Maj,
    Chord::Min,
    Chord::Dim,
];

/// Seventh-chord quality on each degree of the major scale.
///
/// The seventh degree is half-diminished, which is not one of the sixteen
/// chord types a step can name. It does not have to be: `diatonic7` produces
/// intervals rather than selecting a type, so the one quality with no name in
/// the list is still the one that gets played.
const IONIAN_SEVENTHS: [[i32; 4]; 7] = [
    [0, 4, 7, 11], // I maj7
    [0, 3, 7, 10], // ii m7
    [0, 3, 7, 10], // iii m7
    [0, 4, 7, 11], // IV maj7
    [0, 4, 7, 10], // V 7
    [0, 3, 7, 10], // vi m7
    [0, 3, 6, 10], // vii m7♭5
];

impl Mode {
    pub const ALL: [Mode; 8] = [
        Self::Chromatic,
        Self::Ionian,
        Self::Dorian,
        Self::Phrygian,
        Self::Lydian,
        Self::Mixolydian,
        Self::Aeolian,
        Self::Locrian,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Chromatic => "chromatic",
            Self::Ionian => "ionian",
            Self::Dorian => "dorian",
            Self::Phrygian => "phrygian",
            Self::Lydian => "lydian",
            Self::Mixolydian => "mixolydian",
            Self::Aeolian => "aeolian",
            Self::Locrian => "locrian",
        }
    }

    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Chromatic => 0,
            Self::Ionian => 1,
            Self::Dorian => 2,
            Self::Phrygian => 3,
            Self::Lydian => 4,
            Self::Mixolydian => 5,
            Self::Aeolian => 6,
            Self::Locrian => 7,
        }
    }

    #[must_use]
    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn stepped(self, delta: i32) -> Self {
        let target = (i32::from(self.index()) + delta).clamp(0, Self::ALL.len() as i32 - 1);
        Self::ALL[target as usize]
    }

    /// How far into the major scale this mode starts. `None` for Chromatic,
    /// which is not a rotation of anything.
    #[must_use]
    pub const fn rotation(self) -> Option<usize> {
        match self {
            Self::Chromatic => None,
            Self::Ionian => Some(0),
            Self::Dorian => Some(1),
            Self::Phrygian => Some(2),
            Self::Lydian => Some(3),
            Self::Mixolydian => Some(4),
            Self::Aeolian => Some(5),
            Self::Locrian => Some(6),
        }
    }

    /// Semitones above the tonic for each degree of this mode, ascending.
    ///
    /// `None` for Chromatic, which is not a scale — every semitone is a
    /// degree, and asking which one a note is on has no answer.
    #[must_use]
    pub fn scale(self) -> Option<[i32; 7]> {
        let rot = self.rotation()?;
        let base = IONIAN[rot];
        let mut out = [0; 7];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (IONIAN[(i + rot) % 7] - base).rem_euclid(12);
        }
        Some(out)
    }

    /// Which degree of this mode `note` sits on, given a tonic pitch class.
    ///
    /// `None` when the note is not in the scale — a borrowed note, which is a
    /// feature rather than a mistake, and which the diatonic chord types have
    /// no quality for.
    #[must_use]
    pub fn degree_of(self, note: u8, tonic: u8) -> Option<usize> {
        let scale = self.scale()?;
        let pitch_class = (i32::from(note) - i32::from(tonic % 12)).rem_euclid(12);
        scale.iter().position(|&s| s == pitch_class)
    }

    /// The note `steps` degrees away from `note` in this mode.
    ///
    /// Under Chromatic this is a semitone walk. In a mode it is a *scale*
    /// walk: the pitch control moves by degrees, so holding the key sweeps
    /// through the key rather than through every semitone in it. A note that
    /// is not in the scale — one that was set before the mode was, or
    /// borrowed on purpose — snaps onto the scale on the first press rather
    /// than staying off it forever.
    #[must_use]
    pub fn walk(self, note: u8, tonic: u8, steps: i32) -> u8 {
        let Some(scale) = self.scale() else {
            return (i32::from(note) + steps).clamp(0, 127) as u8;
        };
        let tonic = i32::from(tonic % 12);
        let relative = i32::from(note) - tonic;
        let octave = relative.div_euclid(12);
        let pitch_class = relative.rem_euclid(12);

        // Where the note sits in the scale, or the degree just below it when
        // it is not in the scale at all.
        let (degree, on_scale) = match scale.iter().position(|&s| s == pitch_class) {
            Some(d) => (d as i32, true),
            None => (scale.iter().filter(|&&s| s < pitch_class).count() as i32 - 1, false),
        };
        // `degree` is the degree *below* a borrowed note, so walking up from
        // one lands on the next degree already and walking down has to give
        // back the step it would otherwise skip. Either direction snaps onto
        // the scale on the first press.
        let target = degree + steps + i32::from(!on_scale && steps < 0);
        let target_octave = octave + target.div_euclid(7);
        let target_degree = target.rem_euclid(7) as usize;
        (tonic + target_octave * 12 + scale[target_degree]).clamp(0, 127) as u8
    }
}

// ── Chords ──

/// What a melodic step plays: one note, or several.
///
/// The order of this list is on disk — a step stores the chord it names by
/// identity, so entries may be appended but never reordered. `Diatonic` and
/// `Diatonic7` take their quality from the degree the root sits on in the
/// pattern's mode, which is what makes a whole line of them sound like a key
/// rather than like one chord transposed; every other entry is explicit, so
/// that a borrowed chord stays borrowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chord {
    #[default]
    None,
    Fifth,
    Octave,
    Diatonic,
    Diatonic7,
    Maj,
    Min,
    Dim,
    Sus2,
    Sus4,
    Maj6,
    Min6,
    Dom7,
    Min7,
    Maj7,
    Quartal,
}

impl Chord {
    pub const ALL: [Chord; 16] = [
        Self::None,
        Self::Fifth,
        Self::Octave,
        Self::Diatonic,
        Self::Diatonic7,
        Self::Maj,
        Self::Min,
        Self::Dim,
        Self::Sus2,
        Self::Sus4,
        Self::Maj6,
        Self::Min6,
        Self::Dom7,
        Self::Min7,
        Self::Maj7,
        Self::Quartal,
    ];

    /// The identity a step stores. Stable forever.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Fifth => 1,
            Self::Octave => 2,
            Self::Diatonic => 3,
            Self::Diatonic7 => 4,
            Self::Maj => 5,
            Self::Min => 6,
            Self::Dim => 7,
            Self::Sus2 => 8,
            Self::Sus4 => 9,
            Self::Maj6 => 10,
            Self::Min6 => 11,
            Self::Dom7 => 12,
            Self::Min7 => 13,
            Self::Maj7 => 14,
            Self::Quartal => 15,
        }
    }

    #[must_use]
    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn stepped(self, delta: i32) -> Self {
        let target = (i32::from(self.index()) + delta).clamp(0, Self::ALL.len() as i32 - 1);
        Self::ALL[target as usize]
    }

    /// Semitones above the root, written into `out`, and how many there are.
    ///
    /// `mode` and `tonic` are only read by the two diatonic entries; under
    /// Chromatic, or on a root the mode does not contain, they fall back to
    /// major and major seventh.
    fn intervals(self, root: u8, mode: Mode, tonic: u8, out: &mut [i32; 4]) -> usize {
        let fixed: &[i32] = match self {
            Self::None => &[0],
            Self::Fifth => &[0, 7],
            Self::Octave => &[0, 12],
            Self::Maj => &[0, 4, 7],
            Self::Min => &[0, 3, 7],
            Self::Dim => &[0, 3, 6],
            Self::Sus2 => &[0, 2, 7],
            Self::Sus4 => &[0, 5, 7],
            Self::Maj6 => &[0, 4, 7, 9],
            Self::Min6 => &[0, 3, 7, 9],
            Self::Dom7 => &[0, 4, 7, 10],
            Self::Min7 => &[0, 3, 7, 10],
            Self::Maj7 => &[0, 4, 7, 11],
            // Stacked fourths. Three notes rather than four, so it sits in
            // the same register as the triads it is chosen against.
            Self::Quartal => &[0, 5, 10],
            Self::Diatonic | Self::Diatonic7 => {
                let seventh = self == Self::Diatonic7;
                let quality = mode
                    .degree_of(root, tonic)
                    .map(|degree| (degree + mode.rotation().unwrap_or(0)) % 7);
                return match (quality, seventh) {
                    (Some(d), false) => IONIAN_TRIADS[d].intervals(root, mode, tonic, out),
                    (Some(d), true) => {
                        out.copy_from_slice(&IONIAN_SEVENTHS[d]);
                        4
                    }
                    (None, false) => Self::Maj.intervals(root, mode, tonic, out),
                    (None, true) => Self::Maj7.intervals(root, mode, tonic, out),
                };
            }
        };
        out[..fixed.len()].copy_from_slice(fixed);
        fixed.len()
    }
}

/// How the notes of a chord are spread out.
///
/// Every one of these preserves the chord's pitch-class set — it is the same
/// chord, arranged differently — which is the property the tests check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Voicing {
    #[default]
    Close,
    /// The second voice from the top, down an octave. The definition is
    /// worth being precise about: it is not "the middle note", and on a
    /// four-note chord it is the third note up, not the second.
    Drop2,
    /// Lowest voice up an octave.
    First,
    /// Lowest two voices up an octave.
    Second,
}

impl Voicing {
    pub const ALL: [Voicing; 4] = [Self::Close, Self::Drop2, Self::First, Self::Second];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Drop2 => "drop-2",
            Self::First => "1st inv",
            Self::Second => "2nd inv",
        }
    }

    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Close => 0,
            Self::Drop2 => 1,
            Self::First => 2,
            Self::Second => 3,
        }
    }

    #[must_use]
    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    #[must_use]
    pub fn stepped(self, delta: i32) -> Self {
        let target = (i32::from(self.index()) + delta).clamp(0, Self::ALL.len() as i32 - 1);
        Self::ALL[target as usize]
    }
}

/// The most notes one step can produce: a four-note chord plus the bass
/// double.
pub const MAX_CHORD_NOTES: usize = 5;

/// The notes one step plays, ascending, written into `out`.
///
/// Returns how many were written. Out-of-range results are folded by octaves
/// rather than clamped or dropped — folding keeps the pitch class, and a
/// chord that loses a note near the top of the keyboard is a chord that
/// changes quality where nobody asked it to. Exact duplicates after folding
/// are removed, because two note-ons of the same number on one lane leave the
/// child instrument holding a voice nothing will turn off.
#[must_use]
pub fn chord_notes(
    root: u8,
    chord: Chord,
    voicing: Voicing,
    root_below: bool,
    mode: Mode,
    tonic: u8,
    out: &mut [u8; MAX_CHORD_NOTES],
) -> usize {
    let mut intervals = [0i32; 4];
    let count = chord.intervals(root, mode, tonic, &mut intervals);

    let mut voices = [0i32; MAX_CHORD_NOTES];
    for (slot, interval) in voices.iter_mut().zip(&intervals[..count]) {
        *slot = i32::from(root) + interval;
    }
    let mut len = count;

    // Voicings act on the chord as it stands, ascending. A one-note chord has
    // nothing to rearrange, which is why every branch checks the count.
    match voicing {
        Voicing::Close => {}
        Voicing::Drop2 => {
            if len >= 2 {
                voices[len - 2] -= 12;
            }
        }
        Voicing::First => {
            if len >= 2 {
                voices[0] += 12;
            }
        }
        Voicing::Second => {
            if len >= 3 {
                voices[0] += 12;
                voices[1] += 12;
            } else if len >= 2 {
                voices[0] += 12;
            }
        }
    }

    if root_below && len < MAX_CHORD_NOTES {
        voices[len] = i32::from(root) - 12;
        len += 1;
    }

    // Fold into the MIDI range, sort, and drop exact duplicates.
    for voice in &mut voices[..len] {
        while *voice < 0 {
            *voice += 12;
        }
        while *voice > 127 {
            *voice -= 12;
        }
    }
    voices[..len].sort_unstable();

    let mut written = 0;
    for i in 0..len {
        if i > 0 && voices[i] == voices[i - 1] {
            continue;
        }
        out[written] = voices[i] as u8;
        written += 1;
    }
    written
}

// ── Step ──

/// One cell of the grid.
///
/// Nine bytes, laid out so that the whole pattern is a plain block of memory
/// that can be memcpy'd to the audio thread. `reserved` is room for the two
/// per-step features that are already designed and not yet built —
/// probability and ratchets — so that adding them later is not a change to
/// the size of anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// Whether this step fires.
    pub on: bool,
    /// Octave part of the pitch: the note is `octave * 12 + key`. The UI
    /// presents one pitch control; the storage stays split because that is
    /// what a mode-quantised walk needs.
    pub octave: u8,
    /// Pitch class part of the pitch, 0..=11.
    pub key: u8,
    /// Which [`Chord`] this step plays, by [`Chord::index`].
    pub chord: u8,
    /// Which [`Voicing`], by [`Voicing::index`], plus [`Step::ROOT_BELOW`].
    pub voicing: u8,
    /// Whether the step takes the pattern's accent velocity rather than its
    /// base velocity. Per-step numeric velocity is a later change to this
    /// same field and needs no migration.
    pub accent: bool,
    /// Gate length as a percentage of the step, or [`Step::TIE`].
    pub gate: u8,
    /// Probability and ratchets, when they arrive.
    pub reserved: [u8; 2],
}

impl Step {
    /// A gate that holds the note until the lane's next onset.
    ///
    /// 255 rather than a separate field because a gate is one control: the
    /// UI walks it up through the percentages and off the end into the tie,
    /// which is where a player expects to find it.
    pub const TIE: u8 = 255;

    /// Shortest gate. Below this the note-off arrives before an envelope has
    /// opened and the step is inaudible, which reads as a broken step.
    pub const MIN_GATE: u8 = 5;

    /// Longest gate: twice the step, so a step can hold through the next one.
    pub const MAX_GATE: u8 = 200;

    /// Bit in [`Step::voicing`] that doubles the root an octave below.
    ///
    /// Independent of the voicing rather than four more entries in the list,
    /// because it composes with all of them.
    pub const ROOT_BELOW: u8 = 0b0000_0100;

    /// A step that is off, at middle C, one note, half gate.
    #[must_use]
    pub const fn silent() -> Self {
        Self {
            on: false,
            octave: 5,
            key: 0,
            chord: 0,
            voicing: 0,
            accent: false,
            gate: 50,
            reserved: [0; 2],
        }
    }

    /// The root note this step plays, clamped into the MIDI range.
    #[must_use]
    pub fn root(self) -> u8 {
        (u32::from(self.octave) * 12 + u32::from(self.key)).min(127) as u8
    }

    #[must_use]
    pub fn chord_kind(self) -> Chord {
        Chord::from_index(self.chord)
    }

    #[must_use]
    pub fn voicing_kind(self) -> Voicing {
        Voicing::from_index(self.voicing & 0b11)
    }

    #[must_use]
    pub fn root_below(self) -> bool {
        self.voicing & Self::ROOT_BELOW != 0
    }

    /// How long this step holds, in ticks — or `None` when it is tied and
    /// only the lane's next onset ends it.
    ///
    /// The gate is clamped here rather than where it is written, so that a
    /// value out of range can only ever shorten or lengthen a note rather
    /// than produce one with a negative length.
    #[must_use]
    pub fn gate_ticks(self, ticks_per_step: i64) -> Option<i64> {
        if self.gate == Self::TIE {
            return None;
        }
        let percent = i64::from(self.gate.clamp(Self::MIN_GATE, Self::MAX_GATE));
        Some((ticks_per_step * percent / 100).max(1))
    }
}

impl Default for Step {
    fn default() -> Self {
        Self::silent()
    }
}

// ── Lane ──

/// One row of the grid: a voice, and the steps that fire it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    /// The note every step on this lane plays, or [`Lane::FROM_STEP`] when
    /// the pitch comes from the step instead.
    ///
    /// A drum lane is pinned to one note from the kit's map — that is what
    /// makes it "the kick lane" — and a melodic lane takes its pitch, its
    /// chord and its voicing from each step.
    pub note: u8,
    pub muted: bool,
    pub soloed: bool,
    pub steps: [Step; MAX_STEPS],
}

impl Lane {
    /// [`Lane::note`] for a lane whose pitch comes from its steps. Outside
    /// the MIDI range, so it cannot collide with a real note.
    pub const FROM_STEP: u8 = 0xFF;

    /// An empty melodic lane.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            note: Self::FROM_STEP,
            muted: false,
            soloed: false,
            steps: [Step::silent(); MAX_STEPS],
        }
    }

    /// An empty lane pinned to one drum voice.
    #[must_use]
    pub const fn drum(note: u8) -> Self {
        Self { note, ..Self::empty() }
    }

    /// Whether this lane's pitch comes from its steps.
    #[must_use]
    pub const fn is_pitched(&self) -> bool {
        self.note == Self::FROM_STEP
    }
}

impl Default for Lane {
    fn default() -> Self {
        Self::empty()
    }
}

// ── Chain ──

/// One entry of a pattern chain: a slot, and how many times through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChainEntry {
    pub slot: u8,
    /// Times through before moving on. Zero is read as one.
    pub repeats: u8,
}

// ── PatternBlock ──

/// A whole pattern, as the audio thread holds it.
///
/// Every field is a plain scalar or an array of them: the type is `Copy`,
/// nothing in it points anywhere, and a command carrying one is a memcpy.
///
/// Five of the fields — `playing`, `pending_slot`, `switch_quant`, `chain`
/// and `chain_len` — describe the *track* rather than this pattern. They ride
/// on the block because a block is how the UI thread says anything to the
/// audio thread about a sequencer, and [`PatternPlayer::apply`] takes the
/// most recent word on them from whichever slot arrived last. The UI keeps
/// its own single copy and writes it into every block it sends, which is what
/// `phosphor-app`'s single dispatch function exists to guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternBlock {
    /// How many steps play, one of [`STEP_COUNTS`].
    ///
    /// Shortening a pattern *masks*: steps past the end keep their contents
    /// and come back when the pattern is lengthened again.
    pub steps: u8,
    pub rate: Rate,
    /// Swing as a percentage, 50 (straight) to 75 (fully triplet).
    pub swing: u8,
    /// Velocity of an ordinary step.
    pub base_vel: u8,
    /// Velocity of an accented step.
    pub accent_vel: u8,
    /// The gate a newly enabled step inherits.
    pub default_gate: u8,
    /// The scale pitch walking and the diatonic chords work in.
    pub mode: Mode,
    /// Tonic pitch class for `mode`, 0..=11. Named `tonic` and not `key`
    /// because [`Step::key`] is a different thing one field away.
    pub tonic: u8,
    pub lanes: [Lane; LANES],
    /// Whether this sequencer runs when the transport does.
    pub playing: bool,
    /// A slot queued to take over at the next [`SwitchQuant`] point.
    ///
    /// Queueing the slot that is already playing is not a switch, which is
    /// what makes a stale queue on the UI side harmless.
    pub pending_slot: Option<u8>,
    pub switch_quant: SwitchQuant,
    pub chain: [ChainEntry; MAX_CHAIN],
    /// How many entries of `chain` are real. Zero means no chain, and the
    /// live slot is whatever was last selected or queued.
    pub chain_len: u8,
}

impl PatternBlock {
    /// How many bytes one of these is, and therefore what a `SetPattern`
    /// command costs to sit in the queue.
    pub const SIZE: usize = std::mem::size_of::<Self>();

    /// Lowest swing setting: straight.
    pub const MIN_SWING: u8 = 50;

    /// Highest swing setting. At 75 the pair is 3:1, which is a triplet
    /// feel; past it the second note lands on the following step and the
    /// pattern reads as a different rhythm rather than a swung one.
    pub const MAX_SWING: u8 = 75;

    /// An empty 16-step pattern at a sixteenth, straight, and RUNNING.
    ///
    /// Running by default is the difference between a sequencer and a trap:
    /// a user writes steps, presses play, and must hear them. On hardware
    /// the pattern plays when the machine plays; the run/stop toggle exists
    /// to mute a pattern during a performance, not to stand between a
    /// beginner and their first sound. This shipped as `false` once and the
    /// first real user pressed play into silence.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            steps: 16,
            rate: Rate::Sixteenth,
            swing: Self::MIN_SWING,
            base_vel: 100,
            accent_vel: 127,
            default_gate: 50,
            mode: Mode::Chromatic,
            tonic: 0,
            lanes: [Lane::empty(); LANES],
            playing: true,
            pending_slot: None,
            switch_quant: SwitchQuant::PatternEnd,
            chain: [ChainEntry { slot: 0, repeats: 1 }; MAX_CHAIN],
            chain_len: 0,
        }
    }

    /// How many steps actually play. Always at least one, never more than
    /// [`MAX_STEPS`] — a length out of range would otherwise index off the
    /// end of a lane or divide by zero.
    #[must_use]
    pub fn step_count(&self) -> usize {
        (self.steps as usize).clamp(1, MAX_STEPS)
    }

    #[must_use]
    pub fn ticks_per_step(&self) -> i64 {
        self.rate.ticks()
    }

    /// One time through, in ticks.
    #[must_use]
    pub fn length_ticks(&self) -> i64 {
        self.ticks_per_step() * self.step_count() as i64
    }

    /// How far an odd-numbered step is pushed late, in ticks.
    ///
    /// MPC-style: the offset is a fraction of *two* steps, so 75% puts the
    /// off-beat three quarters of the way through the pair — a triplet feel —
    /// and 50% is straight. Integer arithmetic on purpose: the bounce and the
    /// live player run this same expression, so "the bounce swings
    /// identically" needs no tolerance at all.
    ///
    /// Even step indices are never offset, which is what keeps the downbeat
    /// where the transport says it is.
    #[must_use]
    pub fn swing_offset(&self, step_index: usize) -> i64 {
        if step_index % 2 == 0 {
            return 0;
        }
        let swing = i64::from(self.swing.clamp(Self::MIN_SWING, Self::MAX_SWING));
        (swing - i64::from(Self::MIN_SWING)) * 2 * self.ticks_per_step() / 100
    }

    /// The largest [`PatternBlock::swing_offset`] this pattern can produce —
    /// how far back a scan has to start to be sure of catching every onset.
    fn max_swing_offset(&self) -> i64 {
        let swing = i64::from(self.swing.clamp(Self::MIN_SWING, Self::MAX_SWING));
        (swing - i64::from(Self::MIN_SWING)) * 2 * self.ticks_per_step() / 100
    }

    /// The tick step `index` fires at, counting from `origin`.
    ///
    /// `index` is a *global* step number and may run past the end of the
    /// pattern or before its start: index 17 of a 16-step pattern is step 1
    /// of the second time through.
    #[must_use]
    pub fn onset(&self, origin: i64, index: i64) -> i64 {
        let steps = self.step_count() as i64;
        let in_pattern = index.rem_euclid(steps) as usize;
        origin + index * self.ticks_per_step() + self.swing_offset(in_pattern)
    }

    /// Which step is under `tick`, counting from `origin`. Swing is not
    /// applied: this is where the playhead is, not when a note fires.
    #[must_use]
    pub fn step_at(&self, origin: i64, tick: i64) -> usize {
        let steps = self.step_count() as i64;
        (tick - origin).div_euclid(self.ticks_per_step()).rem_euclid(steps) as usize
    }

    /// Whether a lane sounds, given the pattern's mute and solo state.
    #[must_use]
    pub fn lane_audible(&self, lane: usize) -> bool {
        let Some(l) = self.lanes.get(lane) else { return false };
        if l.muted {
            return false;
        }
        let any_solo = self.lanes.iter().any(|l| l.soloed);
        !any_solo || l.soloed
    }

    /// The chain as real entries, ignoring anything past `chain_len`.
    #[must_use]
    pub fn chain_entries(&self) -> &[ChainEntry] {
        &self.chain[..(self.chain_len as usize).min(MAX_CHAIN)]
    }
}

impl Default for PatternBlock {
    fn default() -> Self {
        Self::empty()
    }
}

// ── Events ──

/// One MIDI event a pattern produced, stamped with the absolute song tick it
/// happens at.
///
/// A tick rather than a sample offset because this module has no sample rate:
/// [`PlaybackWindow::sample_offset`] is where a tick becomes a position in a
/// buffer, and the bounce never asks that question at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternEvent {
    pub tick: i64,
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl PatternEvent {
    #[must_use]
    pub const fn note_on(tick: i64, note: u8, velocity: u8) -> Self {
        Self { tick, status: 0x90, data1: note, data2: velocity }
    }

    #[must_use]
    pub const fn note_off(tick: i64, note: u8) -> Self {
        Self { tick, status: 0x80, data1: note, data2: 0 }
    }

    #[must_use]
    pub const fn is_note_on(&self) -> bool {
        self.status == 0x90 && self.data2 > 0
    }
}

/// Somewhere for generated events to go.
///
/// The whole point of the trait is that there is one generator behind both
/// consumers: the audio thread writes straight into the track's plugin queue,
/// converting ticks to sample offsets as they arrive and refusing to grow it,
/// and the bounce writes into a `Vec` that cannot overflow. Generic rather
/// than `dyn`, so each call site compiles to the same code it would if the
/// sink were named directly.
///
/// Events arrive in no particular tick order. Both consumers sort by tick or
/// by sample offset with a *stable* sort, which is what carries the one
/// ordering rule that matters: a note-off pushed before a note-on at the same
/// tick stays before it, and so a switch boundary cannot cut the note it just
/// started.
pub trait EventSink {
    /// Returns whether the event was taken. `false` means the sink is full,
    /// and the generator stops rather than dropping events silently in the
    /// middle of a step.
    fn accept(&mut self, event: PatternEvent) -> bool;
}

impl EventSink for Vec<PatternEvent> {
    fn accept(&mut self, event: PatternEvent) -> bool {
        self.push(event);
        true
    }
}

// ── The playback window ──

/// The span of song time one callback renders.
///
/// **This is the sync guarantee.** Clip playback and pattern playback in
/// `mixer.rs` take the same value and ask it the same question, so a clip
/// note and a pattern step on the same beat cannot land on different samples:
/// there is only one expression that turns a tick into a sample offset, and
/// only one that decides where the window starts and ends.
///
/// Windows are half-open and contiguous. The next window begins exactly where
/// this one ended, rather than at the transport's new position, because those
/// are not always the same number: a block is almost never a whole number of
/// ticks, and a transport that carries the remainder can advance 179 ticks
/// where the block measured 178. Starting the next window at the position
/// would leave a one-tick hole in song time, and an onset that fell in it
/// would never play. Starting it where the last one ended cannot.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackWindow {
    from: i64,
    to: i64,
    /// Where the transport actually was when this callback began, which is
    /// not always `from`: see [`PlaybackWindow::for_block`]. Kept because it
    /// is the only honest thing to compare the *next* block's position
    /// against when deciding whether the playhead moved.
    position: i64,
    ticks_per_sample: f64,
    frames: u32,
    continuous: bool,
}

impl PlaybackWindow {
    /// The largest gap between one window's end and the next block's position
    /// that still counts as continuous playback.
    ///
    /// One tick, and it is not a fudge factor: the block length in ticks is
    /// truncated and the transport's advance is not, so consecutive positions
    /// can run at most one tick ahead of the measured window. Anything larger
    /// is the playhead being moved, which is a discontinuity — pending notes
    /// get flushed and nothing is replayed.
    pub const MAX_TICK_GAP: i64 = 1;

    /// The window for one callback.
    ///
    /// `loop_region` is the loop's `(start, end)` when the transport is
    /// looping, and `None` when it is not — one argument rather than a flag
    /// and two numbers, because "looping over nowhere" is not a state that
    /// should be spellable.
    ///
    /// `previous` is the window the last callback used, if playback has been
    /// running. Two things come from the loop region:
    ///
    /// * A wrap — the transport moving backwards — starts the window at the
    ///   loop point, so the ticks between it and the position the callback
    ///   arrived at are played rather than skipped. That is what clip
    ///   playback has always done.
    /// * The window never extends past the loop end. Without that, the last
    ///   callback of a loop would reach across the loop point and play the
    ///   first notes on the other side of it, and then the wrap would play
    ///   them again: one doubled downbeat per time round.
    #[must_use]
    pub fn for_block(
        position: i64,
        frames: u32,
        ticks_per_sample: f64,
        loop_region: Option<(i64, i64)>,
        previous: Option<Self>,
    ) -> Self {
        let span = (f64::from(frames) * ticks_per_sample) as i64;

        let (from, continuous) = match (previous, loop_region) {
            (Some(prev), Some((loop_start, _))) if position < prev.position => (loop_start, false),
            (Some(prev), _) if prev.to <= position && position - prev.to <= Self::MAX_TICK_GAP => {
                (prev.to, true)
            }
            _ => (position, false),
        };

        let mut to = position + span;
        if let Some((_, loop_end)) = loop_region {
            if loop_end > from {
                to = to.min(loop_end);
            }
        }

        Self {
            from,
            to: to.max(from),
            position,
            ticks_per_sample,
            frames,
            continuous,
        }
    }

    /// A window over part of this one, for splitting a callback at a pattern
    /// switch. Sample offsets are unchanged: they are measured from the
    /// original start of the block, not from the piece.
    #[must_use]
    pub fn narrowed(&self, from: i64, to: i64) -> Self {
        Self { from, to: to.max(from), ..*self }
    }

    #[must_use]
    pub const fn from(&self) -> i64 {
        self.from
    }

    #[must_use]
    pub const fn to(&self) -> i64 {
        self.to
    }

    /// Whether this window carries on from the previous one. `false` after a
    /// jump, a loop wrap, or the first block of playback — every case where
    /// notes still sounding have to be turned off.
    #[must_use]
    pub const fn is_continuous(&self) -> bool {
        self.continuous
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.to <= self.from
    }

    #[must_use]
    pub const fn contains(&self, tick: i64) -> bool {
        tick >= self.from && tick < self.to
    }

    /// Where in the callback's buffer an event at `tick` belongs.
    ///
    /// The one expression that turns song time into a sample. A tick before
    /// the window lands on the first sample rather than underflowing, and one
    /// past the end lands on the last: a note played at the wrong end of a
    /// buffer is 1.5 ms out, and a note not played at all is a hole in the
    /// part.
    #[must_use]
    pub fn sample_offset(&self, tick: i64) -> u32 {
        let last = self.frames.saturating_sub(1);
        let offset = tick - self.from;
        if offset <= 0 || self.ticks_per_sample <= 0.0 {
            return 0;
        }
        let samples = (offset as f64 / self.ticks_per_sample) as i64;
        u32::try_from(samples).unwrap_or(last).min(last)
    }
}

// ── Pending note-offs ──

/// A note that is sounding and the tick it has to stop at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingOff {
    note: u8,
    lane: u8,
    /// When the note-off is due, or `None` for a tied note, which is ended
    /// by the lane's next onset and by nothing else.
    due: Option<i64>,
}

/// Every note this track is holding, oldest first.
///
/// Oldest-first is maintained by removing with a shift rather than a swap,
/// which is what makes "overflow forces off the oldest" a one-line operation
/// on a table of thirty-two. The shift is at most 32 moves of 24 bytes.
#[derive(Debug, Clone, Copy)]
pub struct PendingOffs {
    entries: [PendingOff; MAX_PENDING_OFFS],
    len: usize,
}

impl PendingOffs {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [PendingOff { note: 0, lane: 0, due: None }; MAX_PENDING_OFFS],
            len: 0,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Forget everything without sounding an off. For a panic, where the
    /// instruments are being reset underneath us anyway.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    fn remove(&mut self, index: usize) -> PendingOff {
        let gone = self.entries[index];
        for i in index..self.len - 1 {
            self.entries[i] = self.entries[i + 1];
        }
        self.len -= 1;
        gone
    }

    /// Note that `note` is sounding on `lane`.
    ///
    /// When the table is full the oldest note is turned off at `now` and its
    /// slot reused. Never drops the new note: a note with no off in the table
    /// is a note nothing will ever stop.
    pub fn hold(
        &mut self,
        lane: usize,
        note: u8,
        due: Option<i64>,
        now: i64,
        out: &mut impl EventSink,
    ) {
        if self.len == MAX_PENDING_OFFS {
            let oldest = self.remove(0);
            out.accept(PatternEvent::note_off(now, oldest.note));
        }
        self.entries[self.len] = PendingOff { note, lane: lane as u8, due };
        self.len += 1;
    }

    /// Turn off everything held on `lane`, at `at`.
    ///
    /// Called immediately before a lane's next onset, which is what ends a
    /// tied note and what keeps a gate longer than a step from running over
    /// its own next hit.
    pub fn end_lane(&mut self, lane: usize, at: i64, out: &mut impl EventSink) {
        let lane = lane as u8;
        let mut i = 0;
        while i < self.len {
            if self.entries[i].lane == lane {
                let gone = self.remove(i);
                out.accept(PatternEvent::note_off(at, gone.note));
            } else {
                i += 1;
            }
        }
    }

    /// Turn off everything whose off is due before `tick`, each at its own
    /// due tick.
    pub fn emit_due_before(&mut self, tick: i64, out: &mut impl EventSink) {
        let mut i = 0;
        while i < self.len {
            match self.entries[i].due {
                Some(due) if due < tick => {
                    let gone = self.remove(i);
                    out.accept(PatternEvent::note_off(due, gone.note));
                }
                _ => i += 1,
            }
        }
    }

    /// Turn off everything, at `at`. Stop, pause, a position jump, a loop
    /// wrap, a pattern switch: every discontinuity ends every note.
    pub fn flush(&mut self, at: i64, out: &mut impl EventSink) {
        for i in 0..self.len {
            out.accept(PatternEvent::note_off(at, self.entries[i].note));
        }
        self.len = 0;
    }
}

impl Default for PendingOffs {
    fn default() -> Self {
        Self::new()
    }
}

// ── Generation ──

/// Write every event one pattern produces between `from` and `to`.
///
/// Pure apart from `pending`, which is the caller's note-off table: the same
/// call with the same table produces the same events, which is why the bounce
/// can run it once over a whole cycle and get what the audio thread produces
/// over hundreds of callbacks.
///
/// `origin` is the tick the pattern's step 0 is anchored to — 0 for a pattern
/// playing on its own, and the start of the chain entry when a chain is
/// running. Nothing here keeps a cursor: which step fires is derived from the
/// tick, every time.
pub fn generate(
    block: &PatternBlock,
    origin: i64,
    from: i64,
    to: i64,
    pending: &mut PendingOffs,
    out: &mut impl EventSink,
) {
    if to <= from {
        return;
    }
    let tps = block.ticks_per_step();
    let steps = block.step_count() as i64;

    // Which global step indices could have an onset inside the window. Swing
    // only ever pushes a step *later*, so the scan starts one full swing
    // offset early and every candidate is checked against the window anyway.
    let first = (from - origin - block.max_swing_offset()).div_euclid(tps);
    let last = (to - origin).div_euclid(tps) + 1;
    let last = last.min(first + MAX_STEP_SCAN);

    let mut chord = [0u8; MAX_CHORD_NOTES];
    for index in first..last {
        let onset = block.onset(origin, index);
        if onset < from || onset >= to {
            continue;
        }
        // Everything that was already due before this onset goes first, so a
        // lane that re-triggers cannot have its new note cut by the old one's
        // off arriving afterwards.
        pending.emit_due_before(onset, out);

        let step_index = index.rem_euclid(steps) as usize;
        for lane_index in 0..LANES {
            if !block.lane_audible(lane_index) {
                continue;
            }
            let lane = &block.lanes[lane_index];
            let step = lane.steps[step_index];
            if !step.on {
                continue;
            }

            // The lane's previous note ends here, before the new one starts.
            // Insertion order is what carries that through the sort.
            pending.end_lane(lane_index, onset, out);

            let velocity = if step.accent { block.accent_vel } else { block.base_vel };
            let velocity = velocity.clamp(1, 127);
            let due = step.gate_ticks(tps).map(|len| onset + len);

            let count = if lane.is_pitched() {
                chord_notes(
                    step.root(),
                    step.chord_kind(),
                    step.voicing_kind(),
                    step.root_below(),
                    block.mode,
                    block.tonic,
                    &mut chord,
                )
            } else {
                chord[0] = lane.note;
                1
            };

            for &note in &chord[..count] {
                if !out.accept(PatternEvent::note_on(onset, note, velocity)) {
                    return;
                }
                pending.hold(lane_index, note, due, onset, out);
            }
        }
    }

    pending.emit_due_before(to, out);
}

/// Compile one time through a pattern, as note events from tick zero.
///
/// The bounce. It calls [`generate`] over the whole cycle in one window
/// rather than reimplementing it, so swing, gates, ties and accents are not
/// "the same as" live playback — they are live playback, run with a different
/// sink. Notes still sounding at the end of the cycle are turned off at the
/// cycle's last tick, which is where the pattern would have ended them had it
/// stopped there.
pub fn compile_cycle(block: &PatternBlock, origin: i64, out: &mut Vec<PatternEvent>) {
    let length = block.length_ticks();
    let mut pending = PendingOffs::new();
    generate(block, origin, origin, origin + length, &mut pending, out);
    pending.flush(origin + length, out);
    out.sort_by_key(|e| e.tick);
}

// ── The player ──

/// Everything one sequencer track needs on the audio thread.
///
/// The bank lives here rather than on the UI side because a pattern switch
/// has to be *decided* on the audio thread: the quantization point is a tick,
/// the tick arrives in the middle of a callback, and asking the UI what to
/// play next at that moment would make the answer depend on when the UI
/// thread happened to be scheduled. With all eight slots resident, a switch
/// is an index change and a chain is a lookup.
///
/// Around 19 kB per sequencer track, allocated once, when the track's first
/// pattern arrives — the same shape as an instrument allocating its voice
/// array in `Plugin::init`. Nothing after that reaches the allocator.
#[derive(Debug, Clone, Copy)]
pub struct PatternPlayer {
    slots: [PatternBlock; SLOTS],
    /// The slot currently sounding.
    live: u8,
    /// The last word the UI thread said about the track, taken from whichever
    /// block arrived most recently. See [`PatternBlock`].
    playing: bool,
    pending_slot: Option<u8>,
    switch_quant: SwitchQuant,
    chain: [ChainEntry; MAX_CHAIN],
    chain_len: u8,
    /// Notes this track is holding.
    pending: PendingOffs,
    /// Whether the last callback was producing notes, so that stopping can
    /// flush exactly once.
    active: bool,
    /// The step the playhead was over when this player last ran, for the UI.
    step: u8,
}

impl PatternPlayer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: [PatternBlock::empty(); SLOTS],
            live: 0,
            playing: false,
            pending_slot: None,
            switch_quant: SwitchQuant::PatternEnd,
            chain: [ChainEntry { slot: 0, repeats: 1 }; MAX_CHAIN],
            chain_len: 0,
            pending: PendingOffs::new(),
            active: false,
            step: 0,
        }
    }

    /// Take a pattern for one slot, and with it the UI's current word on the
    /// track-level settings.
    pub fn apply(&mut self, slot: u8, block: PatternBlock) {
        let slot = (slot as usize).min(SLOTS - 1);
        self.slots[slot] = block;
        self.playing = block.playing;
        self.switch_quant = block.switch_quant;
        self.chain = block.chain;
        self.chain_len = block.chain_len;
        // Queueing the slot that is already live is not a switch, so a UI
        // mirror that still names a slot the audio thread has already
        // switched to cannot cause a second, silent switch. A running chain
        // owns the slot outright, so a queue against one is not held.
        self.pending_slot = block
            .pending_slot
            .filter(|&s| s != self.live && block.chain_len == 0);
    }

    #[must_use]
    pub fn slot(&self, index: usize) -> &PatternBlock {
        &self.slots[index.min(SLOTS - 1)]
    }

    #[must_use]
    pub fn live_slot(&self) -> u8 {
        self.live
    }

    #[must_use]
    pub fn queued_slot(&self) -> Option<u8> {
        self.pending_slot
    }

    /// The step the playhead was over on the last callback. What the UI draws
    /// its marker at.
    #[must_use]
    pub fn current_step(&self) -> u8 {
        self.step
    }

    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    #[must_use]
    pub fn held_notes(&self) -> usize {
        self.pending.len()
    }

    /// Forget every held note without sounding an off. For a panic, where the
    /// instruments are reset underneath us.
    pub fn silence(&mut self) {
        self.pending.clear();
        self.active = false;
    }

    /// Where the switch queued on this track will happen, and how many steps
    /// away that is from `now`. `None` when nothing is queued.
    ///
    /// Pure, and the UI computes the same answer from its own mirror: the
    /// countdown on screen is arithmetic, not a message from the audio
    /// thread that may or may not have arrived yet.
    #[must_use]
    pub fn countdown(&self, now: i64) -> Option<(u8, i64)> {
        let slot = self.pending_slot?;
        let block = &self.slots[self.live as usize];
        let at = self.switch_quant.boundary(now, block.length_ticks());
        Some((slot, (at - now).div_euclid(block.ticks_per_step())))
    }

    /// Which slot plays at `tick`, where its step 0 is anchored, and the tick
    /// that answer stops being true at.
    ///
    /// A chain is read straight off the song position, exactly as a step is:
    /// the chain is a program the position indexes into rather than a cursor
    /// something advances, so dropping the playhead into bar 40 lands in the
    /// entry that belongs there. Without a chain the live slot is whatever
    /// was last selected, and the boundary is the queued switch, if any.
    fn locate(&self, tick: i64) -> (u8, i64, i64) {
        if let Some(found) = self.chain_at(tick) {
            return found;
        }
        let boundary = match self.pending_slot {
            Some(_) => {
                let block = &self.slots[self.live as usize];
                self.switch_quant.boundary(tick, block.length_ticks())
            }
            None => i64::MAX,
        };
        (self.live, 0, boundary)
    }

    /// The chain entry covering `tick`, if a chain is running.
    fn chain_at(&self, tick: i64) -> Option<(u8, i64, i64)> {
        let entries = &self.chain[..(self.chain_len as usize).min(MAX_CHAIN)];
        if entries.is_empty() {
            return None;
        }
        let mut total = 0i64;
        for entry in entries {
            let slot = (entry.slot as usize).min(SLOTS - 1);
            total += i64::from(entry.repeats.max(1)) * self.slots[slot].length_ticks();
        }
        if total <= 0 {
            return None;
        }

        let base = tick.div_euclid(total) * total;
        let mut offset = tick.rem_euclid(total);
        let mut start = base;
        for entry in entries {
            let slot = (entry.slot as usize).min(SLOTS - 1);
            let span = i64::from(entry.repeats.max(1)) * self.slots[slot].length_ticks();
            if offset < span {
                return Some((slot as u8, start, start + span));
            }
            offset -= span;
            start += span;
        }
        None
    }

    /// Produce this callback's events.
    ///
    /// `transport_playing` is the DAW transport; the pattern also has to be
    /// running on its own account. Everything else — which step, which slot,
    /// where the switch lands — comes out of the window's ticks.
    pub fn render(
        &mut self,
        window: &PlaybackWindow,
        transport_playing: bool,
        out: &mut impl EventSink,
    ) {
        if !transport_playing || !self.playing {
            if self.active {
                self.pending.flush(window.from(), out);
                self.active = false;
            }
            return;
        }

        // A jump, a loop wrap, or the first block after starting: nothing
        // that was sounding belongs to where we are now.
        if !window.is_continuous() && self.active {
            self.pending.flush(window.from(), out);
        }
        self.active = true;

        let mut cursor = window.from();
        for _ in 0..MAX_SEGMENTS {
            if cursor >= window.to() {
                break;
            }
            let (slot, origin, boundary) = self.locate(cursor);

            // A switch that is due at the cursor itself — an immediate one,
            // or a pattern end that this callback happens to start on — takes
            // effect before anything is generated, so the first note of the
            // new pattern is the first note of the segment.
            if boundary <= cursor {
                self.pending.flush(cursor, out);
                self.switch_at(cursor);
                continue;
            }

            self.live = slot;
            let end = boundary.min(window.to());
            let block = self.slots[slot as usize];
            generate(&block, origin, cursor, end, &mut self.pending, out);

            // A boundary exactly at the end of the window belongs to the next
            // callback, which starts there: taking it here would put its
            // note-offs a whole block early.
            if boundary < window.to() {
                self.pending.flush(boundary, out);
                self.switch_at(boundary);
            }
            cursor = end;
        }

        let block = &self.slots[self.live as usize];
        let origin = self.chain_at(window.from()).map_or(0, |(_, start, _)| start);
        self.step = block.step_at(origin, window.from()) as u8;
    }

    /// Take the queued slot at a boundary that has just passed.
    ///
    /// A chain moves on by itself — its slot is a function of the position —
    /// so a chained track has nothing to take here.
    fn switch_at(&mut self, boundary: i64) {
        if self.chain_at(boundary).is_some() {
            return;
        }
        if let Some(slot) = self.pending_slot.take() {
            self.live = (slot as usize).min(SLOTS - 1) as u8;
        }
    }
}

impl Default for PatternPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Fixtures ──

    /// A pattern with one drum lane, every step on, at a sixteenth.
    fn drum_pattern(steps: u8) -> PatternBlock {
        let mut block = PatternBlock::empty();
        block.steps = steps;
        block.playing = true;
        block.lanes[0] = Lane::drum(36);
        for step in &mut block.lanes[0].steps {
            step.on = true;
        }
        block
    }

    /// A pattern with one melodic lane; only the steps named are on.
    fn melodic_pattern(on: &[usize]) -> PatternBlock {
        let mut block = PatternBlock::empty();
        block.playing = true;
        for &index in on {
            block.lanes[0].steps[index].on = true;
        }
        block
    }

    fn onsets(events: &[PatternEvent]) -> Vec<i64> {
        events.iter().filter(|e| e.is_note_on()).map(|e| e.tick).collect()
    }

    fn run(block: &PatternBlock, from: i64, to: i64) -> Vec<PatternEvent> {
        let mut out = Vec::new();
        let mut pending = PendingOffs::new();
        generate(block, 0, from, to, &mut pending, &mut out);
        out
    }

    // ── Sizes ──

    /// The block crosses to the audio thread by value, so its size is the
    /// cost of every queued `SetPattern`. Pinned here because a field that
    /// creeps in is a cost nobody measures at the time.
    #[test]
    fn the_block_is_the_size_it_is_supposed_to_be() {
        assert_eq!(std::mem::size_of::<Step>(), 9);
        assert_eq!(std::mem::size_of::<Lane>(), 3 + 32 * 9);
        assert_eq!(std::mem::size_of::<PatternBlock>(), PatternBlock::SIZE);
        assert_eq!(
            PatternBlock::SIZE, 2_373,
            "a pattern changed size; every queued SetPattern costs this many bytes"
        );
        // No padding anywhere: every field is a byte-aligned scalar, which is
        // what makes the whole thing a memcpy.
        assert_eq!(std::mem::align_of::<PatternBlock>(), 1);
    }

    // ── Rates and swing ──

    /// The rate table at 960 PPQ. Every division is exact, triplets
    /// included — which is the reason the project is at 960 and not 480.
    #[test]
    fn rate_ticks_are_the_960_ppq_table() {
        assert_eq!(Rate::Quarter.ticks(), 960);
        assert_eq!(Rate::Eighth.ticks(), 480);
        assert_eq!(Rate::Sixteenth.ticks(), 240);
        assert_eq!(Rate::ThirtySecond.ticks(), 120);
        assert_eq!(Rate::EighthTriplet.ticks(), 320);
        assert_eq!(Rate::SixteenthTriplet.ticks(), 160);
        // Three triplets fill the division they subdivide.
        assert_eq!(Rate::EighthTriplet.ticks() * 3, Rate::Quarter.ticks());
        assert_eq!(Rate::SixteenthTriplet.ticks() * 3, Rate::Eighth.ticks());
    }

    #[test]
    fn straight_swing_moves_nothing() {
        let block = drum_pattern(16);
        assert_eq!(block.swing, PatternBlock::MIN_SWING);
        for step in 0..16 {
            assert_eq!(block.swing_offset(step), 0);
        }
    }

    /// MPC swing: the percentage is where the off-beat falls inside the pair,
    /// so 75% is a triplet feel and the offset is exactly half a step.
    #[test]
    fn full_swing_is_a_triplet_feel() {
        let mut block = drum_pattern(16);
        block.swing = 75;
        assert_eq!(block.swing_offset(0), 0);
        assert_eq!(block.swing_offset(1), block.ticks_per_step() / 2);
        assert_eq!(block.swing_offset(2), 0);
        assert_eq!(block.swing_offset(15), block.ticks_per_step() / 2);
    }

    /// Integer arithmetic, so the number is the same every time it is asked
    /// for — which is what makes the bounce and the live player agree without
    /// a tolerance.
    #[test]
    fn swing_is_exact_integer_ticks() {
        let mut block = drum_pattern(16);
        block.swing = 62;
        assert_eq!(block.swing_offset(1), 57); // (62-50) * 2 * 240 / 100
        block.rate = Rate::Eighth;
        assert_eq!(block.swing_offset(1), 115); // ...and 480
    }

    /// Swing never reaches the following step, so the onsets stay in order
    /// however far it is pushed.
    #[test]
    fn swing_never_reorders_the_steps() {
        for swing in PatternBlock::MIN_SWING..=PatternBlock::MAX_SWING {
            let mut block = drum_pattern(16);
            block.swing = swing;
            let mut previous = i64::MIN;
            for index in 0..32 {
                let onset = block.onset(0, index);
                assert!(onset > previous, "swing {swing} reordered step {index}");
                previous = onset;
            }
        }
    }

    // ── Position derivation ──

    /// The clip invariant: what fires depends on where the transport is, not
    /// on how it got there. Starting inside a pattern plays the steps that
    /// are left, not the pattern from the top.
    #[test]
    fn starting_mid_pattern_fires_only_the_remaining_onsets() {
        let block = drum_pattern(16);
        let cycle = block.length_ticks();
        assert_eq!(cycle, 3840);

        let whole = onsets(&run(&block, 0, cycle));
        assert_eq!(whole.len(), 16);
        assert_eq!(whole[0], 0);

        let late = onsets(&run(&block, 1200, cycle));
        assert_eq!(late.len(), 11, "steps 5..=15 remain");
        assert_eq!(late[0], 1200);
        assert_eq!(late, whole[5..]);
    }

    /// The step under the playhead is arithmetic on the position. Bar 5 of a
    /// 16-step pattern is the top of the pattern again.
    #[test]
    fn the_step_is_a_function_of_the_position() {
        let block = drum_pattern(16);
        assert_eq!(block.step_at(0, 0), 0);
        assert_eq!(block.step_at(0, 239), 0);
        assert_eq!(block.step_at(0, 240), 1);
        assert_eq!(block.step_at(0, 3840), 0);
        assert_eq!(block.step_at(0, 3840 * 4 + 720), 3);
    }

    /// 12 and 24 exist so that a pattern can be deliberately out of phase
    /// with the bar. A 12-step sixteenth pattern is three beats long, so it
    /// walks one beat per bar and comes home on the fourth.
    #[test]
    fn a_twelve_step_pattern_drifts_against_the_bar() {
        let block = drum_pattern(12);
        let bar = 3840;
        assert_eq!(block.length_ticks(), 2880);
        assert_eq!(block.step_at(0, 0), 0);
        assert_eq!(block.step_at(0, bar), 4);
        assert_eq!(block.step_at(0, bar * 2), 8);
        assert_eq!(block.step_at(0, bar * 3), 0, "back in phase after three bars");
    }

    /// Shortening a pattern hides the tail; it does not erase it.
    #[test]
    fn a_shorter_pattern_masks_rather_than_truncates() {
        let mut block = drum_pattern(32);
        assert_eq!(onsets(&run(&block, 0, block.length_ticks())).len(), 32);

        block.steps = 16;
        let short = run(&block, 0, block.length_ticks());
        assert_eq!(onsets(&short).len(), 16);

        block.steps = 32;
        assert_eq!(
            onsets(&run(&block, 0, block.length_ticks())).len(),
            32,
            "the steps past 16 were cleared rather than masked"
        );
    }

    /// Contiguous windows tile a cycle exactly once — no onset falls in a
    /// crack and none is seen twice.
    #[test]
    fn tiling_a_cycle_with_windows_fires_every_step_once() {
        let block = drum_pattern(16);
        let cycle = block.length_ticks();
        for span in [1, 7, 240, 241, 1000] {
            let mut all = Vec::new();
            let mut pending = PendingOffs::new();
            let mut from = 0;
            while from < cycle {
                let to = (from + span).min(cycle);
                generate(&block, 0, from, to, &mut pending, &mut all);
                from = to;
            }
            assert_eq!(
                onsets(&all).len(),
                16,
                "span {span} produced the wrong number of onsets"
            );
        }
    }

    // ── Gates and note-offs ──

    #[test]
    fn a_gate_is_a_percentage_of_the_step() {
        let step = Step { gate: 50, ..Step::silent() };
        assert_eq!(step.gate_ticks(240), Some(120));
        let step = Step { gate: 200, ..Step::silent() };
        assert_eq!(step.gate_ticks(240), Some(480));
        // Out of range clamps rather than producing a note of no length.
        let step = Step { gate: 0, ..Step::silent() };
        assert_eq!(step.gate_ticks(240), Some(12));
        let step = Step { gate: Step::TIE, ..Step::silent() };
        assert_eq!(step.gate_ticks(240), None, "a tie has no due tick");
    }

    #[test]
    fn every_note_gets_an_off() {
        let block = drum_pattern(16);
        let events = run(&block, 0, block.length_ticks() + 240);
        let ons = events.iter().filter(|e| e.is_note_on()).count();
        let offs = events.iter().filter(|e| e.status == 0x80).count();
        assert_eq!(ons, 17);
        assert_eq!(offs, 17, "a note was left sounding");
    }

    /// A tie holds until the lane fires again, and the off it produces is at
    /// the next onset rather than at a gate length.
    #[test]
    fn a_tie_holds_to_the_next_onset() {
        let mut block = melodic_pattern(&[0, 4]);
        block.lanes[0].steps[0].gate = Step::TIE;
        let events = run(&block, 0, block.length_ticks());

        let offs: Vec<i64> = events.iter().filter(|e| e.status == 0x80).map(|e| e.tick).collect();
        assert_eq!(offs[0], 960, "the tie ended somewhere other than step 4");

        // ...and the off comes before the note-on it makes room for.
        let at_960: Vec<u8> = events.iter().filter(|e| e.tick == 960).map(|e| e.status).collect();
        assert_eq!(at_960, vec![0x80, 0x90], "the off has to be pushed first");
    }

    /// A gate longer than the step does not run over the lane's own next hit:
    /// the retrigger cuts it, and the cut arrives before the new note.
    #[test]
    fn a_long_gate_is_cut_by_the_next_onset() {
        let mut block = melodic_pattern(&[0, 1]);
        block.lanes[0].steps[0].gate = 200;
        let events = run(&block, 0, 960);
        let at_240: Vec<u8> = events.iter().filter(|e| e.tick == 240).map(|e| e.status).collect();
        assert_eq!(at_240, vec![0x80, 0x90]);
    }

    /// The table holds thirty-two notes, and the thirty-third forces off the
    /// oldest rather than being dropped. A dropped note-on would be silence;
    /// a dropped note-*off* is a voice that never stops.
    #[test]
    fn the_pending_table_forces_off_the_oldest_on_overflow() {
        let mut pending = PendingOffs::new();
        let mut out = Vec::new();
        for i in 0..MAX_PENDING_OFFS {
            pending.hold(0, 40 + i as u8, None, 0, &mut out);
        }
        assert_eq!(pending.len(), MAX_PENDING_OFFS);
        assert!(out.is_empty());

        pending.hold(1, 99, None, 100, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].data1, 40, "the oldest note was not the one forced off");
        assert_eq!(out[0].tick, 100);
        assert_eq!(pending.len(), MAX_PENDING_OFFS);
    }

    #[test]
    fn a_flush_ends_everything_at_one_tick() {
        let mut pending = PendingOffs::new();
        let mut out = Vec::new();
        pending.hold(0, 60, Some(500), 0, &mut out);
        pending.hold(1, 64, None, 0, &mut out);
        pending.flush(300, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|e| e.tick == 300 && e.status == 0x80));
        assert!(pending.is_empty());
    }

    // ── Mute and solo ──

    #[test]
    fn a_muted_lane_is_silent_and_a_soloed_one_is_the_only_one() {
        let mut block = drum_pattern(16);
        block.lanes[1] = Lane::drum(42);
        for step in &mut block.lanes[1].steps {
            step.on = true;
        }
        assert_eq!(onsets(&run(&block, 0, 240)).len(), 2);

        block.lanes[1].muted = true;
        assert_eq!(onsets(&run(&block, 0, 240)).len(), 1);

        block.lanes[1].muted = false;
        block.lanes[1].soloed = true;
        let solo = run(&block, 0, 240);
        assert_eq!(onsets(&solo).len(), 1);
        assert_eq!(solo[0].data1, 42);
    }

    // ── Velocity ──

    #[test]
    fn accent_picks_the_patterns_accent_velocity() {
        let mut block = melodic_pattern(&[0, 1]);
        block.lanes[0].steps[1].accent = true;
        let events = run(&block, 0, 480);
        let ons: Vec<u8> = events.iter().filter(|e| e.is_note_on()).map(|e| e.data2).collect();
        assert_eq!(ons, vec![100, 127]);
    }

    // ── Chords ──

    fn notes_of(root: u8, chord: Chord, voicing: Voicing, below: bool, mode: Mode) -> Vec<u8> {
        let mut out = [0u8; MAX_CHORD_NOTES];
        let n = chord_notes(root, chord, voicing, below, mode, 0, &mut out);
        out[..n].to_vec()
    }

    #[test]
    fn the_chord_table_is_the_shapes_it_names() {
        assert_eq!(notes_of(60, Chord::None, Voicing::Close, false, Mode::Chromatic), vec![60]);
        assert_eq!(notes_of(60, Chord::Fifth, Voicing::Close, false, Mode::Chromatic), vec![60, 67]);
        assert_eq!(notes_of(60, Chord::Octave, Voicing::Close, false, Mode::Chromatic), vec![60, 72]);
        assert_eq!(notes_of(60, Chord::Maj, Voicing::Close, false, Mode::Chromatic), vec![60, 64, 67]);
        assert_eq!(notes_of(60, Chord::Min, Voicing::Close, false, Mode::Chromatic), vec![60, 63, 67]);
        assert_eq!(notes_of(60, Chord::Dim, Voicing::Close, false, Mode::Chromatic), vec![60, 63, 66]);
        assert_eq!(notes_of(60, Chord::Sus2, Voicing::Close, false, Mode::Chromatic), vec![60, 62, 67]);
        assert_eq!(notes_of(60, Chord::Sus4, Voicing::Close, false, Mode::Chromatic), vec![60, 65, 67]);
        assert_eq!(notes_of(60, Chord::Maj6, Voicing::Close, false, Mode::Chromatic), vec![60, 64, 67, 69]);
        assert_eq!(notes_of(60, Chord::Min6, Voicing::Close, false, Mode::Chromatic), vec![60, 63, 67, 69]);
        assert_eq!(notes_of(60, Chord::Dom7, Voicing::Close, false, Mode::Chromatic), vec![60, 64, 67, 70]);
        assert_eq!(notes_of(60, Chord::Min7, Voicing::Close, false, Mode::Chromatic), vec![60, 63, 67, 70]);
        assert_eq!(notes_of(60, Chord::Maj7, Voicing::Close, false, Mode::Chromatic), vec![60, 64, 67, 71]);
        assert_eq!(notes_of(60, Chord::Quartal, Voicing::Close, false, Mode::Chromatic), vec![60, 65, 70]);
    }

    /// The identities a step stores. Appending to this list is allowed;
    /// moving anything already in it rewrites every pattern ever saved.
    #[test]
    fn chord_identities_are_the_documented_order() {
        let order = [
            Chord::None, Chord::Fifth, Chord::Octave, Chord::Diatonic, Chord::Diatonic7,
            Chord::Maj, Chord::Min, Chord::Dim, Chord::Sus2, Chord::Sus4, Chord::Maj6,
            Chord::Min6, Chord::Dom7, Chord::Min7, Chord::Maj7, Chord::Quartal,
        ];
        for (index, chord) in order.iter().enumerate() {
            assert_eq!(chord.index() as usize, index);
            assert_eq!(Chord::from_index(index as u8), *chord);
        }
        assert_eq!(Chord::from_index(200), Chord::None, "an unknown id is one note");
    }

    /// Drop-2 is the second voice from the top, down an octave — not the
    /// middle note, and on a four-note chord not the second note either.
    #[test]
    fn drop_two_lowers_the_second_voice_from_the_top() {
        // C E G -> E an octave down, under the root.
        assert_eq!(
            notes_of(60, Chord::Maj, Voicing::Drop2, false, Mode::Chromatic),
            vec![52, 60, 67]
        );
        // C E G B -> G an octave down.
        assert_eq!(
            notes_of(60, Chord::Maj7, Voicing::Drop2, false, Mode::Chromatic),
            vec![55, 60, 64, 71]
        );
    }

    #[test]
    fn inversions_lift_the_bottom_voices() {
        assert_eq!(
            notes_of(60, Chord::Maj, Voicing::First, false, Mode::Chromatic),
            vec![64, 67, 72]
        );
        assert_eq!(
            notes_of(60, Chord::Maj, Voicing::Second, false, Mode::Chromatic),
            vec![67, 72, 76]
        );
    }

    #[test]
    fn root_below_adds_the_bass_double() {
        assert_eq!(
            notes_of(60, Chord::Maj, Voicing::Close, true, Mode::Chromatic),
            vec![48, 60, 64, 67]
        );
    }

    /// Every combination in the table, checked for the three things that
    /// would make a chord unplayable: a note outside MIDI, the same note
    /// twice — which leaves the child holding a voice nothing turns off —
    /// and an empty chord.
    #[test]
    fn every_chord_and_voicing_is_playable() {
        for &chord in &Chord::ALL {
            for &voicing in &Voicing::ALL {
                for below in [false, true] {
                    for &mode in &Mode::ALL {
                        for root in 24..=96u8 {
                            let notes = notes_of(root, chord, voicing, below, mode);
                            assert!(!notes.is_empty(), "{chord:?} produced nothing");
                            assert!(notes.len() <= MAX_CHORD_NOTES);
                            let mut seen = notes.clone();
                            seen.dedup();
                            assert_eq!(seen, notes, "{chord:?}/{voicing:?} doubled a note");
                            for window in notes.windows(2) {
                                assert!(window[0] < window[1], "not ascending");
                            }
                        }
                    }
                }
            }
        }
    }

    /// A voicing rearranges a chord; it does not change it. The pitch-class
    /// set is what "the same chord" means, and it is invariant under every
    /// voicing — root-below excepted, which is a deliberate duplicate of one
    /// class an octave down and so leaves the *set* alone as well.
    #[test]
    fn voicings_preserve_the_pitch_class_set() {
        for &chord in &Chord::ALL {
            for &mode in &Mode::ALL {
                for root in 36..=84u8 {
                    let classes = |notes: Vec<u8>| {
                        let mut c: Vec<u8> = notes.iter().map(|n| n % 12).collect();
                        c.sort_unstable();
                        c.dedup();
                        c
                    };
                    let close = classes(notes_of(root, chord, Voicing::Close, false, mode));
                    for &voicing in &Voicing::ALL {
                        for below in [false, true] {
                            assert_eq!(
                                classes(notes_of(root, chord, voicing, below, mode)),
                                close,
                                "{chord:?} changed identity under {voicing:?} below={below}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The textbook qualities, in every mode. This is the table the whole
    /// diatonic idea rests on: if the third degree of Dorian is not minor,
    /// a line of diatonic chords is not in a key at all.
    #[test]
    fn diatonic_triads_have_the_textbook_qualities_in_every_mode() {
        let expected: [(Mode, [Chord; 7]); 7] = [
            (Mode::Ionian, [Chord::Maj, Chord::Min, Chord::Min, Chord::Maj, Chord::Maj, Chord::Min, Chord::Dim]),
            (Mode::Dorian, [Chord::Min, Chord::Min, Chord::Maj, Chord::Maj, Chord::Min, Chord::Dim, Chord::Maj]),
            (Mode::Phrygian, [Chord::Min, Chord::Maj, Chord::Maj, Chord::Min, Chord::Dim, Chord::Maj, Chord::Min]),
            (Mode::Lydian, [Chord::Maj, Chord::Maj, Chord::Min, Chord::Dim, Chord::Maj, Chord::Min, Chord::Min]),
            (Mode::Mixolydian, [Chord::Maj, Chord::Min, Chord::Dim, Chord::Maj, Chord::Min, Chord::Min, Chord::Maj]),
            (Mode::Aeolian, [Chord::Min, Chord::Dim, Chord::Maj, Chord::Min, Chord::Min, Chord::Maj, Chord::Maj]),
            (Mode::Locrian, [Chord::Dim, Chord::Maj, Chord::Min, Chord::Min, Chord::Maj, Chord::Maj, Chord::Min]),
        ];

        for tonic in 0..12u8 {
            for (mode, qualities) in &expected {
                let scale = mode.scale().expect("a mode has a scale");
                for (degree, &quality) in qualities.iter().enumerate() {
                    let root = 60 + i32::from(tonic) + scale[degree];
                    let root = root as u8;
                    let mut derived = [0u8; MAX_CHORD_NOTES];
                    let n = chord_notes(
                        root, Chord::Diatonic, Voicing::Close, false, *mode, tonic, &mut derived,
                    );
                    let mut explicit = [0u8; MAX_CHORD_NOTES];
                    let m = chord_notes(
                        root, quality, Voicing::Close, false, *mode, tonic, &mut explicit,
                    );
                    assert_eq!(
                        derived[..n],
                        explicit[..m],
                        "{mode:?} degree {} in tonic {tonic} should be {quality:?}",
                        degree + 1
                    );
                }
            }
        }
    }

    /// The seventh degree of a major scale is half-diminished, which is not
    /// one of the sixteen chord types a step can name — and does not have to
    /// be, because `diatonic7` produces intervals rather than picking a type.
    #[test]
    fn the_seventh_degree_is_half_diminished() {
        let mut out = [0u8; MAX_CHORD_NOTES];
        let n = chord_notes(71, Chord::Diatonic7, Voicing::Close, false, Mode::Ionian, 0, &mut out);
        assert_eq!(&out[..n], &[71, 74, 77, 81], "B D F A is not m7♭5");
    }

    /// Under Chromatic there is no degree to derive a quality from, so the
    /// two diatonic entries collapse — documented behaviour, not a fallback
    /// nobody meant.
    #[test]
    fn the_diatonic_chords_collapse_under_chromatic() {
        assert_eq!(
            notes_of(60, Chord::Diatonic, Voicing::Close, false, Mode::Chromatic),
            notes_of(60, Chord::Maj, Voicing::Close, false, Mode::Chromatic)
        );
        assert_eq!(
            notes_of(60, Chord::Diatonic7, Voicing::Close, false, Mode::Chromatic),
            notes_of(60, Chord::Maj7, Voicing::Close, false, Mode::Chromatic)
        );
    }

    /// A note the mode does not contain is a borrowed note, and a diatonic
    /// chord on one has no derived quality — it falls back to major rather
    /// than refusing to sound.
    #[test]
    fn a_borrowed_root_falls_back_to_major() {
        assert_eq!(Mode::Ionian.degree_of(61, 0), None);
        assert_eq!(
            notes_of(61, Chord::Diatonic, Voicing::Close, false, Mode::Ionian),
            vec![61, 65, 68]
        );
    }

    // ── Mode walking ──

    #[test]
    fn chromatic_walking_is_semitones() {
        assert_eq!(Mode::Chromatic.walk(60, 0, 1), 61);
        assert_eq!(Mode::Chromatic.walk(60, 0, -1), 59);
        assert_eq!(Mode::Chromatic.walk(0, 0, -1), 0, "the bottom of the range holds");
        assert_eq!(Mode::Chromatic.walk(127, 0, 1), 127);
    }

    #[test]
    fn mode_walking_is_scale_degrees() {
        // C major, up the scale and back down through the octave below.
        let mut note = 60;
        for expected in [62, 64, 65, 67, 69, 71, 72, 74] {
            note = Mode::Ionian.walk(note, 0, 1);
            assert_eq!(note, expected);
        }
        let mut note = 60;
        for expected in [59, 57, 55, 53, 52, 50, 48] {
            note = Mode::Ionian.walk(note, 0, -1);
            assert_eq!(note, expected);
        }
    }

    /// A note off the scale — set before the mode was, or borrowed on
    /// purpose — snaps onto it on the first press rather than walking off it
    /// forever.
    #[test]
    fn walking_snaps_a_borrowed_note_onto_the_scale() {
        assert_eq!(Mode::Ionian.walk(61, 0, 1), 62, "C# up lands on D");
        assert_eq!(Mode::Ionian.walk(61, 0, -1), 60, "C# down lands on C");
    }

    #[test]
    fn every_mode_walks_a_full_octave_in_seven_degrees() {
        for &mode in &Mode::ALL {
            if mode == Mode::Chromatic {
                continue;
            }
            for tonic in 0..12u8 {
                let start = 60 + tonic;
                let start = mode.walk(start, tonic, 0);
                let mut note = start;
                for _ in 0..7 {
                    note = mode.walk(note, tonic, 1);
                }
                assert_eq!(note, start + 12, "{mode:?} in {tonic} did not close");
            }
        }
    }

    // ── Switch quantization ──

    #[test]
    fn switch_boundaries_are_the_next_grid_line() {
        let pattern = 3840;
        assert_eq!(SwitchQuant::Immediate.boundary(1234, pattern), 1234);
        assert_eq!(SwitchQuant::Beat.boundary(1234, pattern), 1920);
        assert_eq!(SwitchQuant::Bar.boundary(1234, pattern), 3840);
        assert_eq!(SwitchQuant::PatternEnd.boundary(1234, pattern), 3840);
        assert_eq!(SwitchQuant::PatternEnd.boundary(4000, 2880), 5760);
    }

    /// A queue made exactly on the boundary takes that boundary, not the
    /// next one — otherwise a switch queued on the downbeat waits a whole
    /// extra bar.
    #[test]
    fn a_boundary_already_reached_is_the_answer() {
        assert_eq!(SwitchQuant::Bar.boundary(3840, 3840), 3840);
        assert_eq!(SwitchQuant::Beat.boundary(960, 3840), 960);
        assert_eq!(SwitchQuant::PatternEnd.boundary(0, 3840), 0);
    }

    // ── The window ──

    /// 120 BPM, 44.1 kHz.
    const TPS: f64 = 120.0 * 960.0 / (60.0 * 44_100.0);

    fn window(position: i64, frames: u32, previous: Option<PlaybackWindow>) -> PlaybackWindow {
        PlaybackWindow::for_block(position, frames, TPS, None, previous)
    }

    #[test]
    fn the_first_window_starts_where_the_transport_is() {
        let w = window(1000, 512, None);
        assert_eq!(w.from(), 1000);
        assert!(!w.is_continuous(), "there is nothing for it to continue from");
    }

    /// The gap the contiguity rule exists to close: a transport that carries
    /// the sub-tick remainder advances further than the block measured, and
    /// the tick in between belongs to somebody.
    #[test]
    fn a_window_continues_from_the_last_one_across_a_rounding_gap() {
        let first = window(0, 470, None);
        let span = first.to();
        // The transport landed one tick past where the block measured.
        let second = window(span + 1, 470, Some(first));
        assert_eq!(second.from(), span, "a tick of song time was skipped");
        assert!(second.is_continuous());
        assert_eq!(second.to(), span + 1 + span);
    }

    /// Moving the playhead is not continuous playback, and nothing gets
    /// replayed to cover the jump.
    #[test]
    fn a_jump_breaks_continuity() {
        let first = window(0, 512, None);
        let jumped = window(100_000, 512, Some(first));
        assert_eq!(jumped.from(), 100_000);
        assert!(!jumped.is_continuous());
    }

    /// A loop wrap starts the window at the loop point, so the ticks between
    /// the loop point and where the callback arrived are played rather than
    /// skipped. This is what clip playback has always done.
    #[test]
    fn a_loop_wrap_starts_the_window_at_the_loop_point() {
        let previous = PlaybackWindow::for_block(3800, 512, TPS, Some((0, 3840)), None);
        let wrapped = PlaybackWindow::for_block(3, 512, TPS, Some((0, 3840)), Some(previous));
        assert_eq!(wrapped.from(), 0);
        assert!(!wrapped.is_continuous());
    }

    /// The window stops at the loop point. Reaching across it would play the
    /// notes on the other side, and then the wrap would play them again.
    #[test]
    fn a_window_never_reaches_past_the_loop_end() {
        let w = PlaybackWindow::for_block(3830, 4096, TPS, Some((0, 3840)), None);
        assert_eq!(w.to(), 3840);
        assert!(!w.contains(3840));
    }

    /// The one expression that turns song time into a sample position. Both
    /// clips and patterns ask it, which is the whole point.
    #[test]
    fn sample_offsets_come_from_ticks_and_nothing_else() {
        let w = window(1000, 512, None);
        assert_eq!(w.sample_offset(1000), 0);
        assert_eq!(w.sample_offset(999), 0, "before the window is the first sample");
        assert_eq!(w.sample_offset(1000 + 22), (22.0 / TPS) as u32);
        assert_eq!(w.sample_offset(i64::MAX), 511, "past the block is the last sample");
    }

    #[test]
    fn a_zero_length_block_has_no_samples_to_land_on() {
        let w = window(0, 0, None);
        assert_eq!(w.sample_offset(1000), 0);
    }

    // ── The player ──

    fn player_with(slot0: PatternBlock, slot1: PatternBlock) -> PatternPlayer {
        let mut player = PatternPlayer::new();
        player.apply(1, slot1);
        player.apply(0, slot0);
        player
    }

    /// Run a player over consecutive callbacks, the way the mixer does:
    /// each window continues from the last, and the transport position is
    /// wherever the previous window ended.
    fn run_player(
        player: &mut PatternPlayer,
        start: i64,
        frames: u32,
        until: i64,
    ) -> Vec<PatternEvent> {
        let mut out = Vec::new();
        let mut position = start;
        let mut previous = None;
        while position < until {
            let w = window(position, frames, previous);
            player.render(&w, true, &mut out);
            position = w.to();
            previous = Some(w);
        }
        out
    }

    /// One callback of a running player, and the events it produced.
    fn tick_player(
        player: &mut PatternPlayer,
        position: i64,
        frames: u32,
        previous: Option<PlaybackWindow>,
    ) -> (PlaybackWindow, Vec<PatternEvent>) {
        let w = window(position, frames, previous);
        let mut out = Vec::new();
        player.render(&w, true, &mut out);
        (w, out)
    }

    #[test]
    fn a_stopped_transport_produces_nothing_and_then_flushes_once() {
        let mut player = player_with(drum_pattern(16), PatternBlock::empty());
        let (w, events) = tick_player(&mut player, 0, 512, None);
        assert!(!events.is_empty());
        assert!(player.held_notes() > 0);

        let mut out = Vec::new();
        player.render(&w, false, &mut out);
        assert_eq!(out.len(), 1, "the sounding note was not turned off");
        assert_eq!(out[0].status, 0x80);
        assert_eq!(player.held_notes(), 0);

        let mut again = Vec::new();
        player.render(&w, false, &mut again);
        assert!(again.is_empty(), "the flush repeated");
    }

    /// The switch: at the boundary, everything sounding is turned off, and
    /// those offs are pushed before the new pattern's first notes.
    #[test]
    fn a_pattern_switch_ends_the_old_notes_before_starting_the_new_ones() {
        let mut a = drum_pattern(16);
        a.lanes[0].steps[15].gate = Step::TIE;
        let mut b = drum_pattern(16);
        b.lanes[0] = Lane::drum(42);
        for step in &mut b.lanes[0].steps {
            step.on = true;
        }

        let mut player = player_with(a, b);
        // Queue slot 1 for the end of the pattern.
        let mut queue = a;
        queue.pending_slot = Some(1);
        player.apply(0, queue);
        assert_eq!(player.countdown(3600), Some((1, 1)), "one step to go");

        // Run into the boundary from before the tied step, so that the note
        // the switch has to end is actually sounding when it arrives.
        let out = run_player(&mut player, 3500, 512, 3900);

        let at_boundary: Vec<(u8, u8)> = out
            .iter()
            .filter(|e| e.tick == 3840)
            .map(|e| (e.status, e.data1))
            .collect();
        assert_eq!(
            at_boundary,
            vec![(0x80, 36), (0x90, 42)],
            "the old note has to be ended before the new one starts"
        );
        assert_eq!(player.live_slot(), 1);
        assert_eq!(player.queued_slot(), None);
    }

    /// An immediate switch happens at the top of the next callback, not one
    /// tick into it: the boundary is the cursor itself, and the first note of
    /// the new pattern is the first note of the block.
    #[test]
    fn an_immediate_switch_takes_effect_at_the_start_of_the_block() {
        let a = drum_pattern(16);
        let mut b = drum_pattern(16);
        b.lanes[0] = Lane::drum(42);
        for step in &mut b.lanes[0].steps {
            step.on = true;
        }

        let mut player = player_with(a, b);
        let mut queued = a;
        queued.pending_slot = Some(1);
        queued.switch_quant = SwitchQuant::Immediate;
        player.apply(0, queued);

        // A block starting exactly on a step, so the switch and an onset land
        // on the same tick.
        let w = window(480, 512, None);
        let mut out = Vec::new();
        player.render(&w, true, &mut out);
        assert_eq!(player.live_slot(), 1);
        let first = out.iter().find(|e| e.is_note_on()).expect("a note");
        assert_eq!(first.data1, 42, "the old pattern played after an immediate switch");
        assert_eq!(first.tick, 480);
    }

    /// A switch quantized to the beat lands on the beat, in the middle of the
    /// callback that contains it, not at either end of it.
    #[test]
    fn a_beat_quantized_switch_splits_the_block_at_the_beat() {
        let a = drum_pattern(16);
        let mut b = drum_pattern(16);
        b.lanes[0] = Lane::drum(42);
        for step in &mut b.lanes[0].steps {
            step.on = true;
        }

        let mut player = player_with(a, b);
        let mut queued = a;
        queued.pending_slot = Some(1);
        queued.switch_quant = SwitchQuant::Beat;
        player.apply(0, queued);

        let out = run_player(&mut player, 700, 512, 1100);
        let switched: Vec<(i64, u8)> = out
            .iter()
            .filter(|e| e.is_note_on())
            .map(|e| (e.tick, e.data1))
            .collect();
        assert_eq!(
            switched,
            vec![(720, 36), (960, 42)],
            "the switch did not land on the beat"
        );
        assert_eq!(player.live_slot(), 1);
    }

    /// Queueing the slot that is already playing is not a switch — which is
    /// what makes a stale queue on the UI side harmless rather than a note
    /// cut nobody asked for.
    #[test]
    fn queueing_the_live_slot_does_nothing() {
        let mut block = drum_pattern(16);
        block.pending_slot = Some(0);
        let player = player_with(block, PatternBlock::empty());
        assert_eq!(player.queued_slot(), None);
        assert_eq!(player.countdown(0), None);
    }

    /// A chain is a program the position indexes into, not a cursor: dropping
    /// the playhead into the middle of one lands in the entry that belongs
    /// there, exactly as a step does.
    #[test]
    fn a_chain_is_derived_from_the_position() {
        let a = drum_pattern(16);
        let mut b = drum_pattern(16);
        b.lanes[0] = Lane::drum(42);
        for step in &mut b.lanes[0].steps {
            step.on = true;
        }

        let mut chained = a;
        chained.chain[0] = ChainEntry { slot: 0, repeats: 2 };
        chained.chain[1] = ChainEntry { slot: 1, repeats: 1 };
        chained.chain_len = 2;

        let mut player = PatternPlayer::new();
        player.apply(1, b);
        player.apply(0, chained);

        let cycle = 3840;
        // Two times through A, then one of B, then round again.
        for (position, expected) in [
            (0, 36),
            (cycle, 36),
            (cycle * 2, 42),
            (cycle * 3, 36),
            (cycle * 5, 42),
        ] {
            let mut out = Vec::new();
            let w = window(position, 512, None);
            player.render(&w, true, &mut out);
            let first = out.iter().find(|e| e.is_note_on()).expect("a note");
            assert_eq!(first.data1, expected, "wrong chain entry at tick {position}");
        }
    }

    /// The chain boundary is a switch like any other: notes sounding across
    /// it are ended at it.
    #[test]
    fn a_chain_advance_ends_the_notes_it_replaces() {
        let mut a = drum_pattern(16);
        a.lanes[0].steps[15].gate = Step::TIE;
        let mut b = drum_pattern(16);
        b.lanes[0] = Lane::drum(42);
        b.lanes[0].steps[0].on = true;

        let mut chained = a;
        chained.chain[0] = ChainEntry { slot: 0, repeats: 1 };
        chained.chain[1] = ChainEntry { slot: 1, repeats: 1 };
        chained.chain_len = 2;

        let mut player = PatternPlayer::new();
        player.apply(1, b);
        player.apply(0, chained);

        let out = run_player(&mut player, 3500, 512, 3900);
        let at_boundary: Vec<(u8, u8)> = out
            .iter()
            .filter(|e| e.tick == 3840)
            .map(|e| (e.status, e.data1))
            .collect();
        assert_eq!(at_boundary, vec![(0x80, 36), (0x90, 42)]);
    }

    // ── Bounce ──

    /// The bounce is not "the same as" live playback: it is live playback,
    /// run with a different sink. Anything that changed one and not the other
    /// would show up here as a tick that does not match.
    #[test]
    fn a_bounced_cycle_is_tick_identical_to_live_playback() {
        for swing in [50u8, 58, 62, 75] {
            for rate in Rate::ALL {
                let mut block = drum_pattern(16);
                block.rate = rate;
                block.swing = swing;
                block.lanes[0].steps[3].gate = 150;
                block.lanes[0].steps[7].gate = Step::TIE;
                block.lanes[0].steps[9].accent = true;

                let mut bounced = Vec::new();
                compile_cycle(&block, 0, &mut bounced);

                // ...and the same pattern played a block at a time.
                let cycle = block.length_ticks();
                let mut live = Vec::new();
                let mut pending = PendingOffs::new();
                let mut from = 0;
                while from < cycle {
                    let to = (from + 97).min(cycle);
                    generate(&block, 0, from, to, &mut pending, &mut live);
                    from = to;
                }
                pending.flush(cycle, &mut live);
                live.sort_by_key(|e| e.tick);

                let key = |e: &PatternEvent| (e.tick, e.status, e.data1, e.data2);
                let bounced: Vec<_> = bounced.iter().map(key).collect();
                let live: Vec<_> = live.iter().map(key).collect();
                assert_eq!(bounced, live, "swing {swing} at {}", rate.label());
            }
        }
    }

    // ── Allocation ──

    /// The rule the audio thread lives by. Rendering a pattern, switching
    /// one, advancing a chain and taking a new block are all writes into
    /// memory that already exists.
    #[test]
    fn rendering_a_pattern_does_not_allocate() {
        let mut a = drum_pattern(16);
        a.lanes[0].steps[15].gate = Step::TIE;
        let mut b = drum_pattern(16);
        b.lanes[0] = Lane::drum(42);

        let mut player = Box::new(player_with(a, b));
        let mut sink = Vec::with_capacity(1024);
        let mut queued = a;
        queued.pending_slot = Some(1);

        // One warm-up callback outside the measurement.
        let mut w = window(0, 512, None);
        player.render(&w, true, &mut sink);

        let allocations = crate::alloc_count::allocations_during(|| {
            let mut position = 0;
            for block in 0..64 {
                w = window(position, 512, Some(w));
                sink.clear();
                player.render(&w, true, &mut sink);
                if block == 8 {
                    player.apply(0, queued);
                }
                position = w.to();
            }
        });
        assert_eq!(allocations, 0, "the pattern player reached the allocator");
    }

    /// A sink that is out of room stops the generator rather than losing
    /// events out of the middle of a step — half a chord with no offs is a
    /// stuck voice, and a missing note is not.
    #[test]
    fn a_full_sink_stops_the_generator() {
        struct Capped(Vec<PatternEvent>, usize);
        impl EventSink for Capped {
            fn accept(&mut self, event: PatternEvent) -> bool {
                if self.0.len() >= self.1 {
                    return false;
                }
                self.0.push(event);
                true
            }
        }

        let block = drum_pattern(16);
        let mut sink = Capped(Vec::new(), 3);
        let mut pending = PendingOffs::new();
        generate(&block, 0, 0, block.length_ticks(), &mut pending, &mut sink);
        assert_eq!(sink.0.len(), 3, "the sink was written past its cap");
    }
}

