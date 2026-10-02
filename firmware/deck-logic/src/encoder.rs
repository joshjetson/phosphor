//! An encoder's two pins, A and B, as clicks.
//!
//! Turning the shaft walks the pins through a two-bit Gray code. Each step
//! along it is a quarter of a click on the EC11 (one click is one full cycle,
//! four steps); a step that skips a state is contact bounce and counts for
//! nothing. Clicks are counted from the rest position the encoder started
//! in, so the count lands as the shaft drops into each detent.
//!
//! Which way counts as clockwise depends on which pin is wired to A; the
//! deck flips every encoder at once if the board is built the other way.

/// The step each (previous, current) pair of states makes: +1, -1 or 0.
const STEPS: [i8; 16] = [0, -1, 1, 0, 1, 0, 0, -1, -1, 0, 0, 1, 0, 1, -1, 0];

#[derive(Debug, Clone, Copy, Default)]
pub struct Quadrature {
    state: u8,
    steps: i8,
}

impl Quadrature {
    pub const fn new(a: bool, b: bool) -> Self {
        Self { state: ((a as u8) << 1) | b as u8, steps: 0 }
    }

    /// One reading of the pins. Returns whole clicks completed, positive
    /// clockwise.
    pub fn update(&mut self, a: bool, b: bool) -> i8 {
        let now = ((a as u8) << 1) | b as u8;
        self.steps += STEPS[usize::from((self.state << 2) | now)];
        self.state = now;
        let clicks = self.steps / 4;
        self.steps -= clicks * 4;
        clicks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One click the table counts as clockwise: 11 → 01 → 00 → 10 → 11.
    const CW: [(bool, bool); 4] = [(false, true), (false, false), (true, false), (true, true)];

    #[test]
    fn a_full_cycle_is_one_click_either_way() {
        let mut q = Quadrature::new(true, true);
        let mut total = 0;
        for (a, b) in CW {
            total += q.update(a, b);
        }
        assert_eq!(total, 1);
        for (a, b) in CW.iter().rev().skip(1).chain([(true, true)].iter()) {
            total += q.update(*a, *b);
        }
        assert_eq!(total, 0, "the same cycle backwards is a click back");
    }

    #[test]
    fn bounce_at_rest_adds_nothing() {
        let mut q = Quadrature::new(true, true);
        let mut total = 0;
        for _ in 0..10 {
            total += q.update(true, false);
            total += q.update(true, true);
        }
        assert_eq!(total, 0);
    }
}
