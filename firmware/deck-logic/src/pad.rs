//! A pad: a force sensing resistor read as a 12-bit voltage that rises
//! with pressure.
//!
//! A hit is the reading crossing `on`. Its velocity is the highest reading
//! in the `window` scans that follow, because a stick or a finger takes a
//! couple of milliseconds to load the pad fully: reading the first sample
//! would make every hit soft. It ends when the reading falls below `off`,
//! which sits below `on` so a pad held near the threshold does not flutter.

/// A pad's thresholds, in raw 12-bit readings, and its window in scans.
#[derive(Debug, Clone, Copy)]
pub struct PadConfig {
    pub on: u16,
    pub off: u16,
    /// The reading that counts as the hardest hit, velocity 127.
    pub full: u16,
    pub window: u8,
}

/// A starting point for an FSR 402 over 10 kΩ, scanned at 2 kHz: a 3 ms
/// window. Tune `on` and `full` by hand against the real pads.
pub const DEFAULT: PadConfig = PadConfig { on: 180, off: 100, full: 3000, window: 6 };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadEvent {
    /// Struck at this velocity.
    Hit(u8),
    /// Let go.
    Release,
    /// Struck and let go inside the window: both at once.
    Tap(u8),
}

#[derive(Debug, Clone, Copy, Default)]
enum State {
    #[default]
    Idle,
    Rising { scans: u8, peak: u16 },
    Down,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PadSensor {
    state: State,
}

impl PadSensor {
    pub fn update(&mut self, reading: u16, cfg: &PadConfig) -> Option<PadEvent> {
        match self.state {
            State::Idle => {
                if reading >= cfg.on {
                    self.state = State::Rising { scans: 1, peak: reading };
                }
                None
            }
            State::Rising { scans, peak } => {
                let peak = peak.max(reading);
                if reading < cfg.off {
                    self.state = State::Idle;
                    return Some(PadEvent::Tap(velocity(peak, cfg)));
                }
                if scans + 1 >= cfg.window {
                    self.state = State::Down;
                    return Some(PadEvent::Hit(velocity(peak, cfg)));
                }
                self.state = State::Rising { scans: scans + 1, peak };
                None
            }
            State::Down => {
                if reading < cfg.off {
                    self.state = State::Idle;
                    return Some(PadEvent::Release);
                }
                None
            }
        }
    }
}

/// A peak reading as a velocity, 1 to 127, on a square-root curve: an FSR's
/// voltage climbs fast for light touches and slowly for hard ones, and the
/// curve gives soft playing room without making a hard hit hard to reach.
pub fn velocity(peak: u16, cfg: &PadConfig) -> u8 {
    let range = u32::from(cfg.full.saturating_sub(cfg.on)).max(1);
    let x = (u32::from(peak.saturating_sub(cfg.on)).min(range) * 1024) / range;
    let curved = isqrt(x * 1024);
    (1 + curved * 126 / 1024) as u8
}

fn isqrt(n: u32) -> u32 {
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(readings: &[u16]) -> [Option<PadEvent>; 16] {
        let mut pad = PadSensor::default();
        let mut out = [None; 16];
        for (i, &r) in readings.iter().enumerate() {
            out[i] = pad.update(r, &DEFAULT);
        }
        out
    }

    #[test]
    fn the_velocity_is_the_peak_inside_the_window_not_the_first_sample() {
        let events = play(&[0, 400, 1200, 2600, 2200, 1800, 1500, 1500, 50]);
        let hit = events.iter().flatten().next().copied();
        assert_eq!(hit, Some(PadEvent::Hit(velocity(2600, &DEFAULT))));
        assert!(velocity(2600, &DEFAULT) > 100, "a hard hit is loud");
        assert_eq!(events.iter().flatten().last(), Some(&PadEvent::Release));
    }

    #[test]
    fn a_tap_shorter_than_the_window_still_plays() {
        let events = play(&[0, 600, 300, 20]);
        assert!(matches!(events.iter().flatten().next(), Some(PadEvent::Tap(_))));
    }

    #[test]
    fn velocity_spans_one_to_one_twenty_seven_and_never_goes_down() {
        assert_eq!(velocity(DEFAULT.on, &DEFAULT), 1);
        assert_eq!(velocity(4095, &DEFAULT), 127);
        let mut last = 0;
        for r in (DEFAULT.on..4095).step_by(7) {
            let v = velocity(r, &DEFAULT);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn noise_below_the_threshold_is_silence() {
        assert!(play(&[0, 90, 170, 120, 179, 60]).iter().all(Option::is_none));
    }
}
