//! The practice room: what "fingers" teaches and how it judges.
//!
//! Everything here is generative. The one in-DAW trainer ever shipped —
//! GarageBand's Learn to Play — died of produced content: hand-made video
//! lessons stopped coming and the feature starved. Scales, arpeggios and
//! jazz drills are *patterns*; this module generates them in any key from
//! fingering tables verified against published pedagogy (UVU charts, the
//! ABRSM ladder, Levine, the Barry Harris workshop books), and the player's
//! own synth sounds them.
//!
//! The judge lives in [`judge`]; saved progress in [`progress`]. This file
//! owns the model and the curriculum.

pub mod judge;
pub mod progress;

use phosphor_core::transport::Transport;

const PPQ: i64 = Transport::PPQ;

/// Which hand a note belongs to — the display splits colors by it, the
/// hands-separate variants filter by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

/// Which hands an exercise run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hands {
    #[default]
    Right,
    Left,
    Together,
}

impl Hands {
    pub const ALL: [Hands; 3] = [Hands::Right, Hands::Left, Hands::Together];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Right => "RH",
            Self::Left => "LH",
            Self::Together => "HT",
        }
    }

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Right => "rh",
            Self::Left => "lh",
            Self::Together => "ht",
        }
    }
}

/// One note the player is asked to play.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetNote {
    /// Position on the exercise timeline, ticks at PPQ 960.
    pub tick: i64,
    pub note: u8,
    /// 1 = thumb .. 5 = pinky; 0 = no fingering prescribed.
    pub finger: u8,
    pub hand: Hand,
    /// Length in ticks, for display and gate feel; judging is onset-based.
    pub duration: i64,
}

/// How the metronome clicks during an exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClickMode {
    Off,
    #[default]
    AllBeats,
    /// Beats 2 and 4 only — the jazz convention. The click is the
    /// drummer's hi-hat; beats 1 and 3 are yours to feel.
    TwoAndFour,
}

impl ClickMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::AllBeats => "1234",
            Self::TwoAndFour => "2&4",
        }
    }

    /// The wire value `MixerCommand::SetPracticeClick` takes.
    #[must_use]
    pub fn pattern(self) -> u8 {
        match self {
            Self::TwoAndFour => 1,
            _ => 0,
        }
    }
}

/// A generated exercise: the notes, and everything the room needs to run
/// and score a rep of it.
#[derive(Debug, Clone)]
pub struct Exercise {
    /// Stable identity for saved progress: family, key, hands.
    pub id: String,
    pub title: String,
    pub level: u8,
    pub start_bpm: u32,
    pub target_bpm: u32,
    pub click: ClickMode,
    pub notes: Vec<TargetNote>,
    /// One rep's length, whole bars, including a breath at the end.
    pub loop_ticks: i64,
}

// ── Fingering data (verified: UVU charts / ABRSM convention) ──

/// Major scale fingerings, one octave ascending, seven degrees plus the
/// top-note turnaround finger. Descending is the exact retrograde.
struct ScaleFingering {
    rh: [u8; 7],
    rh_top: u8,
    lh: [u8; 7],
    lh_top: u8,
}

const STD_RH: [u8; 7] = [1, 2, 3, 1, 2, 3, 4];
const STD_LH: [u8; 7] = [5, 4, 3, 2, 1, 3, 2];
const FLAT_LH: [u8; 7] = [3, 2, 1, 4, 3, 2, 1];

/// Fingering for a major scale rooted on `pc` (pitch class, 0 = C).
fn major_fingering(pc: u8) -> ScaleFingering {
    match pc {
        // C G D A E — the reference pattern.
        0 | 7 | 2 | 9 | 4 => ScaleFingering { rh: STD_RH, rh_top: 5, lh: STD_LH, lh_top: 1 },
        // B: LH starts 4, thumb on E and B.
        11 => ScaleFingering { rh: STD_RH, rh_top: 5, lh: [4, 3, 2, 1, 4, 3, 2], lh_top: 1 },
        // F: RH 4 on Bb, thumb never on a black key.
        5 => ScaleFingering { rh: [1, 2, 3, 4, 1, 2, 3], rh_top: 4, lh: STD_LH, lh_top: 1 },
        // Bb.
        10 => ScaleFingering { rh: [4, 1, 2, 3, 1, 2, 3], rh_top: 4, lh: FLAT_LH, lh_top: 3 },
        // Eb.
        3 => ScaleFingering { rh: [3, 1, 2, 3, 4, 1, 2], rh_top: 3, lh: FLAT_LH, lh_top: 3 },
        // Ab.
        8 => ScaleFingering { rh: [3, 4, 1, 2, 3, 1, 2], rh_top: 3, lh: FLAT_LH, lh_top: 3 },
        // Db.
        1 => ScaleFingering { rh: [2, 3, 1, 2, 3, 4, 1], rh_top: 2, lh: FLAT_LH, lh_top: 3 },
        // Gb/F#.
        _ => ScaleFingering { rh: [2, 3, 4, 1, 2, 3, 1], rh_top: 2, lh: [4, 3, 2, 1, 3, 2, 1], lh_top: 4 },
    }
}

