//! Automation lane: drawing, reshaping and erasing the controller curves
//! that recording captures. The lane shares the piano roll's column grid,
//! so these drive the same column cursor the note grid uses.

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::state::*;
    use phosphor_core::clip::{ClipEvent, NoteSnapshot};
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;

    const BAR: i64 = Transport::PPQ * 4;

    fn app() -> App {
        App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false)
    }

    /// A synth track with a one-bar clip in view and a 4-column grid.
    fn app_with_clip() -> (App, usize) {
        let mut app = app();
        app.create_instrument_track(InstrumentType::Synth);
        let ti = app.nav.track_cursor;
        app.nav.tracks[ti].clips.push(Clip {
            number: 1, width: 4, has_content: true,
            start_tick: 0, length_ticks: BAR,
            notes: vec![NoteSnapshot { note: 60, velocity: 100, start_tick: 0, duration_ticks: 960, muted: false }],
            hidden_notes: Vec::new(),
            controls: Vec::new(),
        });
        app.nav.open_clip_view(ti, 0);
        app.nav.clip_view.piano_roll.grid = GridResolution::Quarter;
        app.nav.clip_view.piano_roll.total_beats = 4;
        app.nav.clip_view.piano_roll.update_column_count();
        (app, ti)
    }

    fn mod_events(app: &App, ti: usize) -> Vec<ClipEvent> {
        app.nav.tracks[ti].clips[0]
            .controls
            .iter()
            .filter(|e| e.status & 0xF0 == 0xB0 && e.data1 == 1)
            .copied()
            .collect()
    }

    fn status(app: &App) -> String {
        app.live_status().unwrap_or_default().to_string()
    }

    // ── Opening and focus ──

    /// A opens the lane and gives it the keys; A again hands them back to
    /// the note grid; Esc closes it.
    #[test]
    fn the_lane_opens_focuses_and_closes() {
        let (mut app, _) = app_with_clip();
        assert!(!app.nav.clip_view.piano_roll.automation_open);

        app.toggle_automation_lane();
        assert!(app.nav.clip_view.piano_roll.automation_open, "A did not open the lane");
        assert!(app.nav.clip_view.piano_roll.automation_focus, "A did not focus the lane");

        app.toggle_automation_lane();
        assert!(app.nav.clip_view.piano_roll.automation_open, "the second A closed the lane");
        assert!(!app.nav.clip_view.piano_roll.automation_focus, "the second A kept the keys");

        app.nav.clip_view.piano_roll.automation_focus = true;
        app.close_automation_lane();
        assert!(!app.nav.clip_view.piano_roll.automation_open, "Esc did not close the lane");
    }

    /// A clip with no recording still offers the default streams, mod first.
    #[test]
    fn an_empty_clip_offers_the_default_streams() {
        let (app, _) = app_with_clip();
        let streams = app.automation_streams();
        assert_eq!(streams.first().map(|s| s.label()), Some("mod".to_string()));
        assert!(streams.iter().any(|s| s.label() == "bend"));
        assert!(streams.iter().any(|s| s.label() == "aftertouch"));
    }

    // ── Drawing ──

    /// k raises the curve at the cursor column and writes a mod event there;
    /// the value reaches the audio thread.
    #[test]
    fn drawing_writes_a_point_and_sounds() {
        let (mut app, ti) = app_with_clip();
        app.toggle_automation_lane();
        let _ = app.drain_mixer_commands();

        app.automation_draw(20); // one press of k (coarse step)
        let events = mod_events(&app, ti);
        assert_eq!(events.len(), 1, "the draw wrote no point");
        assert_eq!(events[0].tick, 0, "the point landed off the cursor column");
        assert!(events[0].data2 > 0, "the point has no value");

        // The rebuilt clip reached the audio thread carrying the controller.
        let commands = app.drain_mixer_commands();
        let sent = commands.iter().any(|c| matches!(
            c,
            MixerCommand::UpdateClip { events, .. }
                if events.iter().any(|e| e.status & 0xF0 == 0xB0)
        ));
        assert!(sent, "the drawn point never reached the audio thread");
    }

    /// A ramp: draw in one column, walk right, draw higher — two points at
    /// two columns, rising, and the value carries between them.
    #[test]
    fn a_ramp_across_columns_keeps_rising() {
        let (mut app, ti) = app_with_clip();
        app.toggle_automation_lane();

        app.automation_draw(40);                    // column 0
        app.nav.clip_view.piano_roll.move_column_right();
        app.automation_draw(40);                    // column 1, from the carried value
        app.nav.clip_view.piano_roll.move_column_right();
        app.automation_draw(40);                    // column 2

        let events = mod_events(&app, ti);
        assert_eq!(events.len(), 3, "a point per column did not land: {events:?}");
        let vals: Vec<u8> = events.iter().map(|e| e.data2).collect();
        assert!(vals[0] < vals[1] && vals[1] < vals[2], "the ramp did not rise: {vals:?}");
    }

    /// j after k on the same column lowers the very point just drawn, rather
    /// than starting a second one.
    #[test]
    fn up_then_down_reshapes_one_point() {
        let (mut app, ti) = app_with_clip();
        app.toggle_automation_lane();
        app.automation_draw(40);
        let high = mod_events(&app, ti)[0].data2;
        app.automation_draw(-20);
        let events = mod_events(&app, ti);
        assert_eq!(events.len(), 1, "the down-press started a second point");
        assert!(events[0].data2 < high, "the point did not come down");
    }

    /// A whole draw sweep is one undo step — u lifts the entire curve, not
    /// one column of it.
    #[test]
    fn a_draw_sweep_is_one_undo_step() {
        let (mut app, ti) = app_with_clip();
        app.toggle_automation_lane();
        app.automation_draw(40);
        app.nav.clip_view.piano_roll.move_column_right();
        app.automation_draw(40);
        app.nav.clip_view.piano_roll.move_column_right();
        app.automation_draw(40);
        assert_eq!(mod_events(&app, ti).len(), 3);

        app.perform_undo();
        assert_eq!(mod_events(&app, ti).len(), 0, "one undo did not lift the whole sweep");
    }

    /// d clears the stream's point in the cursor column, and undo brings it
    /// back.
    #[test]
    fn clearing_a_point_undoes() {
        let (mut app, ti) = app_with_clip();
        app.toggle_automation_lane();
        app.automation_draw(40);
        assert_eq!(mod_events(&app, ti).len(), 1);

        app.automation_clear_point();
        assert_eq!(mod_events(&app, ti).len(), 0, "d did not clear the point");

        app.perform_undo();
        assert_eq!(mod_events(&app, ti).len(), 1, "undo did not restore the cleared point");
    }

    /// The lane cycles between the streams the clip offers.
    #[test]
    fn cycling_moves_between_streams() {
        let (mut app, _) = app_with_clip();
        app.toggle_automation_lane();
        let first = app.current_automation_stream().unwrap().label();
        app.automation_cycle_stream(1);
        let second = app.current_automation_stream().unwrap().label();
        assert_ne!(first, second, "cycling did not change the stream");
        assert!(status(&app).contains(&second), "the lane change was not announced");
    }

    /// Drawing on a chosen stream writes that stream, not the mod wheel.
    #[test]
    fn drawing_writes_the_selected_stream() {
        let (mut app, ti) = app_with_clip();
        app.toggle_automation_lane();
        // Move off mod (index 0) to the bend stream.
        while app.current_automation_stream().map(|s| s.label()) != Some("bend".to_string()) {
            app.automation_cycle_stream(1);
        }
        app.automation_draw(40);
        let clip = &app.nav.tracks[ti].clips[0];
        assert!(clip.controls.iter().any(|e| e.status & 0xF0 == 0xE0), "no bend event written");
        assert!(!clip.controls.iter().any(|e| e.status & 0xF0 == 0xB0), "a mod event leaked in");
    }

    /// A recorded sweep is what the lane opens on — its stream comes first,
    /// and its values are what the columns read.
    #[test]
    fn the_lane_shows_a_recorded_sweep() {
        let (mut app, ti) = app_with_clip();
        app.nav.tracks[ti].clips[0].controls = vec![
            ClipEvent { tick: 0, status: 0xB0, data1: 1, data2: 30 },
            ClipEvent { tick: BAR / 2, status: 0xB0, data1: 1, data2: 100 },
        ];
        let stream = app.current_automation_stream().expect("a stream to show");
        assert_eq!(stream.label(), "mod", "the recorded stream was not first");
        let clip = &app.nav.tracks[ti].clips[0];
        assert_eq!(clip.control_value_at_column(stream, 0, 4), Some(30));
        assert_eq!(clip.control_value_at_column(stream, 3, 4), Some(100));
    }

    /// r lays the line in one undo step: draw two ends, ramp, and a single
    /// u removes the whole line while the hand-drawn ends survive.
    #[test]
    fn a_ramp_is_one_undo_step() {
        let (mut app, ti) = app_with_clip();
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        app.nav.clip_view.piano_roll.grid = GridResolution::Sixteenth;
        app.nav.clip_view.piano_roll.update_column_count();
        let cols = app.nav.clip_view.piano_roll.column_count.max(1);
        app.toggle_automation_lane();

        app.nav.tracks[ti].clips[0].set_control_point(mod_wheel, 1, cols, 10);
        app.nav.tracks[ti].clips[0].set_control_point(mod_wheel, 9, cols, 90);
        app.nav.clip_view.piano_roll.column = 9;
        app.automation_ramp();
        assert_eq!(mod_events(&app, ti).len(), 9, "seven fills plus two ends");

        app.perform_undo();
        assert_eq!(
            mod_events(&app, ti).len(),
            2,
            "one undo should remove exactly the ramp's fill"
        );

        // And with a missing end the ramp changes nothing at all.
        app.nav.clip_view.piano_roll.column = 5;
        app.automation_ramp();
        assert_eq!(mod_events(&app, ti).len(), 2, "a refused ramp still wrote points");
    }
}
