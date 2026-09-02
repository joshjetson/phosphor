//! UI-side mirror of the MIDI-effect layer: what sits in a track's
//! pre-instrument slots, so the strip, the chain list, the session file and
//! the undo stack all read one place. The audio thread holds the real
//! effects; this is the front end's copy of their settings, edited through
//! the one path that also tells the mixer.

use phosphor_core::fx::FxParamInfo;
use phosphor_core::midi_fx::{
    ARP_PARAMS, CHORD_PARAMS, COLOR_LABELS, MODE_LABELS, NOTE_NAMES, PROG_LABELS, RATE_LABELS,
    SCALE_LABELS, STYLE_LABELS, VOICING_LABELS,
};

/// Which MIDI effect a slot holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiFxType {
    Chord,
    Arp,
}

impl MidiFxType {
    /// Every type, in menu order — chord first, because it leads the chain.
    pub const ALL: [MidiFxType; 2] = [MidiFxType::Chord, MidiFxType::Arp];

    /// What the menu and the chain list call it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Chord => "chord",
            Self::Arp => "arp",
        }
    }

    /// What the add menu calls it — marked, because it lands in the MIDI
    /// rack rather than the insert chain.
    #[must_use]
    pub fn menu_label(self) -> &'static str {
        match self {
            Self::Chord => "chord \u{00b7} midi",
            Self::Arp => "arp \u{00b7} midi",
        }
    }

    /// The stable name sessions store and the mixer builds from.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Chord => "chord",
            Self::Arp => "arp",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "chord" => Some(Self::Chord),
            "arp" => Some(Self::Arp),
            _ => None,
        }
    }

    /// The type's parameter table, shared with the audio-thread effect.
    #[must_use]
    pub fn params(self) -> &'static [FxParamInfo] {
        match self {
            Self::Chord => &CHORD_PARAMS,
            Self::Arp => &ARP_PARAMS,
        }
    }

    /// The label a parameter's current value reads as, for parameters whose
    /// numbers name things rather than measure them.
    #[must_use]
    pub fn value_label(self, param: usize, value: f32) -> Option<&'static str> {
        let idx = value.round() as usize;
        match (self, param) {
            (Self::Arp, 0) => STYLE_LABELS.get(idx).copied(),
            (Self::Arp, 1) => RATE_LABELS.get(idx).copied(),
            (Self::Arp, 4) => Some(if value >= 0.5 { "hold" } else { "off" }),
            (Self::Arp, 5) if value < 0.5 => Some("as played"),
            (Self::Chord, 0) => NOTE_NAMES.get(idx % 12).copied(),
            (Self::Chord, 1) => SCALE_LABELS.get(idx).copied(),
            (Self::Chord, 2) => COLOR_LABELS.get(idx).copied(),
            (Self::Chord, 3) => VOICING_LABELS.get(idx).copied(),
            (Self::Chord, 5) => Some(["off", "root -1 oct", "root -2 oct"][idx.min(2)]),
            (Self::Chord, 7) => MODE_LABELS.get(idx).copied(),
            (Self::Chord, 8) => PROG_LABELS.get(idx).copied(),
            _ => None,
        }
    }
}

/// The arp's factory feels: a musician judges a device by its first
/// sound, so the panel's number keys land somewhere musical at once.
/// (name, [(param, value)]) — parameters not listed keep their setting.
pub const ARP_PRESETS: [(&str, [(usize, f32); 5]); 4] = [
    // style, rate, gate, octaves, swing
    ("rhodes 8ths", [(0, 0.0), (1, 2.0), (2, 75.0), (3, 1.0), (6, 56.0)]),
    ("dilla 16ths", [(0, 3.0), (1, 5.0), (2, 45.0), (3, 1.0), (6, 58.0)]),
    ("wide updown", [(0, 2.0), (1, 5.0), (2, 60.0), (3, 2.0), (6, 50.0)]),
    ("chord pulse", [(0, 4.0), (1, 2.0), (2, 40.0), (3, 1.0), (6, 54.0)]),
];

