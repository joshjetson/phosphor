//! Track state — TrackState, TrackElement, Clip, MidiNote.

use phosphor_core::fx::{FxTarget, SendSlot};
use phosphor_core::project::{TrackConfig, TrackKind};

// ── Track Element Navigation ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackElement {
    Label,
    Fx,
    Volume,
    Mute,
    Solo,
    RecordArm,
    /// Where the track sits in the stereo field.
    Pan,
    /// How much of it goes to send bus A, and to B.
    SendA,
    SendB,
    Clip(usize),
}

impl TrackElement {
    pub fn move_right(self, num_clips: usize) -> Self {
        match self {
            Self::Label => Self::Fx,
            Self::Fx => Self::Volume,
            Self::Volume => Self::Mute,
            Self::Mute => Self::Solo,
            Self::Solo => Self::RecordArm,
            // The routing comes after the switches and before the clips: the
            // order the switches are in is muscle memory, and the three new
            // cells are a group of their own on the row below them.
            Self::RecordArm => Self::Pan,
            Self::Pan => Self::SendA,
            Self::SendA => Self::SendB,
            Self::SendB => {
                if num_clips > 0 { Self::Clip(0) } else { Self::SendB }
            }
            Self::Clip(i) => {
                if i + 1 < num_clips { Self::Clip(i + 1) } else { Self::Clip(i) }
            }
        }
    }

    pub fn move_left(self) -> Self {
        match self {
            Self::Label => Self::Label,
            Self::Fx => Self::Label,
            Self::Volume => Self::Fx,
            Self::Mute => Self::Volume,
            Self::Solo => Self::Mute,
            Self::RecordArm => Self::Solo,
            Self::Pan => Self::RecordArm,
            Self::SendA => Self::Pan,
            Self::SendB => Self::SendA,
            Self::Clip(0) => Self::SendB,
            Self::Clip(i) => Self::Clip(i - 1),
        }
    }
}

// ── Data Models ──

#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub number: usize,
    pub width: u16,
    pub has_content: bool,
    /// Start position on the timeline (ticks).
    pub start_tick: i64,
    /// Length in ticks.
    pub length_ticks: i64,
    /// Notes for piano roll display (from ClipSnapshot).
    pub notes: Vec<phosphor_core::clip::NoteSnapshot>,
    /// Notes hidden by shrinking the clip. Stored with start_frac and
    /// duration_frac as absolute tick ratios (tick / original_length_when_hidden)
    /// converted to tick offsets for stable restore.
    /// Format: (tick_offset_from_clip_start, duration_ticks, note, velocity)
    pub hidden_notes: Vec<(i64, i64, u8, u8)>,
    /// The recorded performance controllers — control change, pitch bend,
    /// channel pressure — in ticks from the clip's start. Invisible in the
    /// roll until automation lanes exist, but carried through every edit,
    /// paste, undo and session: a recorded wheel sweep must never vanish
    /// because a note near it was deleted.
    pub controls: Vec<phosphor_core::clip::ClipEvent>,
}

impl Clip {
    /// Everything the audio thread should play for this clip: the notes
    /// rebuilt from the roll's fractions, and the controllers as recorded,
    /// ordered so that offs lead, controllers set their state, and ons
    /// strike last on any shared tick. The one way a UI clip's events are
    /// built — a site that rebuilds from notes alone is a site that erases
    /// a recorded sweep.
    #[must_use]
    pub fn events_for_audio(&self) -> Vec<phosphor_core::clip::ClipEvent> {
        use phosphor_core::clip::{same_tick_order, NoteSnapshot};
        let mut events = NoteSnapshot::to_clip_events(&self.notes, self.length_ticks);
        events.extend(
            self.controls
                .iter()
                .filter(|e| e.tick >= 0 && e.tick <= self.length_ticks)
                .copied(),
        );
        events.sort_by_key(|e| (e.tick, same_tick_order(e.status)));
        events
    }
}

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
}

/// Which half of the `sends` pair a bus is.
fn send_index(slot: SendSlot) -> usize {
    match slot {
        SendSlot::A => 0,
        SendSlot::B => 1,
    }
}

