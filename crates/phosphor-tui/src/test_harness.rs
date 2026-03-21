//! Test harness — drive the app with action sequences and assert state.
//!
//! Creates a headless App (no audio, no MIDI, no terminal) and lets tests
//! feed action sequences and inspect the resulting state.

#[cfg(test)]
pub mod tests {
    use crate::actions::Action;
    use crate::app::App;
    use crate::state::*;
    use phosphor_core::EngineConfig;

    /// A headless app for testing. No audio, no MIDI, no terminal rendering.
    pub struct TestApp {
        pub app: App,
    }

    impl TestApp {
        pub fn new() -> Self {
            let config = EngineConfig { buffer_size: 64, sample_rate: 44100 };
            Self {
                app: App::new(config, false, false),
            }
        }

        /// Execute a single action.
        pub fn do_action(&mut self, action: Action) {
            self.app.execute_action(action);
        }

        /// Execute a sequence of actions.
        pub fn do_actions(&mut self, actions: &[Action]) {
            for &action in actions {
                self.app.execute_action(action);
            }
        }

        // ── State accessors ──

        pub fn nav(&self) -> &NavState { &self.app.nav }
        pub fn is_playing(&self) -> bool { self.app.engine.transport.is_playing() }
        pub fn is_recording(&self) -> bool { self.app.engine.transport.is_recording() }
        pub fn is_looping(&self) -> bool { self.app.engine.transport.is_looping() }
        pub fn position_ticks(&self) -> i64 { self.app.engine.transport.position_ticks() }
        pub fn loop_start(&self) -> i64 { self.app.engine.transport.loop_start() }
        pub fn loop_end(&self) -> i64 { self.app.engine.transport.loop_end() }
        pub fn loop_editor(&self) -> &LoopEditor { &self.app.nav.loop_editor }
        pub fn track_count(&self) -> usize { self.app.nav.tracks.len() }
        pub fn is_running(&self) -> bool { self.app.running }
        pub fn focused_pane(&self) -> Pane { self.app.nav.focused_pane }
        pub fn space_menu_open(&self) -> bool { self.app.nav.space_menu.open }
    }

    // ══════════════════════════════════════════════
    // Scenarios
    // ══════════════════════════════════════════════

    // ── Loop editor ──

    #[test]
    fn scenario_set_loop_region_and_activate() {
        let mut t = TestApp::new();

        // Open loop editor
        t.do_action(Action::FocusLoopEditor);
        assert!(t.loop_editor().active);
        assert!(!t.loop_editor().enabled);
        assert!(!t.is_looping());

        // Default is bars 1-4
        assert_eq!(t.loop_editor().start_bar, 1);
        assert_eq!(t.loop_editor().end_bar, 5);

        // Move end marker left twice: 5→4→3 (display "1-2")
        t.do_action(Action::LoopEndLeft);
        t.do_action(Action::LoopEndLeft);
        assert_eq!(t.loop_editor().end_bar, 3);
        assert_eq!(t.loop_editor().display(), "1-2");

        // Activate the loop
        t.do_action(Action::LoopToggleEnabled);
        assert!(t.loop_editor().enabled);
        assert!(t.is_looping(), "Transport should be looping after Enter");

        // Verify transport has correct range
        assert_eq!(t.loop_start(), t.loop_editor().start_ticks());
        assert_eq!(t.loop_end(), t.loop_editor().end_ticks());

        // Unfocus
        t.do_action(Action::LoopUnfocus);
        assert!(!t.loop_editor().active);
        assert!(t.loop_editor().enabled, "Loop should stay enabled after unfocus");
        assert!(t.is_looping(), "Transport should still be looping");
    }

