//! A physical fader against a track's volume: the curve, and pickup.
//!
//! The app's fader moves in 1 dB steps from the floor to the top of its
//! travel, with one more step below the floor that is silence. A deck fader
//! is 128 positions; this module says which step each position is, so the
//! deck sets volume through the same stepped road the keys do (and inherits
//! its undo — a whole move is one step).
//!
//! **Pickup.** A fader that sits at the bottom while the track is at 0 dB
//! must not slam the track to silence when it is touched. It takes over only
//! once it reaches the track's own level — the soft takeover every hardware
//! mixer with non-motorised faders uses. Until then it moves without moving
//! anything.

/// Bottom of the app's travel above silence, dB. `TrackState::VOLUME_FLOOR_DB`.
pub const FLOOR_DB: i32 = -40;

/// Top of the travel, dB: `TrackConfig::MAX_VOLUME` (2.0, +6 dB) rounded to
/// the step.
pub const TOP_DB: i32 = 6;

/// The step a fader position asks for: `None` is silence (position 0), and
/// positions 1..=127 spread over the floor to the top, 1 dB apart.
pub fn position_db(position: u8) -> Option<i32> {
    match position.min(127) {
        0 => None,
        p => {
            let span = (TOP_DB - FLOOR_DB) as f32;
            Some(FLOOR_DB + ((p - 1) as f32 * span / 126.0).round() as i32)
        }
    }
}

/// How many 1 dB steps take `current` to `target` along the app's travel,
/// silence being the one step below the floor.
pub fn steps_between(current: Option<i32>, target: Option<i32>) -> i32 {
    let rung = |db: Option<i32>| db.map_or(FLOOR_DB - 1, |d| d.clamp(FLOOR_DB, TOP_DB));
    rung(target) - rung(current)
}

/// One fader's pickup state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pickup {
    /// Where the fader was last seen.
    last: Option<u8>,
    /// It has reached the track's level and is in control.
    engaged: bool,
}

impl Pickup {
    /// The fader moved to `position` while the track is at `current`. Returns
    /// the step it asks for once it is in control, `None` while it is still
    /// travelling towards the track's level.
    ///
    /// It engages when it lands on the track's step or passes through it —
    /// a fast move skips positions, and a fader that had to land exactly
    /// would be one that never caught.
    pub fn moved(&mut self, position: u8, current: Option<i32>) -> Option<Option<i32>> {
        let here = position_db(position);
        let rung = |db: Option<i32>| db.map_or(FLOOR_DB - 1, |d| d);
        if !self.engaged {
            let now = rung(here);
            let was = self.last.map(|p| rung(position_db(p)));
            let track = rung(current);
            let crossed = match was {
                Some(was) => (was.min(now)..=was.max(now)).contains(&track),
                None => now == track,
            };
            self.engaged = crossed;
        }
        self.last = Some(position);
        self.engaged.then_some(here)
    }

    /// The track the fader controls changed, or its volume was set by
    /// something else: the fader has to catch it again.
    pub fn release(&mut self) {
        self.engaged = false;
    }

    pub fn engaged(&self) -> bool {
        self.engaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_covers_silence_the_floor_and_the_top() {
        assert_eq!(position_db(0), None);
        assert_eq!(position_db(1), Some(FLOOR_DB));
        assert_eq!(position_db(127), Some(TOP_DB));
        let unity = (1..=127).find(|&p| position_db(p) == Some(0)).unwrap();
        assert!((105..=115).contains(&unity), "0 dB sits at {unity}, not near the top third");
        let all: Vec<_> = (1..=127).map(|p| position_db(p).unwrap()).collect();
        assert!(all.windows(2).all(|w| w[1] >= w[0]), "the curve runs backwards somewhere");
        assert_eq!(all.iter().collect::<std::collections::HashSet<_>>().len(), (TOP_DB - FLOOR_DB + 1) as usize, "a dB step cannot be reached");
    }

    #[test]
    fn steps_count_silence_as_the_rung_below_the_floor() {
        assert_eq!(steps_between(Some(0), Some(-3)), -3);
        assert_eq!(steps_between(None, Some(FLOOR_DB)), 1);
        assert_eq!(steps_between(Some(FLOOR_DB), None), -1);
        assert_eq!(steps_between(Some(0), Some(0)), 0);
    }

    /// The track is at 0 dB and the fader at the bottom: moving it up does
    /// nothing until it reaches 0 dB, then follows.
    #[test]
    fn a_fader_far_from_the_level_moves_nothing_until_it_gets_there() {
        let mut p = Pickup::default();
        assert_eq!(p.moved(0, Some(0)), None, "the first sighting at silence took a 0 dB track");
        assert_eq!(p.moved(60, Some(0)), None);
        assert!(!p.engaged());
        let caught = p.moved(115, Some(0)).expect("passing 0 dB did not catch");
        assert!(caught.unwrap() >= 0);
        assert!(p.engaged());
        assert_eq!(p.moved(100, Some(caught.unwrap())), Some(position_db(100)));
    }

    #[test]
    fn landing_on_the_level_catches_it_and_release_lets_go() {
        let mut p = Pickup::default();
        let unity = (1..=127).find(|&q| position_db(q) == Some(0)).unwrap();
        assert!(p.moved(unity, Some(0)).is_some());
        // A new track under the fader sits at -20 dB. Moving away from it
        // catches nothing; coming down through -20 catches it.
        p.release();
        assert_eq!(p.moved(127, Some(-20)), None, "a released fader still drove the track");
        assert!(p.moved(40, Some(-20)).is_some(), "passing -20 dB did not catch the new track");
    }

    #[test]
    fn a_silent_track_is_caught_at_the_bottom() {
        let mut p = Pickup::default();
        assert_eq!(p.moved(0, None), Some(None));
    }
}