/// Fingering for a harmonic minor scale rooted on `pc`.
fn harmonic_minor_fingering(pc: u8) -> ScaleFingering {
    match pc {
        // A E C G D minor — the reference pattern.
        9 | 4 | 0 | 7 | 2 => ScaleFingering { rh: STD_RH, rh_top: 5, lh: STD_LH, lh_top: 1 },
        // B minor.
        11 => ScaleFingering { rh: STD_RH, rh_top: 5, lh: [4, 3, 2, 1, 4, 3, 2], lh_top: 1 },
        // F# minor.
        6 => ScaleFingering { rh: [3, 4, 1, 2, 3, 1, 2], rh_top: 3, lh: [4, 3, 2, 1, 3, 2, 1], lh_top: 4 },
        // C# / G# minor.
        1 | 8 => ScaleFingering { rh: [3, 4, 1, 2, 3, 1, 2], rh_top: 3, lh: FLAT_LH, lh_top: 3 },
        // Eb minor.
        3 => ScaleFingering { rh: [3, 1, 2, 3, 4, 1, 2], rh_top: 3, lh: [2, 1, 4, 3, 2, 1, 3], lh_top: 2 },
        // Bb minor.
        10 => ScaleFingering { rh: [4, 1, 2, 3, 1, 2, 3], rh_top: 4, lh: [2, 1, 3, 2, 1, 4, 3], lh_top: 2 },
        // F minor.
        _ => ScaleFingering { rh: [1, 2, 3, 4, 1, 2, 3], rh_top: 4, lh: STD_LH, lh_top: 1 },
    }
}

const MAJOR_STEPS: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const HARMONIC_MINOR_STEPS: [i32; 7] = [0, 2, 3, 5, 7, 8, 11];

/// Note names for titles and readouts.
pub const NOTE_NAMES: [&str; 12] =
    ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// The chromatic scale's per-pitch-class fingerings (French standard):
/// 3 on every black key; whites take the thumb except the listed 2s.
fn chromatic_finger(pc: u8, hand: Hand) -> u8 {
    let black = matches!(pc, 1 | 3 | 6 | 8 | 10);
    if black {
        return 3;
    }
    match hand {
        // RH: 2 on F and C.
        Hand::Right => {
            if pc == 5 || pc == 0 {
                2
            } else {
                1
            }
        }
        // LH: 2 on E and B.
        Hand::Left => {
            if pc == 4 || pc == 11 {
                2
            } else {
                1
            }
        }
    }
}

// ── Generators ──

/// A scale run: `octaves` up and back down, eighth notes, both directions,
/// with the verified fingering. `steps` are the scale's semitone offsets;
/// `f` its fingering table.
fn scale_run(
    root: u8,
    steps: &[i32; 7],
    f: &ScaleFingering,
    hands: Hands,
    octaves: u8,
) -> Vec<TargetNote> {
    let octaves = octaves.max(1) as i32;
    let count = 7 * octaves; // ascending notes below the top
    let step_ticks = PPQ / 2; // eighth notes
    // Ascending indices 0..=count, then descending back to 0.
    let mut order: Vec<i32> = (0..=count).collect();
    order.extend((0..count).rev());

    let mut out = Vec::new();
    for (pos, &idx) in order.iter().enumerate() {
        let octave = idx / 7;
        let degree = (idx % 7) as usize;
        let at_top = idx == count;
        let semis = if at_top { 12 * octaves } else { 12 * octave + steps[degree] };
        let tick = pos as i64 * step_ticks;
        let dur = step_ticks - PPQ / 16;
        if matches!(hands, Hands::Right | Hands::Together) {
            let finger = if at_top { f.rh_top } else { f.rh[degree] };
            let note = (i32::from(root) + semis).clamp(0, 127) as u8;
            out.push(TargetNote { tick, note, finger, hand: Hand::Right, duration: dur });
        }
        if matches!(hands, Hands::Left | Hands::Together) {
            let finger = if at_top { f.lh_top } else { f.lh[degree] };
            // LH plays an octave below the RH line.
            let note = (i32::from(root) + semis - 12).clamp(0, 127) as u8;
            out.push(TargetNote { tick, note, finger, hand: Hand::Left, duration: dur });
        }
    }
    out
}

fn bars_for(notes: &[TargetNote]) -> i64 {
    let last = notes.iter().map(|n| n.tick + n.duration).max().unwrap_or(0);
    let bar = PPQ * 4;
    // A breath at the end: round up, then one bar of rest.
    ((last + bar - 1) / bar + 1) * bar
}

