//! UI-side mirror of the MIDI-effect layer: what sits in a track's
//! pre-instrument slots, so the strip, the chain list, the session file and
//! the undo stack all read one place. The audio thread holds the real
//! effects; this is the front end's copy of their settings, edited through
//! the one path that also tells the mixer.

use phosphor_core::fx::FxParamInfo;
use phosphor_core::midi_fx::{
    ARP_PARAMS, CHORD_PARAMS, COLOR_LABELS, NOTE_NAMES, RATE_LABELS, SCALE_LABELS, STYLE_LABELS,
    VOICING_LABELS,
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
            _ => None,
        }
    }
}

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
