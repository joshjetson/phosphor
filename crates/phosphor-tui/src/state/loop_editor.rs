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
    /// Whether the loop editor is focused (controls locked to markers).
    pub active: bool,
    /// Whether the loop is enabled (playhead loops within the region).
    pub enabled: bool,
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
            enabled: false,
            start_bar: 1,
            end_bar: 5,
        }
    }

    /// Focus the editor (lock controls to loop markers).
    pub fn focus(&mut self) {
        self.active = true;
    }

    /// Unfocus the editor (release controls).
    pub fn unfocus(&mut self) {
        self.active = false;
    }

    /// Toggle the loop on/off. Called when user presses Enter on the loop.
    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
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
    /// end_bar is exclusive, so bars 1-2 (end_bar=3) ends at the start of bar 3.
    pub fn end_ticks(&self) -> i64 {
        (self.end_bar as i64 - 1) * TICKS_PER_BAR
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
        // Default: bars 1-4 (start_bar=1, end_bar=5)
        assert_eq!(le.start_ticks(), 0);
        assert_eq!(le.end_ticks(), 4 * TICKS_PER_BAR); // 4 bars
    }

    #[test]
    fn ticks_for_two_bars() {
        let mut le = LoopEditor::new();
        // Move end to bar 3 (display "1-2" = bars 1 and 2)
        le.end_bar = 3;
        assert_eq!(le.display(), "1-2");
        assert_eq!(le.start_ticks(), 0);
        assert_eq!(le.end_ticks(), 2 * TICKS_PER_BAR); // 2 bars exactly
    }

    #[test]
    fn focus_unfocus() {
        let mut le = LoopEditor::new();
        assert!(!le.active);
        le.focus();
        assert!(le.active);
        le.unfocus();
        assert!(!le.active);
    }

    #[test]
    fn enabled_toggle() {
        let mut le = LoopEditor::new();
        assert!(!le.enabled);
        le.toggle_enabled();
        assert!(le.enabled);
        le.toggle_enabled();
        assert!(!le.enabled);
    }

    #[test]
    fn focus_does_not_enable() {
        let mut le = LoopEditor::new();
        le.focus();
        assert!(le.active);
        assert!(!le.enabled);
    }
}
