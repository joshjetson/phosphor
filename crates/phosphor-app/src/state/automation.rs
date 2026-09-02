//! The automation lane's model — the controller streams a clip holds, the
//! value a stream shows in a column, and the step-edits the lane makes.
//!
//! Lifted out of `track.rs` so that file stays about the track and the clip,
//! not about controllers. [`AutomationStream`] and the `Clip` methods here
//! are the whole model the lane's UI drives; the lane itself lives in
//! `phosphor-tui`.

use super::Clip;

/// One controller's worth of automation — the thing an automation lane
/// draws and edits. A stream is a *kind* of controller (mod wheel, pitch
/// bend, aftertouch), identified the same way [`phosphor_core::clip`]'s
/// commit thins one: control change is one stream per controller number,
/// while pitch bend and channel pressure are one stream each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomationStream {
    /// `0xB0` control change, `0xE0` pitch bend, `0xD0` channel pressure.
    pub kind: u8,
    /// The controller number, for `0xB0`. Ignored for the others.
    pub cc: u8,
}

impl AutomationStream {
    /// The three streams a lane always offers, so a player can draw a
    /// wheel or a bend onto a clip that has none recorded. Mod wheel first,
    /// because it is the one hands reach for.
    pub const DEFAULTS: [AutomationStream; 3] = [
        AutomationStream { kind: 0xB0, cc: 1 },
        AutomationStream { kind: 0xE0, cc: 0 },
        AutomationStream { kind: 0xD0, cc: 0 },
    ];

    #[must_use]
    pub fn label(self) -> String {
        match self.kind {
            0xE0 => "bend".to_string(),
            0xD0 => "aftertouch".to_string(),
            0xB0 => match self.cc {
                1 => "mod".to_string(),
                7 => "volume".to_string(),
                11 => "expression".to_string(),
                64 => "sustain".to_string(),
                74 => "cutoff".to_string(),
                n => format!("cc{n}"),
            },
            _ => "?".to_string(),
        }
    }

    /// Whether an event belongs to this stream.
    #[must_use]
    pub fn matches(self, e: &phosphor_core::clip::ClipEvent) -> bool {
        e.status & 0xF0 == self.kind && (self.kind != 0xB0 || e.data1 == self.cc)
    }

    /// The 0..=127 value an event carries, for drawing and editing. Pitch
    /// bend is 14-bit; a lane shows and edits its coarse top seven bits,
    /// which is all a hand-drawn curve needs — a recorded bend keeps its
    /// full resolution right up until the moment a point of it is edited.
    #[must_use]
    pub fn value_of(self, e: &phosphor_core::clip::ClipEvent) -> u8 {
        match self.kind {
            0xE0 => (((e.data1 as u16) | ((e.data2 as u16) << 7)) >> 7) as u8,
            0xD0 => e.data1,
            _ => e.data2,
        }
    }

    /// Build an event of this stream at `tick` from a 0..=127 value.
    #[must_use]
    pub fn event_at(self, tick: i64, value: u8) -> phosphor_core::clip::ClipEvent {
        let v = value & 0x7F;
        match self.kind {
            0xE0 => phosphor_core::clip::ClipEvent { tick, status: 0xE0, data1: 0, data2: v },
            0xD0 => phosphor_core::clip::ClipEvent { tick, status: 0xD0, data1: v, data2: 0 },
            _ => phosphor_core::clip::ClipEvent { tick, status: 0xB0, data1: self.cc, data2: v },
        }
    }
}

impl Clip {
    /// The controller streams this clip offers a lane: every one it has
    /// recorded, in first-seen order, then any of the default three it does
    /// not already have — so the lane always has something to draw onto,
    /// recording or not.
    #[must_use]
    pub fn control_streams(&self) -> Vec<AutomationStream> {
        let mut streams: Vec<AutomationStream> = Vec::new();
        for e in &self.controls {
            let s = AutomationStream {
                kind: e.status & 0xF0,
                cc: if e.status & 0xF0 == 0xB0 { e.data1 } else { 0 },
            };
            if !streams.contains(&s) {
                streams.push(s);
            }
        }
        for &s in &AutomationStream::DEFAULTS {
            if !streams.contains(&s) {
                streams.push(s);
            }
        }
        streams
    }

    /// The tick a grid column starts on, given how many columns the grid
    /// has. The one place column and tick meet for automation, so the lane
    /// and the note grid above it agree to the tick.
    #[must_use]
    pub fn column_tick(&self, col: usize, col_count: usize) -> i64 {
        if col_count == 0 { return 0; }
        (col as i64 * self.length_ticks) / col_count as i64
    }

    /// The value `stream` holds during column `col`: the last event of the
    /// stream at or before the column's end, or `None` when nothing has set
    /// it yet. A value holds until the next event, which is how MIDI
    /// controllers behave and so how the lane draws them.
    #[must_use]
    pub fn control_value_at_column(
        &self,
        stream: AutomationStream,
        col: usize,
        col_count: usize,
    ) -> Option<u8> {
        let until = self.column_tick(col + 1, col_count);
        self.controls
            .iter()
            .filter(|e| stream.matches(e) && e.tick < until)
            .max_by_key(|e| e.tick)
            .map(|e| stream.value_of(e))
    }

