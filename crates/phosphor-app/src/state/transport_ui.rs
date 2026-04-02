//! Transport UI state — navigable elements in the transport bar.
//!
//! Pattern matches TrackElement: h/l navigates, Enter activates/locks, Esc releases.

/// Navigable elements in the transport bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportElement {
    Bpm,
    Record,
    Loop,
    Metronome,
}

impl TransportElement {
    pub fn move_right(self) -> Self {
        match self {
            Self::Bpm => Self::Record,
            Self::Record => Self::Loop,
            Self::Loop => Self::Metronome,
            Self::Metronome => Self::Metronome,
        }
    }

    pub fn move_left(self) -> Self {
        match self {
            Self::Bpm => Self::Bpm,
            Self::Record => Self::Bpm,
            Self::Loop => Self::Record,
            Self::Metronome => Self::Loop,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bpm => "bpm",
            Self::Record => "rec",
            Self::Loop => "loop",
            Self::Metronome => "met",
        }
    }
}

/// State for transport pane navigation.
#[derive(Debug)]
pub struct TransportUiState {
    /// Which element the cursor is on.
    pub element: TransportElement,
    /// Whether the current element is "entered" (controls locked to it).
    pub editing: bool,
}

impl Default for TransportUiState {
    fn default() -> Self { Self::new() }
}

impl TransportUiState {
    pub fn new() -> Self {
        Self {
            element: TransportElement::Bpm,
            editing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_navigation() {
        let e = TransportElement::Bpm;
        assert_eq!(e.move_right(), TransportElement::Record);
        assert_eq!(e.move_right().move_right(), TransportElement::Loop);
        assert_eq!(e.move_right().move_right().move_right(), TransportElement::Metronome);
        assert_eq!(TransportElement::Metronome.move_right(), TransportElement::Metronome);
    }

    #[test]
    fn element_left_navigation() {
        assert_eq!(TransportElement::Metronome.move_left(), TransportElement::Loop);
        assert_eq!(TransportElement::Loop.move_left(), TransportElement::Record);
        assert_eq!(TransportElement::Record.move_left(), TransportElement::Bpm);
        assert_eq!(TransportElement::Bpm.move_left(), TransportElement::Bpm);
    }
}
