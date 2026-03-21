//! Loop region editor — controls the loop start/end markers.
//!
//! When active, h/l move the left (start) marker.
//! Shift+h/l move the right (end) marker.
//! Both are clamped in bars and cannot cross each other.

use phosphor_core::transport::Transport;

/// One bar in ticks (4/4 time).
const TICKS_PER_BAR: i64 = Transport::PPQ * 4;

#[derive(Debug)]
pub struct LoopEditor {
    /// Whether the loop editor is active (controls are locked to it).
    pub active: bool,
    /// Start bar (1-based).
    pub start_bar: u32,
    /// End bar (1-based, exclusive — loop plays bars start..end).
    pub end_bar: u32,
}

impl Default for LoopEditor {
    fn default() -> Self { Self::new() }
}

impl LoopEditor {
    pub fn new() -> Self {
        Self {
            active: false,
            start_bar: 1,
            end_bar: 5, // default: 4 bars (1..5 means bars 1,2,3,4)
        }
    }

    /// Activate the editor (lock controls to loop markers).
    pub fn enter(&mut self) {
        self.active = true;
    }

    /// Deactivate the editor (release controls).
    pub fn escape(&mut self) {
        self.active = false;
    }

    /// Move the left (start) marker left by one bar.
    pub fn move_start_left(&mut self) {
        if self.start_bar > 1 {
            self.start_bar -= 1;
        }
    }

    /// Move the left (start) marker right by one bar.
    /// Cannot pass or equal the end marker.
    pub fn move_start_right(&mut self) {
        if self.start_bar + 1 < self.end_bar {
            self.start_bar += 1;
        }
    }

    /// Move the right (end) marker left by one bar.
    /// Cannot pass or equal the start marker.
    pub fn move_end_left(&mut self) {
        if self.end_bar > self.start_bar + 1 {
            self.end_bar -= 1;
        }
    }

    /// Move the right (end) marker right by one bar.
    pub fn move_end_right(&mut self) {
        self.end_bar += 1;
    }

    /// Start tick for the transport.
    pub fn start_ticks(&self) -> i64 {
        (self.start_bar as i64 - 1) * TICKS_PER_BAR
    }

    /// End tick for the transport.
    pub fn end_ticks(&self) -> i64 {
        self.end_bar as i64 * TICKS_PER_BAR
    }

    /// Number of bars in the loop region.
    pub fn bar_count(&self) -> u32 {
        self.end_bar - self.start_bar
    }

    /// Display string: "1-4" or "3-8" etc.
    pub fn display(&self) -> String {
        format!("{}-{}", self.start_bar, self.end_bar - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_4_bars() {
        let le = LoopEditor::new();
        assert_eq!(le.start_bar, 1);
        assert_eq!(le.end_bar, 5);
        assert_eq!(le.bar_count(), 4);
        assert_eq!(le.display(), "1-4");
    }

    #[test]
    fn start_cant_go_below_1() {
        let mut le = LoopEditor::new();
        le.move_start_left();
        le.move_start_left();
        le.move_start_left();
        assert_eq!(le.start_bar, 1);
    }

    #[test]
    fn start_cant_pass_end() {
        let mut le = LoopEditor::new();
        // end is 5, start should stop at 4
        for _ in 0..10 {
            le.move_start_right();
        }
        assert_eq!(le.start_bar, 4);
        assert!(le.start_bar < le.end_bar);
    }

    #[test]
    fn end_cant_pass_start() {
        let mut le = LoopEditor::new();
        // start is 1, end should stop at 2
        for _ in 0..10 {
            le.move_end_left();
        }
        assert_eq!(le.end_bar, 2);
        assert!(le.end_bar > le.start_bar);
    }

    #[test]
    fn end_can_grow() {
        let mut le = LoopEditor::new();
        le.move_end_right();
        le.move_end_right();
        assert_eq!(le.end_bar, 7);
        assert_eq!(le.display(), "1-6");
    }

    #[test]
    fn ticks_correct() {
        let le = LoopEditor::new();
        assert_eq!(le.start_ticks(), 0);
        assert_eq!(le.end_ticks(), 5 * TICKS_PER_BAR);
    }

    #[test]
    fn enter_escape() {
        let mut le = LoopEditor::new();
        assert!(!le.active);
        le.enter();
        assert!(le.active);
        le.escape();
        assert!(!le.active);
    }
}