impl MidiFxType {
    /// The words a parameter's value reads as on the panel — the label
    /// where the number names a thing, the note name for the split, the
    /// plain number with its unit for everything else.
    #[must_use]
    pub fn value_text(self, param: usize, value: f32) -> String {
        if let Some(label) = self.value_label(param, value) {
            return label.to_string();
        }
        if self == Self::Chord && param == 4 {
            let n = value.round() as i32;
            let name = NOTE_NAMES[(n.rem_euclid(12)) as usize];
            let octave = n / 12 - 1;
            return format!("{name}{octave}");
        }
        let unit = self.params().get(param).map(|p| p.unit).unwrap_or("");
        format!("{value:.0}{unit}")
    }
}

/// Which half of a track's combined rack a cursor position names. The
/// chain list draws MIDI slots above the audio inserts — top to bottom is
/// the order the signal flows — and one cursor walks both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RackSlot {
    Midi(usize),
    Audio(usize),
}

/// One slot's front-end state: the type, the switch, the settings.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiFxInstance {
    pub fx_type: MidiFxType,
    pub bypass: bool,
    pub params: Vec<f32>,
}

impl MidiFxInstance {
    /// A fresh instance at the type's defaults.
    #[must_use]
    pub fn new(fx_type: MidiFxType) -> Self {
        Self {
            fx_type,
            bypass: false,
            params: fx_type.params().iter().map(|p| p.default).collect(),
        }
    }