    /// Set a control point for `stream` in column `col`: clear whatever of
    /// the stream sat in the column already, then place one event at the
    /// column's start tick. Returns whether anything changed.
    pub fn set_control_point(
        &mut self,
        stream: AutomationStream,
        col: usize,
        col_count: usize,
        value: u8,
    ) -> bool {
        let start = self.column_tick(col, col_count);
        let end = self.column_tick(col + 1, col_count);
        let before = self.controls.len();
        self.controls
            .retain(|e| !(stream.matches(e) && e.tick >= start && e.tick < end));
        self.controls.push(stream.event_at(start, value));
        self.controls.sort_by_key(|e| e.tick);
        before != self.controls.len() || true
    }

    /// Remove `stream`'s events from column `col`. Returns whether any went.
    pub fn clear_control_point(
        &mut self,
        stream: AutomationStream,
        col: usize,
        col_count: usize,
    ) -> bool {
        let start = self.column_tick(col, col_count);
        let end = self.column_tick(col + 1, col_count);
        let before = self.controls.len();
        self.controls
            .retain(|e| !(stream.matches(e) && e.tick >= start && e.tick < end));
        before != self.controls.len()
    }

    /// Lay a straight line from the stream's previous point up to the
    /// cursor column: every column strictly between them gets a linearly
    /// interpolated point. The cursor column must hold a point of its own —
    /// the ramp's far end is drawn first, then the line is pulled back to
    /// the point before it. Returns how many columns were written, or
    /// `None` when an end is missing.
    pub fn ramp_control_to(
        &mut self,
        stream: AutomationStream,
        col: usize,
        col_count: usize,
    ) -> Option<usize> {
        let start = self.column_tick(col, col_count);
        let end = self.column_tick(col + 1, col_count);
        let to_value = self
            .controls
            .iter()
            .find(|e| stream.matches(e) && e.tick >= start && e.tick < end)
            .map(|e| stream.value_of(e))?;
        let prev = self
            .controls
            .iter()
            .filter(|e| stream.matches(e) && e.tick < start)
            .max_by_key(|e| e.tick)?;
        let (prev_tick, from_value) = (prev.tick, stream.value_of(prev));
        let prev_col = ((prev_tick * col_count as i64) / self.length_ticks.max(1)) as usize;
        if prev_col + 1 >= col {
            return Some(0);
        }
        let span = (col - prev_col) as f64;
        for c in prev_col + 1..col {
            let t = (c - prev_col) as f64 / span;
            let value =
                (from_value as f64 + (to_value as f64 - from_value as f64) * t).round() as u8;
            self.set_control_point(stream, c, col_count, value);
        }
        Some(col - prev_col - 1)
    }
}


#[cfg(test)]
mod automation_tests {
    use super::*;
    use phosphor_core::clip::ClipEvent;
    use phosphor_core::transport::Transport;

    const BAR: i64 = Transport::PPQ * 4;

    fn clip_with(controls: Vec<ClipEvent>) -> Clip {
        Clip {
            number: 1,
            width: 4,
            has_content: true,
            start_tick: 0,
            length_ticks: BAR,
            notes: Vec::new(),
            hidden_notes: Vec::new(),
            controls,
        }
    }

    fn cc(tick: i64, num: u8, val: u8) -> ClipEvent {
        ClipEvent { tick, status: 0xB0, data1: num, data2: val }
    }

    /// A clip lists the streams it recorded, then the defaults it lacks, so
    /// the lane always has mod / bend / aftertouch to draw onto.
    #[test]
    fn streams_are_recorded_first_then_the_defaults() {
        let clip = clip_with(vec![cc(0, 7, 100)]); // volume, not a default
        let streams = clip.control_streams();
        assert_eq!(streams[0], AutomationStream { kind: 0xB0, cc: 7 }, "recorded stream lost its place at the front");
        // The three defaults follow, none duplicated.
        for d in AutomationStream::DEFAULTS {
            assert!(streams.contains(&d), "default {d:?} missing");
        }
        assert_eq!(streams.len(), 4, "a stream was duplicated: {streams:?}");
    }

