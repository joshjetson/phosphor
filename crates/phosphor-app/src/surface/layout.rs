//! The Phosphor Deck, as data: every control, what kind it is, and the one
//! MIDI message it sends.
//!
//! One table, three readers. The app decodes what the deck sends through it,
//! the simulator the tests drive encodes through it, and the firmware is
//! generated from it ([`layout_json`]). A control added here exists on all
//! three at once, and a number can never mean one thing to the firmware and
//! another to the app.
//!
//! # The protocol
//!
//! Everything travels on MIDI channel 16, which a keyboard does not use by
//! default, so the deck can share a port with one without its buttons
//! playing notes:
//!
//! * a button or an encoder's push is a note: on (velocity 127) when it goes
//!   down, off when it comes up;
//! * an encoder turn is a control change carrying the detents turned, as a
//!   7-bit two's-complement number — 1 is one click clockwise, 127 one click
//!   back;
//! * a fader is a control change carrying where it sits, 0 to 127;
//! * a pad in STEP mode is a note with the velocity it was struck at. In
//!   PADS and NOTE modes the pads play ordinary notes on channel 1, exactly
//!   as a keyboard does, and never come through here.
//!
//! Numbers are handed out in table order — notes from 0, controllers from
//! 16 — so the table is the protocol and there is nothing else to keep in
//! step with it.

use std::collections::HashMap;
use std::sync::OnceLock;

use phosphor_midi::MidiMessageType;

/// MIDI channel 16, zero-based.
pub const DECK_CHANNEL: u8 = 15;

/// The first controller number the deck uses: below it are the bank select,
/// mod wheel and breath controllers a keyboard might send.
const FIRST_CC: u8 = 16;

/// Track strips on the deck, master excluded — one per track row the app
/// shows at once (`MAX_VISIBLE_TRACKS`).
pub const STRIPS: u8 = 5;

/// The master strip's fader index.
pub const MASTER_FADER: u8 = STRIPS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlId {
    // Transport
    Play,
    Stop,
    Rec,
    LoopRec,
    Loop,
    Click,
    Count,
    Panic,
    Tempo,
    // Navigate
    PaneTransport,
    PaneTracks,
    PaneClip,
    Value,
    Up,
    Down,
    Left,
    Right,
    Shift,
    Back,
    Tab,
    Menu,
    Enter,
    Save,
    // The encoder bank
    Bank(u8),
    PagePrev,
    PageNext,
    // Edit
    Undo,
    Redo,
    Copy,
    Paste,
    Delete,
    New,
    // Strips: `Track(n)` selects visible track `n`; `Fader(5)` is master.
    Track(u8),
    Master,
    Mute(u8),
    Solo(u8),
    Arm(u8),
    Fader(u8),
    // Pads
    Pad(u8),
    Lane(u8),
    ModeStep,
    ModePads,
    ModeNote,
    Accent,
    StepPage(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Button,
    /// Endless, relative, with a push switch.
    Encoder,
    /// Absolute position, 0..=127.
    Fader,
    /// Velocity-sensitive.
    Pad,
}

/// Where a control sits on the panel — the drawing's zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Transport,
    Navigate,
    Bank,
    Edit,
    Strips,
    Pads,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub id: ControlId,
    pub kind: Kind,
    pub zone: Zone,
    /// The note a button, pad or encoder push sends.
    pub note: Option<u8>,
    /// The controller an encoder turn or a fader sends.
    pub cc: Option<u8>,
}

impl Control {
    /// What the panel's silkscreen says.
    pub fn label(&self) -> String {
        use ControlId::*;
        match self.id {
            Bank(n) => format!("ENC {}", n + 1),
            Track(n) => format!("TRK {}", n + 1),
            Mute(n) => format!("M {}", n + 1),
            Solo(n) => format!("S {}", n + 1),
            Arm(n) => format!("R {}", n + 1),
            Fader(n) if n == MASTER_FADER => "FADER MSTR".into(),
            Fader(n) => format!("FADER {}", n + 1),
            Pad(n) => format!("PAD {}", n + 1),
            Lane(n) => format!("LANE {}", n + 1),
            StepPage(0) => "1-16".into(),
            StepPage(_) => "17-32".into(),
            other => format!("{other:?}").to_uppercase(),
        }
    }
}