/// The exercise families the curriculum lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    MajorScale,
    MinorScale,
    Chromatic,
    Hanon1,
    MajorArpeggio,
    Shell251,
    Rootless251,
    Charleston,
    BebopDominant,
    BarryHarris,
    Enclosure3rds,
}

impl Family {
    pub const ALL: [Family; 11] = [
        Family::MajorScale,
        Family::MinorScale,
        Family::Chromatic,
        Family::Hanon1,
        Family::MajorArpeggio,
        Family::Shell251,
        Family::Rootless251,
        Family::Charleston,
        Family::BebopDominant,
        Family::BarryHarris,
        Family::Enclosure3rds,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::MajorScale => "major scale",
            Self::MinorScale => "harmonic minor scale",
            Self::Chromatic => "chromatic scale",
            Self::Hanon1 => "hanon no. 1",
            Self::MajorArpeggio => "major arpeggio",
            Self::Shell251 => "shell 2-5-1 \u{00b7} all keys",
            Self::Rootless251 => "rootless 2-5-1 \u{00b7} all keys",
            Self::Charleston => "charleston comp",
            Self::BebopDominant => "bebop dominant scale",
            Self::BarryHarris => "6th-diminished chords",
            Self::Enclosure3rds => "enclosures \u{00b7} 3rds",
        }
    }

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::MajorScale => "maj_scale",
            Self::MinorScale => "min_scale",
            Self::Chromatic => "chromatic",
            Self::Hanon1 => "hanon1",
            Self::MajorArpeggio => "arp_maj",
            Self::Shell251 => "shell251",
            Self::Rootless251 => "rootless251",
            Self::Charleston => "charleston",
            Self::BebopDominant => "bebop_dom",
            Self::Enclosure3rds => "encl3",
            Self::BarryHarris => "bh6dim",
        }
    }

    #[must_use]
    pub fn level(self) -> u8 {
        match self {
            Self::MajorScale => 1,
            Self::MinorScale => 2,
            Self::Chromatic => 2,
            Self::Hanon1 => 2,
            Self::MajorArpeggio => 3,
            Self::Shell251 => 3,
            Self::Charleston => 4,
            Self::BebopDominant => 4,
            Self::Rootless251 => 5,
            Self::BarryHarris => 5,
            Self::Enclosure3rds => 5,
        }
    }

    /// Whether the exercise is rooted in a key the player cycles, or is a
    /// full 12-key cycle in itself.
    #[must_use]
    pub fn keyed(self) -> bool {
        !matches!(self, Self::Shell251 | Self::Rootless251 | Self::Enclosure3rds)
    }

    /// Whether the hands variant applies.
    #[must_use]
    pub fn handed(self) -> bool {
        matches!(
            self,
            Self::MajorScale | Self::MinorScale | Self::Chromatic | Self::Hanon1 | Self::MajorArpeggio
        )
    }

    #[must_use]
    pub fn start_bpm(self) -> u32 {
        match self {
            Self::MajorScale | Self::MinorScale => 60,
            Self::Chromatic => 66,
            Self::Hanon1 => 60,
            Self::MajorArpeggio => 63,
            Self::Shell251 | Self::Rootless251 => 60,
            Self::Charleston => 70,
            Self::BebopDominant => 70,
            Self::BarryHarris => 60,
            Self::Enclosure3rds => 60,
        }
    }

    #[must_use]
    pub fn target_bpm(self) -> u32 {
        match self {
            Self::MajorScale | Self::MinorScale => 120,
            Self::Chromatic => 132,
            Self::Hanon1 => 108,
            Self::MajorArpeggio => 100,
            Self::Shell251 => 120,
            Self::Rootless251 => 140,
            Self::Charleston => 140,
            Self::BebopDominant => 160,
            Self::BarryHarris => 100,
            Self::Enclosure3rds => 140,
        }
    }

    #[must_use]
    pub fn click(self) -> ClickMode {
        match self {
            Self::Shell251
            | Self::Rootless251
            | Self::Charleston
            | Self::BebopDominant
            | Self::BarryHarris
            | Self::Enclosure3rds => ClickMode::TwoAndFour,
            _ => ClickMode::AllBeats,
        }
    }

    /// One line of coaching, shown under the title.
    #[must_use]
    pub fn coach(self) -> &'static str {
        match self {
            Self::MajorScale => "thumb passes under 3 and 4 \u{2014} never on a black key",
            Self::MinorScale => "the raised 7th is the colour \u{2014} keep it even",
            Self::Chromatic => "3 on every black key \u{00b7} slide, don't hop",
            Self::Hanon1 => "even weight on 4 and 5 \u{2014} the weak-finger builder",
            Self::MajorArpeggio => "arm carries the hand \u{2014} the thumb just lands",
            Self::Shell251 => "root, 3rd, 7th \u{2014} one voice moves per change",
            Self::Rootless251 => "Bill Evans' left hand \u{00b7} stay near middle C",
            Self::Charleston => "beat 1 and the and-of-2 \u{2014} the first comp rhythm",
            Self::BebopDominant => "8 notes \u{2014} chord tones land on downbeats",
            Self::BarryHarris => "chord \u{00b7} diminished \u{00b7} chord \u{2014} the moving stairway",
            Self::Enclosure3rds => "surround the 3rd, land it on the beat",
        }
    }
}

