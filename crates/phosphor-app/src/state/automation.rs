//! The automation lane's model: which controller streams a clip holds, what
//! value a stream has at a moment, and the step-edits the lane makes.
//!
//! A stream is one controller's worth of events — the mod wheel, the pitch
//! bend, the aftertouch, or any other control-change number that was
//! recorded. The lane edits in *columns*, the same grid the piano roll
//! walks: setting a column writes one event at the column's start and clears
//! whatever the stream held inside the column, so drawn automation is steps
//! on the grid while recorded automation keeps its full density until the
//! player draws over it.

use phosphor_core::clip::ClipEvent;

use super::Clip;

/// One controller stream, as the lane names and edits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoStream {
    /// A control change, by controller number. Number 1 is the mod wheel.
    Cc(u8),
    /// Pitch bend. Edited and drawn by its coarse byte, 64 = centre.
    Bend,
    /// Channel pressure.
    Pressure,
}

impl AutoStream {
    /// The three a controller always has under the hand, in the order the
    /// lane offers them.
    pub const BASICS: [Self; 3] = [Self::Cc(1), Self::Bend, Self::Pressure];

    /// What the lane's header calls it.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Cc(1) => "mod".to_string(),
            Self::Cc(n) => format!("cc{n}"),
            Self::Bend => "bend".to_string(),
            Self::Pressure => "press".to_string(),
        }
    }

    /// Whether this event belongs to the stream.
    #[must_use]
    pub fn owns(self, event: &ClipEvent) -> bool {
        match self {
            Self::Cc(n) => event.status &