    /// A value holds from its event until the next one, the way a MIDI
    /// controller does — so a column past the last point reads that point.
    #[test]
    fn a_value_holds_across_columns_until_the_next() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let clip = clip_with(vec![
            ClipEvent { tick: 0, status: 0xB0, data1: 1, data2: 20 },
            ClipEvent { tick: BAR / 2, status: 0xB0, data1: 1, data2: 100 },
        ]);
        let cols = 4;
        assert_eq!(clip.control_value_at_column(mod_wheel, 0, cols), Some(20));
        assert_eq!(clip.control_value_at_column(mod_wheel, 1, cols), Some(20), "value did not hold");
        assert_eq!(clip.control_value_at_column(mod_wheel, 2, cols), Some(100), "second point not seen");
        assert_eq!(clip.control_value_at_column(mod_wheel, 3, cols), Some(100), "value did not hold to the end");
    }

    /// Drawing a point replaces whatever of the stream sat in that column,
    /// and leaves other columns and other streams alone.
    #[test]
    fn a_drawn_point_replaces_its_column_only() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let mut clip = clip_with(vec![
            cc(0, 1, 30),         // column 0, mod
            cc(BAR / 4, 1, 40),   // column 1, mod — to be overwritten
            cc(BAR / 4, 7, 90),   // column 1, volume — must survive
        ]);
        let cols = 4;
        clip.set_control_point(mod_wheel, 1, cols, 110);
        assert_eq!(clip.control_value_at_column(mod_wheel, 0, cols), Some(30), "column 0 was disturbed");
        assert_eq!(clip.control_value_at_column(mod_wheel, 1, cols), Some(110), "the point did not land");
        let volume = AutomationStream { kind: 0xB0, cc: 7 };
        assert_eq!(clip.control_value_at_column(volume, 1, cols), Some(90), "a different stream was overwritten");
    }

    /// Clearing a column removes only that stream's events there.
    #[test]
    fn clearing_a_column_takes_only_its_stream() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let volume = AutomationStream { kind: 0xB0, cc: 7 };
        let mut clip = clip_with(vec![cc(0, 1, 30), cc(0, 7, 90)]);
        assert!(clip.clear_control_point(mod_wheel, 0, 4));
        assert_eq!(clip.control_value_at_column(mod_wheel, 0, 4), None, "the mod point survived");
        assert_eq!(clip.control_value_at_column(volume, 0, 4), Some(90), "the volume point was taken too");
        assert!(!clip.clear_control_point(mod_wheel, 0, 4), "clearing an empty column claimed a change");
    }

    /// Pitch bend round-trips its coarse value through an edit.
    #[test]
    fn bend_edits_round_trip_coarse() {
        let bend = AutomationStream { kind: 0xE0, cc: 0 };
        let e = bend.event_at(0, 96);
        assert_eq!(e.status, 0xE0);
        assert_eq!(bend.value_of(&e), 96, "the bend value did not survive the round trip");
    }

    /// r pulls a straight line back from the cursor's point to the previous
    /// one: every column between them gets an interpolated point, and the
    /// endpoints themselves are left exactly as drawn.
    #[test]
    fn a_ramp_fills_the_columns_between_two_points() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let cols = 16;
        let mut clip = clip_with(Vec::new());
        clip.set_control_point(mod_wheel, 2, cols, 20);
        clip.set_control_point(mod_wheel, 10, cols, 100);
        let written = clip.ramp_control_to(mod_wheel, 10, cols);
        assert_eq!(written, Some(7), "eight columns apart leaves seven to fill");
        let values: Vec<u8> = (2..=10)
            .map(|c| clip.control_value_at_column(mod_wheel, c, cols).unwrap())
            .collect();
        assert_eq!(values[0], 20, "the ramp moved its own start point");
        assert_eq!(values[8], 100, "the ramp moved its own end point");
        for w in values.windows(2) {
            assert!(w[1] > w[0], "the ramp is not strictly rising: {values:?}");
        }
    }

    /// A ramp downhill interpolates just as well as one uphill.
    #[test]
    fn a_ramp_can_fall() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let cols = 16;
        let mut clip = clip_with(Vec::new());
        clip.set_control_point(mod_wheel, 0, cols, 120);
        clip.set_control_point(mod_wheel, 8, cols, 0);
        assert_eq!(clip.ramp_control_to(mod_wheel, 8, cols), Some(7));
        let mid = clip.control_value_at_column(mod_wheel, 4, cols).unwrap();
        assert!((55..=65).contains(&mid), "midpoint of 120..0 should be near 60, got {mid}");
    }

    /// Both ends must exist: no point under the cursor, or nothing to the
    /// left, and the ramp refuses instead of inventing an endpoint.
    #[test]
    fn a_ramp_refuses_when_an_end_is_missing() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let cols = 16;
        let mut clip = clip_with(Vec::new());
        clip.set_control_point(mod_wheel, 2, cols, 20);
        assert_eq!(clip.ramp_control_to(mod_wheel, 10, cols), None, "no point at the cursor");
        let mut clip = clip_with(Vec::new());
        clip.set_control_point(mod_wheel, 10, cols, 100);
        assert_eq!(clip.ramp_control_to(mod_wheel, 10, cols), None, "no point to the left");
    }

    /// Adjacent points have nothing between them; the ramp says so rather
    /// than doing something.
    #[test]
    fn a_ramp_between_neighbours_writes_nothing() {
        let mod_wheel = AutomationStream { kind: 0xB0, cc: 1 };
        let cols = 16;
        let mut clip = clip_with(Vec::new());
        clip.set_control_point(mod_wheel, 4, cols, 20);
        clip.set_control_point(mod_wheel, 5, cols, 100);
        assert_eq!(clip.ramp_control_to(mod_wheel, 5, cols), Some(0));
        assert_eq!(clip.controls.len(), 2, "a neighbour ramp invented points");
    }
}
