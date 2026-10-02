//! A fader, or the expression pedal: a 12-bit reading as a 7-bit value.
//!
//! An analog reading never sits still; a fader resting on the line between
//! two values would send both, over and over. So a value changes only once
//! the reading is a few counts past the edge of the band the last value
//! came from.

/// Raw counts per 7-bit step.
const STEP: i32 = 32;
/// How far past a band's edge the reading must go before the value moves.
const HYSTERESIS: i32 = 6;

#[derive(Debug, Clone, Copy, Default)]
pub struct Fader {
    sent: Option<u8>,
}

impl Fader {
    /// One reading. Returns the new value when it has changed.
    pub fn update(&mut self, raw: u16) -> Option<u8> {
        let raw = i32::from(raw.min(4095));
        let target = (raw / STEP) as u8;
        if let Some(sent) = self.sent {
            if sent == target {
                return None;
            }
            let low = i32::from(sent) * STEP - HYSTERESIS;
            let high = (i32::from(sent) + 1) * STEP + HYSTERESIS - 1;
            if (low..=high).contains(&raw) {
                return None;
            }
        }
        self.sent = Some(target);
        Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_reach_zero_and_one_twenty_seven() {
        let mut f = Fader::default();
        assert_eq!(f.update(0), Some(0));
        assert_eq!(f.update(4095), Some(127));
    }

    #[test]
    fn jitter_across_a_band_edge_sends_once() {
        let mut f = Fader::default();
        assert_eq!(f.update(2047), Some(63));
        let mut sent = 0;
        for raw in [2048, 2046, 2049, 2050, 2045, 2051, 2047, 2052] {
            sent += f.update(raw).is_some() as u32;
        }
        assert_eq!(sent, 0, "a fader at rest on an edge is silent");
        assert_eq!(f.update(2060), Some(64), "a real move still goes out");
    }
}