#[derive(Debug, Clone)]
pub struct TrackState {
    pub name: String,
    pub muted: bool,
    pub soloed: bool,
    pub armed: bool,
    pub color_index: usize,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
    /// The UI's mirror of this strip's six insert slots. The effects
    /// themselves live on the audio thread; see [`super::FxInstance`].
    pub fx_chain: Vec<super::FxInstance>,
    /// Where the track sits in the image, −1 hard left to +1 hard right.
    ///
    /// Centre is unity in both channels, so a session written before pan
    /// existed loads at 0.0 and renders exactly as it did — see
    /// `phosphor_core::fx::pan_gains`.
    pub pan: f32,
    /// Post-fader send levels into buses A and B, as linear gains. Both
    /// start at zero, which is −inf: a send is a thing the player opens, not
    /// a thing they have to remember to close.
    pub sends: [f32; 2],
    /// The track this one's sidechain keys off, by mixer id — an identity
    /// rather than a position, so adding or deleting a track above it does
    /// not silently re-point the key at something else.
    pub key_source: Option<usize>,
    /// What that track was called when the key was set.
    ///
    /// Kept only so that a key whose track has been deleted can say *which*
    /// track — `Kick (missing)` rather than a bare `(missing)`, which tells
    /// the player nothing they can act on. Not persisted and not authoritative:
    /// the id is the key, this is a label, and a track that has been renamed
    /// re-labels itself the moment it is found.
    pub key_source_name: Option<String>,
    /// Fader position as a linear gain, mirroring the audio thread's
    /// `TrackConfig::volume`. Travel is
    /// [`TrackConfig::MIN_VOLUME`]..=[`TrackConfig::MAX_VOLUME`]; the audio
    /// thread's copy is clamped to it by `TrackConfig::set_volume`, so this
    /// mirror is the only place an out-of-range value could survive.
    pub volume: f32,
    /// Unique ID for this track (matches the mixer's track ID).
    pub mixer_id: Option<usize>,
    /// Handle to the audio engine's track state. When present, mute/solo/arm/volume
    /// writes go directly to the audio thread via atomics.
    pub handle: Option<std::sync::Arc<phosphor_core::project::TrackHandle>>,
    /// What type of instrument this track has.
    ///
    /// On a sequencer track this is the *child*: the thing in the plugin slot
    /// making the sound. There is no separate instrument type for a
    /// sequencer, which is what lets the child's panel, preset bank and saved
    /// parameters all keep working untouched. What marks the track is
    /// [`TrackState::sequencer`].
    pub instrument_type: Option<super::InstrumentType>,
    /// Parameter values (mirrors the audio thread's plugin params).
    pub synth_params: Vec<f32>,
    /// The step sequencer driving this track, when it has one.
    ///
    /// Edited only through [`crate::sequencer::ops::dispatch`] — see that
    /// module for why there is exactly one way in.
    ///
    /// Boxed because eight patterns are nineteen kilobytes, and a
    /// `TrackState` is cloned whole for every undo entry and moved whenever
    /// the track list grows. A track with no sequencer should pay a pointer
    /// for the possibility, not a pattern bank.
    pub sequencer: Option<Box<crate::sequencer::SequencerState>>,
}

impl TrackState {
    /// One press of the fader, in dB.
    ///
    /// The fader is stepped in dB rather than in linear gain so that every
    /// press moves the readout by exactly one. A linear step small enough to
    /// be useful near the top of the travel is a 6 dB jump near the bottom,
    /// and three consecutive linear detents around unity all round to the
    /// same displayed number — a control that does not appear to respond.
    pub const VOLUME_STEP_DB: f32 = 1.0;

    /// Bottom of the fader's travel, below which it goes to silence.
    ///
    /// −40 dB rather than the −60 a drawn fader usually bottoms out at: this
    /// one is stepped a keypress at a time, and 20 extra presses to reach a
    /// level that is inaudible anyway is travel nobody wants. Muting a track
    /// outright is `m`.
    pub const VOLUME_FLOOR_DB: f32 = -40.0;

    /// The middle of the pan control.
    pub const CENTRE_PAN: f32 = 0.0;

    /// One press of the pan control: twenty steps from centre to either end,
    /// which is fine enough to place a sound and coarse enough to reach the
    /// end of the travel without holding the key down.
    pub const PAN_STEP: f32 = 0.05;

    /// The top of a send, unity. A send cannot be pushed past the level the
    /// track is already at — that is what the bus return is for.
    pub const MAX_SEND_DB: f32 = 0.0;

    pub fn new(name: &str, color_index: usize, armed: bool, kind: TrackKind, clips: Vec<Clip>) -> Self {
        Self {
            name: name.to_string(),
            muted: false,
            soloed: false,
            armed,
            color_index,
            kind,
            clips,
            fx_chain: Vec::new(),
            pan: Self::CENTRE_PAN,
            sends: [0.0; 2],
            key_source: None,
            key_source_name: None,
            volume: TrackConfig::DEFAULT_VOLUME,
            mixer_id: None,
            handle: None,
            instrument_type: None,
            synth_params: Vec::new(),
            sequencer: None,
        }
    }

    /// Sync mute/solo/arm/volume to the audio thread handle (if wired up).
    pub fn sync_to_audio(&self) {
        if let Some(ref h) = self.handle {
            h.config.muted.store(self.muted, std::sync::atomic::Ordering::Relaxed);
            h.config.soloed.store(self.soloed, std::sync::atomic::Ordering::Relaxed);
            h.config.armed.store(self.armed, std::sync::atomic::Ordering::Relaxed);
            h.config.set_volume(self.volume);
        }
    }