/// Build one exercise: a family, in a key, with a hands variant.
#[must_use]
pub fn build(family: Family, key: u8, hands: Hands) -> Exercise {
    let key = key % 12;
    let notes = match family {
        Family::MajorScale => scale_run(48 + key, &MAJOR_STEPS, &major_fingering(key), hands, 2),
        Family::MinorScale => {
            scale_run(48 + key, &HARMONIC_MINOR_STEPS, &harmonic_minor_fingering(key), hands, 2)
        }
        Family::Chromatic => chromatic_run(48 + key, hands),
        Family::Hanon1 => hanon1(48 + key, hands),
        Family::MajorArpeggio => major_arpeggio(48 + key, hands, 2),
        Family::Shell251 => shell_251_cycle(),
        Family::Rootless251 => rootless_251_cycle(),
        Family::Charleston => charleston(key),
        Family::BebopDominant => bebop_dominant(60 + key),
        Family::BarryHarris => barry_harris(48 + key),
        Family::Enclosure3rds => enclosure_3rds(),
    };
    let loop_ticks = bars_for(&notes);
    let id = if family.keyed() {
        if family.handed() {
            format!("{}:{}:{}", family.key(), NOTE_NAMES[key as usize], hands.key())
        } else {
            format!("{}:{}", family.key(), NOTE_NAMES[key as usize])
        }
    } else {
        family.key().to_string()
    };
    let title = if family.keyed() {
        format!("{} \u{00b7} {}", family.title(), NOTE_NAMES[key as usize])
    } else {
        family.title().to_string()
    };
    Exercise {
        id,
        title,
        level: family.level(),
        start_bpm: family.start_bpm(),
        target_bpm: family.target_bpm(),
        click: family.click(),
        notes,
        loop_ticks,
    }
}

fn chromatic_run(root: u8, hands: Hands) -> Vec<TargetNote> {
    let step_ticks = PPQ / 2;
    let count = 24i32; // two octaves up
    let mut order: Vec<i32> = (0..=count).collect();
    order.extend((0..count).rev());
    let mut out = Vec::new();
    for (pos, &idx) in order.iter().enumerate() {
        let tick = pos as i64 * step_ticks;
        let dur = step_ticks - PPQ / 16;
        let note = (i32::from(root) + idx).clamp(0, 127) as u8;
        let pc = note % 12;
        if matches!(hands, Hands::Right | Hands::Together) {
            out.push(TargetNote {
                tick,
                note,
                finger: chromatic_finger(pc, Hand::Right),
                hand: Hand::Right,
                duration: dur,
            });
        }
        if matches!(hands, Hands::Left | Hands::Together) {
            let lnote = (i32::from(note) - 12).clamp(0, 127) as u8;
            out.push(TargetNote {
                tick,
                note: lnote,
                finger: chromatic_finger(lnote % 12, Hand::Left),
                hand: Hand::Left,
                duration: dur,
            });
        }
    }
    out
}

