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
            (Self::Chord, 8) if idx < PROG_LABELS.len() => PROG_LABELS.get(idx).copied(),
            (Self::Chord, 8) => Some("user"),
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
    /// The chord device's user progression, when one is loaded — resolved
    /// chords, not a library reference, so the session owns its sound even
    /// if the library changes later.
    pub custom_chords: Vec<phosphor_core::midi_fx::UserChord>,
    /// What the panel calls the loaded progression.
    pub custom_name: String,
}

impl MidiFxInstance {
    /// A fresh instance at the type's defaults.
    #[must_use]
    pub fn new(fx_type: MidiFxType) -> Self {
        Self {
            fx_type,
            bypass: false,
            params: fx_type.params().iter().map(|p| p.default).collect(),
            custom_chords: Vec::new(),
            custom_name: String::new(),
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
        if !slot.custom_chords.is_empty() {
            fx.set_progression(&slot.custom_chords);
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
/// The progression editor: a small modal over the chord panel where a
/// progression is written one chord at a time — root, quality, slash bass
/// — browsed against the library, named, saved, and loaded into the
/// device. Cursors and a working copy only; the library file and the
/// device both change through `App` methods.
#[derive(Debug, Default)]
pub struct ProgEditor {
    pub open: bool,
    /// The chord slot in the rack this editor is working for.
    pub slot: usize,
    pub row: usize,
    /// 0 = root, 1 = quality, 2 = bass.
    pub col: usize,
    pub chords: Vec<phosphor_core::midi_fx::UserChord>,
    pub name: String,
    /// The library as loaded when the editor opened.
    pub library: Vec<crate::progressions::UserProgression>,
    pub lib_cursor: usize,
    /// Learn mode: the editor is listening to the controller, and the next
    /// chord played replaces the cursor's row.
    pub learn_armed: bool,
    /// The keys currently down while learning.
    learn_held: Vec<u8>,
    /// Every key touched during this learn gesture.
    learn_captured: Vec<u8>,
}

impl ProgEditor {
    /// Open over `slot`, seeded from what the device already holds — its
    /// loaded progression, or a one-chord starting point.
    pub fn open_for(
        &mut self,
        slot: usize,
        chords: &[phosphor_core::midi_fx::UserChord],
        name: &str,
        library: Vec<crate::progressions::UserProgression>,
    ) {
        self.open = true;
        self.slot = slot;
        self.row = 0;
        self.col = 0;
        self.chords = if chords.is_empty() {
            vec![phosphor_core::midi_fx::UserChord::pick(0, 1, -1)]
        } else {
            chords.to_vec()
        };
        self.name = if name.is_empty() { "untitled".to_string() } else { name.to_string() };
        self.library = library;
        self.lib_cursor = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.learn_armed = false;
        self.learn_held.clear();
        self.learn_captured.clear();
    }

    /// Arm or cancel learn. Armed, the next chord played on the controller
    /// replaces the cursor's row with exactly the shape of the hand.
    pub fn toggle_learn(&mut self) -> bool {
        self.learn_armed = !self.learn_armed;
        self.learn_held.clear();
        self.learn_captured.clear();
        self.learn_armed
    }

    /// A key went down while learning.
    pub fn learn_note_on(&mut self, note: u8) {
        if !self.learn_armed {
            return;
        }
        if !self.learn_held.contains(&note) {
            self.learn_held.push(note);
        }
        if !self.learn_captured.contains(&note) {
            self.learn_captured.push(note);
        }
    }

    /// A key came up. When the last one does, the gesture is the chord:
    /// the lowest note is the root, everything else its intervals, exactly
    /// as spaced. Returns true when a chord was captured.
    pub fn learn_note_off(&mut self, note: u8) -> bool {
        if !self.learn_armed {
            return false;
        }
        self.learn_held.retain(|&n| n != note);
        if !self.learn_held.is_empty() || self.learn_captured.is_empty() {
            return false;
        }
        let mut notes = std::mem::take(&mut self.learn_captured);
        notes.sort_unstable();
        let low = notes[0];
        let intervals: Vec<i8> = notes
            .iter()
            .take(8)
            .map(|&n| (n - low).min(120) as i8)
            .collect();
        let chord =
            phosphor_core::midi_fx::UserChord::learned((low % 12) as i8, &intervals, -1);
        if let Some(slot) = self.chords.get_mut(self.row) {
            *slot = chord;
        }
        self.learn_armed = false;
        true
    }

    pub fn move_row(&mut self, delta: i32) {
        let max = self.chords.len().saturating_sub(1) as i32;
        self.row = (self.row as i32 + delta).clamp(0, max) as usize;
    }

    pub fn move_col(&mut self, delta: i32) {
        self.col = (self.col as i32 + delta).rem_euclid(3) as usize;
    }

    /// Turn the selected cell. Roots and basses walk the pitch classes,
    /// the bass with an "off" stop below C; qualities walk the dictionary.
    pub fn adjust(&mut self, delta: i32) {
        let Some(chord) = self.chords.get_mut(self.row) else { return };
        match self.col {
            0 => chord.root = (i32::from(chord.root) + delta).rem_euclid(12) as i8,
            1 => {
                let n = phosphor_core::midi_fx::QUALITIES.len() as i32;
                chord.quality = (i32::from(chord.quality) + delta).rem_euclid(n) as u8;
            }
            _ => {
                // -1 (off) .. 11, stopping at the ends.
                chord.bass = (i32::from(chord.bass) + delta).clamp(-1, 11) as i8;
            }
        }
    }

    /// Add a chord after the cursor, copying it — the next chord in a
    /// progression usually starts life as a variation of the last.
    pub fn add_chord(&mut self) -> bool {
        if self.chords.len() >= phosphor_core::midi_fx::MAX_USER_CHORDS {
            return false;
        }
        let template = self.chords.get(self.row).copied().unwrap_or(
            phosphor_core::midi_fx::UserChord::pick(0, 1, -1),
        );
        self.chords.insert(self.row + 1, template);
        self.row += 1;
        true
    }

    /// Remove the cursor's chord; the last one stays — an empty
    /// progression is not a thing the editor can hold.
    pub fn remove_chord(&mut self) -> bool {
        if self.chords.len() <= 1 {
            return false;
        }
        self.chords.remove(self.row);
        self.row = self.row.min(self.chords.len() - 1);
        true
    }

    /// Step through the library, loading the entry under the cursor into
    /// the working copy. Returns the loaded name, if the library has any.
    pub fn cycle_library(&mut self, delta: i32) -> Option<String> {
        if self.library.is_empty() {
            return None;
        }
        let n = self.library.len() as i32;
        self.lib_cursor = (self.lib_cursor as i32 + delta).rem_euclid(n) as usize;
        let entry = &self.library[self.lib_cursor];
        self.chords = entry.wire_chords();
        self.name = entry.name.clone();
        self.row = self.row.min(self.chords.len().saturating_sub(1));
        Some(entry.name.clone())
    }

    /// The working copy as a library entry.
    #[must_use]
    pub fn to_progression(&self) -> crate::progressions::UserProgression {
        crate::progressions::UserProgression {
            name: self.name.clone(),
            chords: self
                .chords
                .iter()
                .map(crate::progressions::StoredChord::from_wire)
                .collect(),
        }
    }
}

#[cfg(test)]
mod editor_tests {
    use super::*;

    /// The editor's cell walk: roots wrap, qualities wrap, the bass stops
    /// at "off" below C.
    #[test]
    fn cells_turn_within_their_ranges() {
        let mut ed = ProgEditor::default();
        ed.open_for(0, &[], "", Vec::new());
        ed.adjust(-1);
        assert_eq!(ed.chords[0].root, 11, "the root should wrap under C");
        ed.move_col(1);
        ed.adjust(-2);
        assert_eq!(
            ed.chords[0].quality,
            (phosphor_core::midi_fx::QUALITIES.len() - 1) as u8,
            "the quality should wrap"
        );
        ed.move_col(1);
        ed.adjust(-5);
        assert_eq!(ed.chords[0].bass, -1, "the bass should stop at off");
        ed.adjust(3);
        assert_eq!(ed.chords[0].bass, 2);
    }

    /// Add copies the cursor's chord; remove keeps at least one; the cap
    /// is one chord per white key.
    #[test]
    fn add_and_remove_hold_the_shape() {
        let mut ed = ProgEditor::default();
        ed.open_for(0, &[phosphor_core::midi_fx::UserChord::pick(9, 5, -1)], "x", Vec::new());
        assert!(ed.add_chord());
        assert_eq!(ed.chords.len(), 2);
        assert_eq!(ed.chords[1].root, 9, "the new chord should copy the cursor's");
        for _ in 0..10 {
            ed.add_chord();
        }
        assert_eq!(ed.chords.len(), phosphor_core::midi_fx::MAX_USER_CHORDS, "the cap broke");
        while ed.remove_chord() {}
        assert_eq!(ed.chords.len(), 1, "the last chord must stay");
    }

    /// Learn: arm, play a spread chord, lift — the row becomes exactly
    /// that shape, root from the lowest key.
    #[test]
    fn learn_captures_the_hand() {
        let mut ed = ProgEditor::default();
        ed.open_for(0, &[], "", Vec::new());
        assert!(ed.toggle_learn());
        // A rolled Dm9 shape, wide: D3 C4 E4 F4 A4.
        for n in [50u8, 60, 64, 65, 69] {
            ed.learn_note_on(n);
        }
        // Lift in a different order; the chord commits on the last off.
        assert!(!ed.learn_note_off(60));
        assert!(!ed.learn_note_off(50));
        assert!(!ed.learn_note_off(65));
        assert!(!ed.learn_note_off(64));
        assert!(ed.learn_note_off(69), "the last lift should commit");
        assert!(!ed.learn_armed, "learn should disarm after a capture");
        let c = ed.chords[0];
        assert_eq!(c.quality, phosphor_core::midi_fx::LEARNED_QUALITY);
        assert_eq!(c.root, 2, "the root should be the lowest key's pitch class");
        assert_eq!(&c.custom[..5], &[0, 10, 14, 15, 19], "the spacing changed");
    }

    /// Turning the quality cell of a learned chord walks back into the
    /// dictionary instead of wrapping through 255.
    #[test]
    fn turning_a_learned_quality_returns_to_the_dictionary() {
        let mut ed = ProgEditor::default();
        ed.open_for(0, &[phosphor_core::midi_fx::UserChord::learned(0, &[0, 4, 7], -1)], "", Vec::new());
        ed.move_col(1);
        ed.adjust(1);
        assert!(
            usize::from(ed.chords[0].quality) < phosphor_core::midi_fx::QUALITIES.len(),
            "the quality should land in the dictionary, got {}",
            ed.chords[0].quality
        );
    }

    /// Browsing the library loads entries into the working copy.
    #[test]
    fn the_library_browses_into_the_editor() {
        let lib = vec![
            crate::progressions::UserProgression { name: "a".into(), chords: vec![crate::progressions::StoredChord::Tuple((0, 1, -1))] },
            crate::progressions::UserProgression {
                name: "b".into(),
                chords: vec![crate::progressions::StoredChord::Tuple((2, 5, -1)), crate::progressions::StoredChord::Tuple((7, 10, -1))],
            },
        ];
        let mut ed = ProgEditor::default();
        ed.open_for(0, &[], "", lib);
        // The cursor starts at 0; the first step lands on entry 1.
        assert_eq!(ed.cycle_library(1).as_deref(), Some("b"));
        assert_eq!(ed.chords.len(), 2);
        assert_eq!(ed.name, "b");
        assert_eq!(ed.cycle_library(1).as_deref(), Some("a"));
        assert_eq!(ed.chords.len(), 1);
    }
}
