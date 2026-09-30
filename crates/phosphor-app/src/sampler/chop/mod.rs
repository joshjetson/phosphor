//! Chop: cut one recording into slices and lay them across the keys.
//!
//! A slice is not a copy. Every slice is a layer whose start and end window
//! into the one shared buffer, so a break cut across the whole bed costs the
//! break once — and every slice stays a normal layer afterwards, trimmed,
//! reversed and triggered like any other.
//!
//! Where the cuts go is decided three ways:
//!
//! * **transient** — wherever a hit starts, at a sensitivity the player
//!   turns. It listens through a [`Band`], which is how a chop cuts "at the
//!   kicks": the low band hears the kick and little else, so every slice
//!   runs from one kick to the next with everything in between. See
//!   [`onset`].
//! * **grid** — a loop that is N bars long, cut at a beat division. See
//!   [`grid`].
//! * **equal** — N slices of the same length, however the audio falls.
//!
//! All three produce [`markers::Marker`]s, and the player's own edits —
//! adding one, moving one — pin a marker so that turning the sensitivity
//! afterwards never throws the hand-work away. See [`markers`].
//!
//! Every frame in this module is a frame of the *buffer*, not of the region
//! being chopped: a slice lands as a layer whose start and end are buffer
//! frames, and one coordinate system is one fewer conversion to get wrong.

pub mod bands;
pub mod grid;
pub mod markers;
pub mod onset;

#[cfg(test)]
pub(crate) mod testkit;

/// What a transient chop listens to.
///
/// The recording is filtered before the hits are looked for, so the hits it
/// finds are the ones that band carries. The names say which drum each one
/// is for, because that is the question the player is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Band {
    /// The whole recording: every hit, whatever it is.
    #[default]
    Full,
    /// Under 100 Hz: the kick, and the bass notes.
    Low,
    /// 300 Hz to 5 kHz: the snare's crack and the clap — above the kick's
    /// body, below the hats.
    Mid,
    /// Over 7 kHz: the hats and the cymbals.
    High,
}