/// Hanon No. 1: the 8-note cell walked up two octaves and back. Ascending
/// cell degrees (from each step's root): 1 3 4 5 6 5 4 3 in C position;
/// descending cell mirrors. Sixteenth-feel at practice tempi as 8ths.
fn hanon1(root: u8, hands: Hands) -> Vec<TargetNote> {
    let step_ticks = PPQ / 2;
    // Semitone offsets of the ascending cell within C major from the
    // cell root, and the RH fingers for them.
    const ASC_CELL: [i32; 8] = [0, 4, 5, 7, 9, 7, 5, 4];
    const DESC_CELL: [i32; 8] = [12, 7, 5, 4, 2, 4, 5, 7];
    const RH_ASC: [u8; 8] = [1, 2, 3, 4, 5, 4, 3, 2];
    const LH_ASC: [u8; 8] = [5, 4, 3, 2, 1, 2, 3, 4];
    const RH_DESC: [u8; 8] = [5, 4, 3, 2, 1, 2, 3, 4];
    const LH_DESC: [u8; 8] = [1, 2, 3, 4, 5, 4, 3, 2];

    let scale = MAJOR_STEPS;
    let degree_semis = |deg: i32| -> i32 { 12 * deg.div_euclid(7) + scale[deg.rem_euclid(7) as usize] };
    let mut out = Vec::new();
    let mut pos = 0i64;
    // 8 ascending cells (C..C an octave up), then 8 descending.
    for cell in 0..8 {
        for k in 0..8 {
            let semis = degree_semis(cell) + ASC_CELL[k];
            push_hanon(&mut out, root, semis, RH_ASC[k], LH_ASC[k], hands, pos, step_ticks);
            pos += 1;
        }
    }
    for cell in (0..8).rev() {
        for k in 0..8 {
            let semis = degree_semis(cell) + DESC_CELL[k] - 12;
            push_hanon(&mut out, root, semis, RH_DESC[k], LH_DESC[k], hands, pos, step_ticks);
            pos += 1;
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn push_hanon(
    out: &mut Vec<TargetNote>,
    root: u8,
    semis: i32,
    rh: u8,
    lh: u8,
    hands: Hands,
    pos: i64,
    step_ticks: i64,
) {
    let tick = pos * step_ticks;
    let dur = step_ticks - PPQ / 16;
    if matches!(hands, Hands::Right | Hands::Together) {
        let note = (i32::from(root) + 12 + semis).clamp(0, 127) as u8;
        out.push(TargetNote { tick, note, finger: rh, hand: Hand::Right, duration: dur });
    }
    if matches!(hands, Hands::Left | Hands::Together) {
        let note = (i32::from(root) + semis).clamp(0, 127) as u8;
        out.push(TargetNote { tick, note, finger: lh, hand: Hand::Left, duration: dur });
    }
}

/// Major arpeggio, root position, the white-key rule set: RH 1-2-3 top 5,
/// LH 5-4-2 top 1. Triplet feel written as even eighths.
fn major_arpeggio(root: u8, hands: Hands, octaves: u8) -> Vec<TargetNote> {
    let step_ticks = PPQ / 2;
    const CHORD: [i32; 3] = [0, 4, 7];
    const RH: [u8; 3] = [1, 2, 3];
    const LH: [u8; 3] = [5, 4, 2];
    let octaves = octaves.max(1) as i32;
    let count = 3 * octaves;
    let mut order: Vec<i32> = (0..=count).collect();
    order.extend((0..count).rev());
    let mut out = Vec::new();
    for (pos, &idx) in order.iter().enumerate() {
        let at_top = idx == count;
        let semis = if at_top { 12 * octaves } else { 12 * (idx / 3) + CHORD[(idx % 3) as usize] };
        let tick = pos as i64 * step_ticks;
        let dur = step_ticks - PPQ / 16;
        if matches!(hands, Hands::Right | Hands::Together) {
            let finger = if at_top { 5 } else { RH[(idx % 3) as usize] };
            let note = (i32::from(root) + semis).clamp(0, 127) as u8;
            out.push(TargetNote { tick, note, finger, hand: Hand::Right, duration: dur });
        }
        if matches!(hands, Hands::Left | Hands::Together) {
            let finger = if at_top { 1 } else { LH[(idx % 3) as usize] };
            let note = (i32::from(root) + semis - 12).clamp(0, 127) as u8;
            out.push(TargetNote { tick, note, finger, hand: Hand::Left, duration: dur });
        }
    }
    out
}

/// Place a chord's pitch classes as one voicing near a register center.
fn voice_near(pcs: &[i32], root_pc: i32, center: i32, fingers: &[u8], tick: i64, dur: i64, out: &mut Vec<TargetNote>) {
    // Lowest voice lands in the octave nearest (center - 7); the rest stack
    // upward from it, each the next pitch above the previous.
    let mut prev = None;
    for (k, &pc) in pcs.iter().enumerate() {
        let abs_pc = (root_pc + pc).rem_euclid(12);
        let base = match prev {
            None => {
                let mut n = abs_pc + 12 * ((center - 7) / 12);
                while n < center - 13 {
                    n += 12;
                }
                while n > center - 1 {
                    n -= 12;
                }
                n
            }
            Some(p) => {
                let mut n = abs_pc + 12 * (p / 12);
                while n <= p {
                    n += 12;
                }
                n
            }
        };
        prev = Some(base);
        out.push(TargetNote {
            tick,
            note: base.clamp(0, 127) as u8,
            finger: fingers.get(k).copied().unwrap_or(0),
            hand: Hand::Left,
            duration: dur,
        });
    }
}

/// The circle of fourths from C — the jazz default ordering.
const CYCLE: [i32; 12] = [0, 5, 10, 3, 8, 1, 6, 11, 4, 9, 2, 7];

/// Shell ii-V-I around the cycle: A-shell on the ii, B-shell on the V and
/// I — one voice moves per change. One key per four bars, all 12 keys.
fn shell_251_cycle() -> Vec<TargetNote> {
    let bar = PPQ * 4;
    let mut out = Vec::new();
    for (k, &key) in CYCLE.iter().enumerate() {
        let start = k as i64 * 4 * bar;
        let ii = key + 2;
        let v = key + 7;
        voice_near(&[0, 3, 10], ii, 55, &[5, 3, 1], start, bar - 60, &mut out);
        voice_near(&[0, 10, 16], v, 55, &[5, 2, 1], start + bar, bar - 60, &mut out);
        voice_near(&[0, 11, 16], key, 55, &[5, 2, 1], start + 2 * bar, 2 * bar - 60, &mut out);
    }
    out
}

/// Rootless A/B ii-V-I around the cycle. Dm9 A-form 3-5-7-9, G13 B-form
/// 7-9-3-13, Cmaj9 A-form 3-5-7-9 — the one-voice-glides drill.
fn rootless_251_cycle() -> Vec<TargetNote> {
    let bar = PPQ * 4;
    let mut out = Vec::new();
    for (k, &key) in CYCLE.iter().enumerate() {
        let start = k as i64 * 4 * bar;
        let ii = key + 2;
        let v = key + 7;
        // ii m9 A-form: b3 5 b7 9.
        voice_near(&[0, 4, 7, 11], ii + 3, 57, &[5, 3, 2, 1], start, bar - 60, &mut out);
        // V13 B-form: b7 9 3 13.
        voice_near(&[0, 4, 9, 16], v + 10, 57, &[5, 3, 2, 1], start + bar, bar - 60, &mut out);
        // I maj9 A-form: 3 5 7 9.
        voice_near(&[0, 3, 7, 10], key + 4, 57, &[5, 3, 2, 1], start + 2 * bar, 2 * bar - 60, &mut out);
    }
    out
}

/// The Charleston: shells on beat 1 and the and-of-2, ii-V-I-I loop in the
/// chosen key, four bars.
fn charleston(key: u8) -> Vec<TargetNote> {
    let bar = PPQ * 4;
    let key = i32::from(key);
    let chords: [(&[i32], i32); 4] =
        [(&[0, 3, 10], key + 2), (&[0, 10, 16], key + 7), (&[0, 11, 16], key), (&[0, 11, 16], key)];
    let mut out = Vec::new();
    for (b, (pcs, root)) in chords.iter().enumerate() {
        let start = b as i64 * bar;
        // Beat 1 (dotted quarter) + and-of-2.
        voice_near(pcs, *root, 55, &[5, 3, 1], start, PPQ * 3 / 2 - 30, &mut out);
        voice_near(pcs, *root, 55, &[5, 3, 1], start + PPQ * 3 / 2, PPQ - 30, &mut out);
    }
    out
}

/// The dominant bebop scale, descending from the root, two passes, with
/// the crawl fingering (4321 4321) — the jazz-simplified standard.
fn bebop_dominant(root: u8) -> Vec<TargetNote> {
    let step_ticks = PPQ / 2;
    // Descending from root: R, maj7 passing, b7, 13, 5, 11, 3, 9.
    const DESC: [i32; 8] = [0, -1, -2, -3, -5, -7, -8, -10];
    const CRAWL: [u8; 8] = [4, 3, 2, 1, 4, 3, 2, 1];
    let mut out = Vec::new();
    for pass in 0..2i64 {
        for k in 0..8usize {
            let tick = (pass * 8 + k as i64) * step_ticks;
            let note = (i32::from(root) + DESC[k] - 12 * pass as i32).clamp(0, 127) as u8;
            out.push(TargetNote {
                tick,
                note,
                finger: CRAWL[k],
                hand: Hand::Right,
                duration: step_ticks - PPQ / 16,
            });
        }
    }
    // Land on the root two octaves down, on the downbeat.
    out.push(TargetNote {
        tick: 16 * step_ticks,
        note: (i32::from(root) - 24).clamp(0, 127) as u8,
        finger: 1,
        hand: Hand::Right,
        duration: PPQ - 60,
    });
    out
}

/// Barry Harris: the major 6th-diminished scale harmonized — C6, D°7,
/// C6/E, F°7 ... up the octave and back, quarter notes, RH 4 voices.
fn barry_harris(root: u8) -> Vec<TargetNote> {
    // The 8-step scale: 6th chord tones interleaved with dim7 tones.
    const SCALE: [i32; 8] = [0, 2, 4, 5, 7, 8, 9, 11];
    const FINGERS: [u8; 4] = [1, 2, 3, 5];
    let step_ticks = PPQ;
    let mut order: Vec<i32> = (0..8).collect();
    order.extend((0..8).rev());
    let mut out = Vec::new();
    for (pos, &step) in order.iter().enumerate() {
        let tick = pos as i64 * step_ticks;
        for v in 0..4i32 {
            let idx = step + 2 * v;
            let semis = 12 * idx.div_euclid(8) + SCALE[idx.rem_euclid(8) as usize];
            out.push(TargetNote {
                tick,
                note: (i32::from(root) + 12 + semis).clamp(0, 127) as u8,
                finger: FINGERS[v as usize],
                hand: Hand::Right,
                duration: step_ticks - 60,
            });
        }
    }
    out
}

/// Enclosures targeting the 3rd of every major triad around the cycle:
/// above, below, land — swing 8th pickups into a downbeat, one key per bar.
fn enclosure_3rds() -> Vec<TargetNote> {
    let bar = PPQ * 4;
    let eighth = PPQ / 2;
    let mut out = Vec::new();
    for (k, &key) in CYCLE.iter().enumerate() {
        let start = k as i64 * bar;
        let third = 60 + ((key + 4).rem_euclid(12)) as u8;
        // Pickups on the and-of-4 of the previous bar would cross the rep
        // start; keep them inside: beats 3&, 4& then land on the next 1.
        out.push(TargetNote {
            tick: start + 2 * PPQ + eighth,
            note: third + 1,
            finger: 3,
            hand: Hand::Right,
            duration: eighth - 30,
        });
        out.push(TargetNote {
            tick: start + 3 * PPQ + eighth,
            note: third - 1,
            finger: 1,
            hand: Hand::Right,
            duration: eighth - 30,
        });
        out.push(TargetNote {
            tick: start + 4 * PPQ,
            note: third,
            finger: 2,
            hand: Hand::Right,
            duration: PPQ - 30,
        });
    }
    out
}

// ── The room ──

/// What happened during a tick, for the App to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomEvent {
    /// A rep finished; the report is in `run.last_report`.
    RepDone,
    /// Three clean reps — the tempo went up. The click needs re-arming.
    BpmUp,
}

/// One running exercise.
#[derive(Debug)]
pub struct Run {
    pub exercise: Exercise,
    pub judge: judge::Judge,
    pub rep: u32,
    pub streak: u32,
    pub last_report: Option<judge::RepReport>,
    /// Micros at which this rep's tick 0 falls (after the count-in).
    pub anchor_micros: u64,
    /// Keys currently held, for the keyboard display.
    pub down: Vec<u8>,
}

/// The practice room: browsing state, the running exercise, saved records.
#[derive(Debug, Default)]
pub struct Room {
    pub open: bool,
    pub cursor: usize,
    /// Index into the cycle-of-fourths ordering — `<`/`>` walk keys the
    /// way jazz practice walks them.
    pub key_pos: usize,
    pub hands: Hands,
    pub mode: judge::Mode,
    pub click: ClickMode,
    /// Manual tempo override; None follows the record.
    pub bpm: Option<u32>,
    pub run: Option<Run>,
    pub progress: progress::Progress,
    pub progress_dirty: bool,
}



impl Room {
    pub fn family(&self) -> Family {
        Family::ALL[self.cursor.min(Family::ALL.len() - 1)]
    }

    pub fn key(&self) -> u8 {
        const CYCLE: [u8; 12] = [0, 5, 10, 3, 8, 1, 6, 11, 4, 9, 2, 7];
        CYCLE[self.key_pos % 12]
    }

    /// The exercise the current selection names.
    #[must_use]
    pub fn selected(&self) -> Exercise {
        let family = self.family();
        let hands = if family.handed() { self.hands } else { Hands::Left };
        build(family, self.key(), hands)
    }

    /// The tempo a run would start at: the saved clean tempo, or the
    /// family's floor — pick up where the hands left off.
    #[must_use]
    pub fn start_bpm(&self) -> u32 {
        if let Some(bpm) = self.bpm {
            return bpm;
        }
        let ex = self.selected();
        self.progress
            .get(&ex.id)
            .map(|r| r.clean_bpm)
            .filter(|&b| b > 0)
            .unwrap_or(ex.start_bpm)
            .max(ex.start_bpm)
    }

    #[must_use]
    pub fn record_for(&self, id: &str) -> u32 {
        self.progress.get(id).map(|r| r.clean_bpm).unwrap_or(0)
    }

    /// Open, with saved progress handed in.
    pub fn open_with(&mut self, progress: progress::Progress) {
        self.open = true;
        self.progress = progress;
        self.run = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.run = None;
    }

    /// Start a rep at `now`. Flow gets a four-beat count-in; wait starts
    /// at once.
    pub fn start(&mut self, now_micros: u64) {
        let exercise = self.selected();
        let bpm = self.start_bpm();
        let window = judge::WINDOW_COMFORTABLE_MS;
        let mut j = judge::Judge::new(exercise.notes.clone(), self.mode, bpm, window);
        let beat_micros = 60_000_000u64 / u64::from(bpm.max(1));
        let anchor = now_micros + 4 * beat_micros;
        j.set_anchor(anchor);
        self.run = Some(Run {
            exercise,
            judge: j,
            rep: 1,
            streak: 0,
            last_report: None,
            anchor_micros: anchor,
            down: Vec::new(),
        });
    }

    pub fn stop(&mut self) {
        self.run = None;
    }

    /// Live MIDI in. Sound has already gone to the synth; this is the ear.
    pub fn note_on(&mut self, note: u8, micros: u64) {
        if let Some(run) = &mut self.run {
            if !run.down.contains(&note) {
                run.down.push(note);
            }
            run.judge.note_on(note, micros);
        }
    }

    pub fn note_off(&mut self, note: u8) {
        if let Some(run) = &mut self.run {
            run.down.retain(|&n| n != note);
        }
    }

    /// Advance the clock; finish and roll reps. Returns what happened.
    pub fn tick(&mut self, now_micros: u64) -> Vec<RoomEvent> {
        let mut events = Vec::new();
        let Some(run) = &mut self.run else { return events };
        run.judge.expire(now_micros);
        if !run.judge.done() {
            return events;
        }
        // The rep is resolved: measure, score the ladder, roll the next.
        let report = run.judge.report(judge::WINDOW_TIGHT_MS);
        run.last_report = Some(report);
        events.push(RoomEvent::RepDone);

        let id = run.exercise.id.clone();
        let floor = run.exercise.start_bpm;
        let record = self.progress.get(&id).map(|r| r.clean_bpm).unwrap_or(0);
        let bpm = self.bpm.unwrap_or_else(|| record.max(floor));
        let entry = self.progress.entry(id).or_default();
        entry.attempts += 1;
        if report.clean {
            entry.clean_reps += 1;
            run.streak += 1;
            if bpm > entry.clean_bpm {
                entry.clean_bpm = bpm;
            }
            if run.streak >= 3 {
                // Three clean in a row: the ladder climbs.
                self.bpm = Some(bpm + 5);
                run.streak = 0;
                events.push(RoomEvent::BpmUp);
            }
        } else {
            run.streak = 0;
        }
        self.progress_dirty = true;

        // Roll the next rep on the next bar boundary of the same clock, so
        // the click never hiccups.
        let new_bpm = self.bpm.unwrap_or(bpm);
        let exercise = run.exercise.clone();
        let mode = self.mode;
        let window = run.judge.window_ms;
        let micros_per_tick = 60_000_000.0 / (f64::from(new_bpm) * 960.0);
        let loop_micros = (exercise.loop_ticks as f64 * micros_per_tick) as u64;
        let mut anchor = run.anchor_micros + loop_micros;
        let beat = 60_000_000u64 / u64::from(new_bpm.max(1));
        while anchor < now_micros + beat {
            anchor += 4 * beat;
        }
        let mut j = judge::Judge::new(exercise.notes.clone(), mode, new_bpm, window);
        j.set_anchor(anchor);
        run.judge = j;
        run.rep += 1;
        run.anchor_micros = anchor;
        events
    }
}

#[cfg(test)]
mod room_tests {
    use super::*;

    /// The whole rep ladder: three clean reps raise the tempo, the record
    /// remembers the clean BPM, and a dirty rep resets the streak.
    #[test]
    fn three_clean_reps_climb_the_ladder() {
        let mut room = Room { open: true, mode: judge::Mode::Wait, ..Default::default() };
        room.cursor = 0; // major scale
        room.start(0);
        let notes = room.run.as_ref().unwrap().exercise.notes.clone();
        let start_bpm = room.start_bpm();

        for rep in 0..3 {
            for n in &notes {
                room.note_on(n.note, 1000 + n.tick as u64);
                room.note_off(n.note);
            }
            let events = room.tick(10_000_000 * (rep + 1));
            assert!(events.contains(&RoomEvent::RepDone));
            if rep == 2 {
                assert!(events.contains(&RoomEvent::BpmUp), "third clean rep did not climb");
            }
        }
        assert_eq!(room.bpm, Some(start_bpm + 5));
        let ex_id = room.run.as_ref().unwrap().exercise.id.clone();
        assert_eq!(room.record_for(&ex_id), start_bpm, "the record did not learn the clean bpm");

        // A wrong-note rep breaks the streak.
        room.note_on(21, 99);
        for n in &notes {
            room.note_on(n.note, 1000 + n.tick as u64);
        }
        room.tick(90_000_000);
        assert_eq!(room.run.as_ref().unwrap().streak, 0, "a dirty rep kept the streak");
    }

    /// Picking an exercise with a saved record starts at the record, not
    /// back at the floor.
    #[test]
    fn practice_resumes_at_the_record() {
        let mut room = Room { cursor: 0, ..Default::default() };
        let id = room.selected().id;
        room.progress.insert(
            id,
            progress::ExerciseRecord { clean_bpm: 92, clean_reps: 9, attempts: 20 },
        );
        assert_eq!(room.start_bpm(), 92);
    }
}
