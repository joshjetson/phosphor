//! A switch's contacts bounce for a few milliseconds when they close or
//! open. A level counts only once it has held for `settle` scans in a row.

#[derive(Debug, Clone, Copy)]
pub struct Debounce {
    stable: bool,
    count: u8,
}

impl Debounce {
    pub const fn new(level: bool) -> Self {
        Self { stable: level, count: 0 }
    }

    pub fn level(&self) -> bool {
        self.stable
    }

    /// One reading. Returns the new level when it has just settled.
    pub fn update(&mut self, raw: bool, settle: u8) -> Option<bool> {
        if raw == self.stable {
            self.count = 0;
            return None;
        }
        self.count += 1;
        if self.count >= settle {
            self.stable = raw;
            self.count = 0;
            return Some(raw);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bounce_shorter_than_the_settle_time_is_ignored() {
        let mut d = Debounce::new(false);
        for raw in [true, false, true, false, true, true] {
            assert_eq!(d.update(raw, 3), None);
        }
        assert_eq!(d.update(true, 3), Some(true), "three in a row settle");
        assert!(d.level());
    }
}