    /// In the signal path right now.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.bypass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI's parameter table and the audio thread's must be the same
    /// table — a panel drawing ranges the effect does not have is a lie.
    #[test]
    fn the_panel_table_matches_the_effect() {
        use phosphor_core::midi_fx::MidiEffect as _;
        let arp = phosphor_core::midi_fx::Arpeggiator::new();
        let table = MidiFxType::Arp.params();
        assert_eq!(arp.parameter_count(), table.len());
        for (i, info) in table.iter().enumerate() {
            let theirs = arp.parameter_info(i).expect("effect param missing");
            assert_eq!(theirs, *info, "param {i} diverged");
            assert_eq!(arp.get_parameter(i), info.default, "default {i} diverged");
        }
    }
}
/// Render a clip through a MIDI rack, offline — the one code path under
/// both the piano roll's ghost notes and the commit action, so what the
/// ghosts show is exactly what a commit would print.
///
/// Fresh effect instances are built from the mirror and run from reset
/// state in fixed blocks, which is what makes the render deterministic:
/// the same clip through the same settings always produces the same notes.
/// Only note events come back, clip-relative and clamped to the clip;
/// controllers pass through the chain unchanged and the clip already owns
/// them.
#[must_use]
pub fn render_clip_through_rack(
    clip: &super::Clip,
    rack: &[MidiFxInstance],
    clip_start_tick: i64,
    sample_rate: f32,
    tempo_bpm: f64,
) -> Vec<phosphor_core::clip::ClipEvent> {
    use phosphor_core::midi_fx::{build_midi_fx, MidiFxContext};

    let mut chain: Vec<Box<dyn phosphor_core::midi_fx::MidiEffect>> = Vec::new();
    for slot in rack.iter().filter(|s| !s.bypass) {
        let Some(mut fx) = build_midi_fx(slot.fx_type.key()) else { continue };
        for (index, &value) in slot.params.iter().enumerate() {
            fx.set_parameter(index, value);
        }
        fx.init(f64::from(sample_rate), 256);
        fx.reset();
        chain.push(fx);
    }
    let input = clip.events_for_audio();
    if chain.is_empty() {
        return input.into_iter().filter(|e| matches!(e.status & 0xF0, 0x90 | 0x80)).collect();
    }

    const BLOCK: u32 = 256;
    let ticks_per_sample = (tempo_bpm * phosphor_core::transport::Transport::PPQ as f64)
        / (60.0 * f64::from(sample_rate));
    let block_ticks = f64::from(BLOCK) * ticks_per_sample;
    let length = clip.length_ticks.max(1);
    // A beat of tail lets gates that reach past the end close on their own.
    let total_ticks = length as f64 + phosphor_core::transport::Transport::PPQ as f64;
    let blocks = (total_ticks / block_ticks).ceil() as i64;

    let mut events = Vec::new();
    let mut feed: Vec<phosphor_plugin::MidiEvent> = Vec::new();
    let mut buf_a: Vec<phosphor_plugin::MidiEvent> = Vec::with_capacity(1024);
    let mut buf_b: Vec<phosphor_plugin::MidiEvent> = Vec::with_capacity(1024);
    let mut cursor = 0usize; // next input event to feed
    let mut sorted = input;
    sorted.sort_by_key(|e| (e.tick, phosphor_core::clip::same_tick_order(e.status)));

    for b in 0..blocks {
        let start_rel = (b as f64 * block_ticks) as i64;
        let end_rel = ((b + 1) as f64 * block_ticks) as i64;
        feed.clear();
        while cursor < sorted.len() && sorted[cursor].tick < end_rel {
            let e = &sorted[cursor];
            let offset = ((e.tick - start_rel).max(0) as f64 / ticks_per_sample) as u32;
            feed.push(phosphor_plugin::MidiEvent {
                sample_offset: offset.min(BLOCK - 1),
                status: e.status,
                data1: e.data1,
                data2: e.data2,
            });
            cursor += 1;
        }
        let ctx = MidiFxContext {
            sample_rate,
            tempo_bpm,
            playing: true,
            num_frames: BLOCK,
            block_start_tick: clip_start_tick + start_rel,
            ticks_per_sample,
        };
        buf_a.clear();
        buf_a.extend_from_slice(&feed);
        for fx in &mut chain {
            buf_b.clear();
            fx.process(&buf_a, &mut buf_b, &ctx);
            std::mem::swap(&mut buf_a, &mut buf_b);
        }
        for e in &buf_a {
            if !matches!(e.status & 0xF0, 0x90 | 0x80) {
                continue;
            }
            let tick = start_rel + (f64::from(e.sample_offset) * ticks_per_sample) as i64;
            events.push(phosphor_core::clip::ClipEvent {
                tick: tick.min(length),
                status: e.status,
                data1: e.data1,
                data2: e.data2,
            });
        }
    }
    // Anything still latched or gated closes at the end.
    buf_a.clear();
    for fx in &mut chain {
        fx.flush(&mut buf_a);
    }
    for e in &buf_a {
        if e.status & 0xF0 == 0x80 {
            events.push(phosphor_core::clip::ClipEvent {
                tick: length,
                status: e.status,
                data1: e.data1,
                data2: e.data2,
            });
        }
    }
    events
}

/// Pair a rendered event stream into displayable notes — the same pairing
/// the recorder's snapshot uses, borrowed through [`MidiClip`].
#[must_use]
pub fn rendered_events_to_notes(
    events: Vec<phosphor_core::clip::ClipEvent>,
    length_ticks: i64,
) -> Vec<phosphor_core::clip::NoteSnapshot> {
    let clip = phosphor_core::clip::MidiClip::new(0, length_ticks, events);
    phosphor_core::clip::ClipSnapshot::from_clip(0, 0, &clip).notes
}
#[cfg(test)]
mod render_tests {
    use super::*;
    use phosphor_core::clip::NoteSnapshot;
    use phosphor_core::transport::Transport;

    const BAR: i64 = Transport::PPQ * 4;

    fn one_note_clip(note: u8, start_tick: i64, duration_ticks: i64) -> crate::state::Clip {
        crate::state::Clip {
            number: 1,
            width: 4,
            has_content: true,
            start_tick: 0,
            length_ticks: BAR,
            notes: vec![NoteSnapshot { note, velocity: 100, start_tick, duration_ticks, muted: false }],
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        }
    }