/// The panel, in its fixed order. Changing the order renumbers the
/// protocol; the firmware is regenerated from the same table.
fn controls() -> Vec<(ControlId, Kind, Zone)> {
    use ControlId::*;
    use Kind::{Button, Encoder};
    let mut out = vec![
        (Play, Button, Zone::Transport),
        (Stop, Button, Zone::Transport),
        (Rec, Button, Zone::Transport),
        (LoopRec, Button, Zone::Transport),
        (Loop, Button, Zone::Transport),
        (Click, Button, Zone::Transport),
        (Count, Button, Zone::Transport),
        (Panic, Button, Zone::Transport),
        (Tempo, Encoder, Zone::Transport),
        (PaneTransport, Button, Zone::Navigate),
        (PaneTracks, Button, Zone::Navigate),
        (PaneClip, Button, Zone::Navigate),
        (Value, Encoder, Zone::Navigate),
        (Up, Button, Zone::Navigate),
        (Down, Button, Zone::Navigate),
        (Left, Button, Zone::Navigate),
        (Right, Button, Zone::Navigate),
        (Shift, Button, Zone::Navigate),
        (Back, Button, Zone::Navigate),
        (Tab, Button, Zone::Navigate),
        (Menu, Button, Zone::Navigate),
        (Enter, Button, Zone::Navigate),
        (Save, Button, Zone::Navigate),
    ];
    out.extend((0..8).map(|n| (Bank(n), Encoder, Zone::Bank)));
    out.extend([(PagePrev, Button, Zone::Bank), (PageNext, Button, Zone::Bank)]);
    out.extend([Undo, Redo, Copy, Paste, Delete, New].map(|id| (id, Button, Zone::Edit)));
    for n in 0..STRIPS {
        out.extend([(Track(n), Button, Zone::Strips), (Mute(n), Button, Zone::Strips)]);
        out.extend([(Solo(n), Button, Zone::Strips), (Arm(n), Button, Zone::Strips)]);
    }
    out.push((Master, Button, Zone::Strips));
    out.extend((0..=MASTER_FADER).map(|n| (Fader(n), Kind::Fader, Zone::Strips)));
    out.extend((0..16).map(|n| (Pad(n), Kind::Pad, Zone::Pads)));
    out.extend((0..8).map(|n| (Lane(n), Button, Zone::Pads)));
    out.extend([ModeStep, ModePads, ModeNote, Accent].map(|id| (id, Button, Zone::Pads)));
    out.extend([(StepPage(0), Button, Zone::Pads), (StepPage(1), Button, Zone::Pads)]);
    out
}

struct Table {
    controls: Vec<Control>,
    by_note: HashMap<u8, ControlId>,
    by_cc: HashMap<u8, ControlId>,
    by_id: HashMap<ControlId, usize>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let (mut note, mut cc) = (0u8, FIRST_CC);
        let mut controls = Vec::new();
        for (id, kind, zone) in self::controls() {
            let mut control = Control { id, kind, zone, note: None, cc: None };
            // An encoder has both: its turn is a controller, its push a note.
            if matches!(kind, Kind::Button | Kind::Pad | Kind::Encoder) {
                control.note = Some(note);
                note += 1;
            }
            if matches!(kind, Kind::Encoder | Kind::Fader) {
                control.cc = Some(cc);
                cc += 1;
            }
            controls.push(control);
        }
        let by_note = controls.iter().filter_map(|c| Some((c.note?, c.id))).collect();
        let by_cc = controls.iter().filter_map(|c| Some((c.cc?, c.id))).collect();
        let by_id = controls.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        Table { controls, by_note, by_cc, by_id }
    })
}

/// Every control on the deck, in panel order.
pub fn deck() -> &'static [Control] {
    &table().controls
}

/// One control by id. Panics on an id the panel does not have: a binding to
/// a control that does not exist is a bug in this module, not an input.
pub fn control(id: ControlId) -> &'static Control {
    &table().controls[table().by_id[&id]]
}

/// What the player did to the deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckInput {
    Press(ControlId),
    Release(ControlId),
    /// Detents turned; positive is clockwise.
    Turn(ControlId, i8),
    /// Where a fader now sits, 0..=127.
    Move(ControlId, u8),
    /// A pad struck in STEP mode, with its velocity.
    Hit(ControlId, u8),
}

/// The three bytes the deck sends for `input` — what the firmware does, and
/// what the simulator does in its place.
pub fn encode(input: DeckInput) -> [u8; 3] {
    let note_on = 0x90 | DECK_CHANNEL;
    let note_off = 0x80 | DECK_CHANNEL;
    let cc = 0xB0 | DECK_CHANNEL;
    let note_of = |id| control(id).note.expect("this control sends no note");
    let cc_of = |id| control(id).cc.expect("this control sends no controller");
    match input {
        DeckInput::Press(id) => [note_on, note_of(id), 127],
        DeckInput::Release(id) => [note_off, note_of(id), 0],
        DeckInput::Hit(id, velocity) => [note_on, note_of(id), velocity.clamp(1, 127)],
        DeckInput::Turn(id, detents) => [cc, cc_of(id), (detents.clamp(-63, 63) as u8) & 0x7F],
        DeckInput::Move(id, value) => [cc, cc_of(id), value.min(127)],
    }
}