    #[test]
    fn scenario_play_with_loop_starts_at_loop_start() {
        let mut t = TestApp::new();

        // Set loop to bars 3-4 (move start right twice, end left once)
        // start: 1→2→3, end: 5→4  → start_bar=3 end_bar=4 → display "3-3" (bar 3 only)
        // For bars 3-4: start: 1→2→3, end stays at 5 → display "3-4"
        t.do_action(Action::FocusLoopEditor);
        t.do_action(Action::LoopStartRight); // 1→2
        t.do_action(Action::LoopStartRight); // 2→3
        // end stays at 5, so loop is bars 3-4 (display "3-4")
        t.do_action(Action::LoopToggleEnabled);
        t.do_action(Action::LoopUnfocus);

        assert_eq!(t.loop_editor().start_bar, 3);
        assert_eq!(t.loop_editor().end_bar, 5);
        assert_eq!(t.loop_editor().display(), "3-4");
        assert!(t.is_looping());

        // Play
        t.do_action(Action::PlayPause);
        assert!(t.is_playing());

        // Position should be at loop start (bar 3 = tick 7680)
        let expected_start = t.loop_editor().start_ticks();
        assert_eq!(t.position_ticks(), expected_start,
            "Playhead should start at loop start (bar 3)");
    }

    #[test]
    fn scenario_play_without_loop_starts_at_zero() {
        let mut t = TestApp::new();

        // Don't enable loop, just play
        t.do_action(Action::PlayPause);
        assert!(t.is_playing());
        assert!(!t.is_looping());
        assert_eq!(t.position_ticks(), 0);
    }

    #[test]
    fn scenario_loop_markers_cant_cross() {
        let mut t = TestApp::new();
        t.do_action(Action::FocusLoopEditor);

        // Try to move start past end
        for _ in 0..20 {
            t.do_action(Action::LoopStartRight);
        }
        assert!(t.loop_editor().start_bar < t.loop_editor().end_bar,
            "Start must be less than end");

        // Try to move end past start
        for _ in 0..20 {
            t.do_action(Action::LoopEndLeft);
        }
        assert!(t.loop_editor().end_bar > t.loop_editor().start_bar,
            "End must be greater than start");
    }

    #[test]
    fn scenario_loop_start_cant_go_below_1() {
        let mut t = TestApp::new();
        t.do_action(Action::FocusLoopEditor);
        for _ in 0..20 {
            t.do_action(Action::LoopStartLeft);
        }
        assert_eq!(t.loop_editor().start_bar, 1);
    }

    // ── Transport ──

    #[test]
    fn scenario_play_pause_toggle() {
        let mut t = TestApp::new();
        assert!(!t.is_playing());
        t.do_action(Action::PlayPause);
        assert!(t.is_playing());
        t.do_action(Action::PlayPause);
        assert!(!t.is_playing());
    }

    // ── Instrument tracks ──

    #[test]
    fn scenario_add_instrument_track() {
        let mut t = TestApp::new();
        let initial = t.track_count();

        // Open instrument modal and select
        t.do_action(Action::AddInstrument);
        assert!(t.nav().instrument_modal.open);
        t.do_action(Action::InstrumentSelect);

        assert_eq!(t.track_count(), initial + 1, "Should have one more track");
        assert!(!t.nav().instrument_modal.open);
    }

    // ── Loop editor + transport sync ──

    #[test]
    fn scenario_loop_range_syncs_to_transport_on_every_move() {
        let mut t = TestApp::new();
        t.do_action(Action::FocusLoopEditor);
        t.do_action(Action::LoopToggleEnabled);

        // Move start right
        t.do_action(Action::LoopStartRight);
        assert_eq!(t.loop_start(), t.loop_editor().start_ticks(),
            "Transport loop start should match editor after move");

        // Move end left
        t.do_action(Action::LoopEndLeft);
        assert_eq!(t.loop_end(), t.loop_editor().end_ticks(),
            "Transport loop end should match editor after move");
    }

    #[test]
    fn scenario_disable_loop_disables_transport_looping() {
        let mut t = TestApp::new();
        t.do_action(Action::FocusLoopEditor);
        t.do_action(Action::LoopToggleEnabled);
        assert!(t.is_looping());

        t.do_action(Action::LoopToggleEnabled);
        assert!(!t.is_looping(), "Transport should stop looping when loop disabled");
    }

    // ── Space menu ──

    #[test]
    fn scenario_space_menu_open_close() {
        let mut t = TestApp::new();
        t.do_action(Action::OpenSpaceMenu);
        assert!(t.space_menu_open());
        t.do_action(Action::CloseSpaceMenu);
        assert!(!t.space_menu_open());
    }

    // ── Edge cases ──

    #[test]
    fn scenario_quit() {
        let mut t = TestApp::new();
        assert!(t.is_running());
        t.do_action(Action::Quit);
        assert!(!t.is_running());
    }
}