    /// No active devices: the render is the clip's own notes, untouched.
    #[test]
    fn an_empty_rack_renders_identity() {
        let clip = one_note_clip(60, 0, 960);
        let events = render_clip_through_rack(&clip, &[], 0, 44_100.0, 120.0);
        let notes = rendered_events_to_notes(events, BAR);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].note, 60);
        assert_eq!(notes[0].start_tick, 0);

        // Bypassed counts as absent.
        let mut arp = MidiFxInstance::new(MidiFxType::Arp);
        arp.bypass = true;
        let events = render_clip_through_rack(&clip, &[arp], 0, 44_100.0, 120.0);
        assert_eq!(rendered_events_to_notes(events, BAR).len(), 1);
    }

    /// The chord device prints a chord for a one-key clip, and prints the
    /// same chord every time — determinism is what makes ghosts honest.
    #[test]
    fn a_chord_renders_deterministically() {
        let clip = one_note_clip(48, 0, 1920);
        let rack = vec![MidiFxInstance::new(MidiFxType::Chord)];
        let a = render_clip_through_rack(&clip, &rack, 0, 44_100.0, 120.0);
        let b = render_clip_through_rack(&clip, &rack, 0, 44_100.0, 120.0);
        assert_eq!(a, b, "two renders of the same clip differed");
        let notes = rendered_events_to_notes(a, BAR);
        assert!(notes.len() >= 4, "one key should print a full chord, got {notes:?}");
        // Every printed note starts where the key was pressed.
        assert!(notes.iter().all(|n| n.start_tick < 60), "the chord smeared: {notes:?}");
    }

    /// A held note through the arp prints a run of short notes on the
    /// grid, every one closed inside the clip.
    #[test]
    fn an_arp_prints_a_run_that_closes() {
        let clip = one_note_clip(60, 0, BAR - 60);
        let rack = vec![MidiFxInstance::new(MidiFxType::Arp)];
        let events = render_clip_through_rack(&clip, &rack, 0, 44_100.0, 120.0);
        let notes = rendered_events_to_notes(events, BAR);
        // A bar of 1/16ths from a held note: sixteen steps, give or take
        // the boundary.
        assert!(
            (14..=17).contains(&notes.len()),
            "expected about sixteen steps, got {}: {notes:?}",
            notes.len()
        );
        assert!(notes.iter().all(|n| n.note == 60));
        assert!(
            notes.iter().all(|n| n.start_tick + n.duration_ticks <= BAR),
            "a step ran past the clip"
        );
        // The steps land on the 1/16 grid.
        for n in &notes {
            let miss = n.start_tick.rem_euclid(240).min(240 - n.start_tick.rem_euclid(240));
            assert!(miss <= 12, "a step missed the grid by {miss}: {notes:?}");
        }
    }

    /// Chord into arp, printed: the run walks the chord's tones.
    #[test]
    fn the_full_chain_prints_the_rolling_voicing() {
        let clip = one_note_clip(48, 0, BAR - 60);
        let mut chord = MidiFxInstance::new(MidiFxType::Chord);
        chord.params[5] = 0.0; // bass off, for clean pitch-class checks
        let rack = vec![chord, MidiFxInstance::new(MidiFxType::Arp)];
        let events = render_clip_through_rack(&clip, &rack, 0, 44_100.0, 120.0);
        let notes = rendered_events_to_notes(events, BAR);
        let mut pc: Vec<u8> = notes.iter().map(|n| n.note % 12).collect();
        pc.sort_unstable();
        pc.dedup();
        assert_eq!(pc, vec![0, 4, 7, 11], "the printed run is not the chord: {pc:?}");
        assert!(notes.len() > 8, "the chain printed too few steps: {}", notes.len());
    }
}