/// What a message from the deck means, or `None` when it is not the deck's:
/// another channel, or a number the panel does not use.
pub fn decode(message: MidiMessageType) -> Option<DeckInput> {
    let t = table();
    match message {
        MidiMessageType::NoteOn { channel: DECK_CHANNEL, note, velocity } => {
            let id = *t.by_note.get(&note)?;
            Some(match (control(id).kind, velocity) {
                (_, 0) => DeckInput::Release(id),
                (Kind::Pad, v) => DeckInput::Hit(id, v),
                _ => DeckInput::Press(id),
            })
        }
        MidiMessageType::NoteOff { channel: DECK_CHANNEL, note, .. } => {
            Some(DeckInput::Release(*t.by_note.get(&note)?))
        }
        MidiMessageType::ControlChange { channel: DECK_CHANNEL, controller, value } => {
            let id = *t.by_cc.get(&controller)?;
            Some(match control(id).kind {
                // Two's complement in seven bits: 1..=63 forward, 65..=127 back.
                Kind::Encoder => DeckInput::Turn(id, if value >= 64 { (i16::from(value) - 128) as i8 } else { value as i8 }),
                _ => DeckInput::Move(id, value),
            })
        }
        _ => None,
    }
}

/// Whether a message belongs to the deck rather than to a keyboard — what
/// keeps a button press from reaching an instrument as a note.
pub fn is_deck(message: MidiMessageType) -> bool {
    decode(message).is_some()
}

/// The panel as JSON, for the firmware build: id, label, kind, zone and the
/// numbers it sends.
pub fn layout_json() -> String {
    let rows: Vec<serde_json::Value> = deck()
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": format!("{:?}", c.id),
                "label": c.label(),
                "kind": format!("{:?}", c.kind),
                "zone": format!("{:?}", c.zone),
                "note": c.note,
                "cc": c.cc,
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({ "channel": DECK_CHANNEL + 1, "controls": rows }))
        .expect("plain values serialise")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn parse(bytes: [u8; 3]) -> MidiMessageType {
        phosphor_midi::MidiMessage::from_bytes(&bytes).unwrap().message_type
    }

    #[test]
    fn every_control_has_numbers_nobody_else_has() {
        let notes: Vec<u8> = deck().iter().filter_map(|c| c.note).collect();
        let ccs: Vec<u8> = deck().iter().filter_map(|c| c.cc).collect();
        assert_eq!(notes.len(), notes.iter().collect::<HashSet<_>>().len(), "two controls share a note");
        assert_eq!(ccs.len(), ccs.iter().collect::<HashSet<_>>().len(), "two controls share a controller");
        assert!(notes.iter().all(|&n| n < 128) && ccs.iter().all(|&c| (FIRST_CC..120).contains(&c)));
        let ids: HashSet<_> = deck().iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), deck().len(), "a control is on the panel twice");
    }

    #[test]
    fn the_panel_matches_the_drawing() {
        let count = |kind| deck().iter().filter(|c| c.kind == kind).count();
        assert_eq!(count(Kind::Pad), 16);
        assert_eq!(count(Kind::Fader), 6, "five tracks and master");
        assert_eq!(count(Kind::Encoder), 10, "eight in the bank, VALUE and TEMPO");
    }

    /// The simulator and the app read the same table: whatever goes in comes
    /// out, for every control and every gesture it can make.
    #[test]
    fn every_gesture_survives_the_wire() {
        for c in deck() {
            let gestures: Vec<DeckInput> = match c.kind {
                Kind::Button => vec![DeckInput::Press(c.id), DeckInput::Release(c.id)],
                Kind::Pad => vec![DeckInput::Hit(c.id, 1), DeckInput::Hit(c.id, 127), DeckInput::Release(c.id)],
                Kind::Encoder => vec![
                    DeckInput::Turn(c.id, 1),
                    DeckInput::Turn(c.id, -1),
                    DeckInput::Turn(c.id, 63),
                    DeckInput::Turn(c.id, -63),
                    DeckInput::Press(c.id),
                ],
                Kind::Fader => vec![DeckInput::Move(c.id, 0), DeckInput::Move(c.id, 64), DeckInput::Move(c.id, 127)],
            };
            for g in gestures {
                assert_eq!(decode(parse(encode(g))), Some(g), "{g:?} changed on the wire");
            }
        }
    }

    /// A keyboard on channel 1 is never mistaken for the deck, even on the
    /// same note numbers.
    #[test]
    fn a_keyboard_is_not_the_deck() {
        assert!(!is_deck(parse([0x90, 0, 127])));
        assert!(!is_deck(parse([0xB0, FIRST_CC, 1])));
        assert!(is_deck(parse(encode(DeckInput::Press(ControlId::Play)))));
        assert_eq!(decode(parse([0x9F, 127, 127])), None, "a note the panel does not use");
    }

    #[test]
    fn the_firmware_gets_the_whole_panel() {
        let json: serde_json::Value = serde_json::from_str(&layout_json()).unwrap();
        assert_eq!(json["channel"], 16);
        assert_eq!(json["controls"].as_array().unwrap().len(), deck().len());
        assert_eq!(json["controls"][0]["id"], "Play");
    }
}