    /// The fader position in dB relative to unity. `None` at the bottom of
    /// the travel, where the answer is negative infinity.
    pub fn volume_db(&self) -> Option<f32> {
        (self.volume > 0.0).then(|| 20.0 * self.volume.log10())
    }

    /// Move the fader by `steps` presses and push the result to the audio
    /// thread. Returns the new linear gain.
    ///
    /// The current position is rounded onto the dB grid before stepping, so
    /// the fader self-corrects: the default of 0.75 is −2.5 dB, off the grid,
    /// and the first press lands it on −2 or −3 and every press after that
    /// moves exactly one. A session saved with a hand-edited volume snaps the
    /// same way.
    pub fn adjust_volume(&mut self, steps: i32) -> f32 {
        let top_db = 20.0 * TrackConfig::MAX_VOLUME.log10();
        // One step below the floor is the silent position, so stepping down
        // from the bottom of the travel reaches it and stepping up leaves it.
        let silent_db = Self::VOLUME_FLOOR_DB - Self::VOLUME_STEP_DB;
        let current_db = self.volume_db().map_or(silent_db, |db| db.round());

        let target_db =
            (current_db + steps as f32 * Self::VOLUME_STEP_DB).clamp(silent_db, top_db);
        self.volume = if target_db < Self::VOLUME_FLOOR_DB {
            TrackConfig::MIN_VOLUME
        } else {
            10.0f32
                .powf(target_db / 20.0)
                .clamp(TrackConfig::MIN_VOLUME, TrackConfig::MAX_VOLUME)
        };

        self.sync_to_audio();
        self.volume
    }

    /// Read VU levels from the audio thread handle.
    pub fn vu_levels(&self) -> (f32, f32) {
        self.handle.as_ref().map(|h| h.vu.get()).unwrap_or((0.0, 0.0))
    }

    /// Whether this track is an instrument track wired to the audio engine.
    ///
    /// The bus strips have handles too — that is how their meters and their
    /// return levels work — so "has a handle" stopped being the same question
    /// as "is a track the player can arm, record and load a patch on" the day
    /// the buses became real. Every caller of this means the second one.
    pub fn is_live(&self) -> bool {
        self.handle.is_some() && !self.is_bus()
    }

    /// Whether this strip is one of the two send buses or the master.
    pub fn is_bus(&self) -> bool {
        matches!(
            self.kind,
            TrackKind::SendA | TrackKind::SendB | TrackKind::Master
        )
    }

    /// Which send bus this strip is, if it is one.
    pub fn send_slot(&self) -> Option<SendSlot> {
        match self.kind {
            TrackKind::SendA => Some(SendSlot::A),
            TrackKind::SendB => Some(SendSlot::B),
            _ => None,
        }
    }

    /// Which insert chain on the audio thread this strip's `fx_chain`
    /// mirrors.
    ///
    /// `None` for an instrument track that has not been given to the mixer
    /// yet: there is nothing to address.
    pub fn fx_target(&self) -> Option<FxTarget> {
        match self.kind {
            TrackKind::SendA => Some(FxTarget::BusA),
            TrackKind::SendB => Some(FxTarget::BusB),
            TrackKind::Master => Some(FxTarget::Master),
            TrackKind::Instrument | TrackKind::Audio => self.mixer_id.map(FxTarget::Track),
        }
    }

    /// Move the pan control by `steps` presses, and report where it landed.
    ///
    /// Snaps onto the centre when it passes it: the centre is a position the
    /// player has to be able to get back to exactly, since it is the one that
    /// leaves the track untouched.
    pub fn adjust_pan(&mut self, steps: i32) -> f32 {
        let target = self.pan + steps as f32 * Self::PAN_STEP;
        self.pan = if target.abs() < Self::PAN_STEP * 0.5 {
            Self::CENTRE_PAN
        } else {
            target.clamp(-1.0, 1.0)
        };
        self.pan
    }

    /// A send level in decibels, or `None` when the send is closed.
    pub fn send_db(&self, slot: SendSlot) -> Option<f32> {
        let gain = self.sends[send_index(slot)];
        (gain > 0.0).then(|| 20.0 * gain.log10())
    }

    /// Set a send from a level in decibels. Below the floor, the send closes.
    pub fn set_send_db(&mut self, slot: SendSlot, db: f32) -> f32 {
        let gain = if db <= phosphor_core::fx::SILENT_DB {
            0.0
        } else {
            phosphor_core::fx::db_to_gain(db.min(Self::MAX_SEND_DB))
        };
        self.sends[send_index(slot)] = gain;
        gain
    }

    /// A send's linear gain.
    pub fn send(&self, slot: SendSlot) -> f32 {
        self.sends[send_index(slot)]
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
}
