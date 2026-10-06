//! What the deck's lights show, read from the app's state.
//!
//! The deck lights a control when Phosphor sends a note back on the deck's
//! channel for that control's note, at a velocity that is the brightness.
//! This module works out the brightness of all forty-two from the same state
//! the screen draws, so the panel and the screen can never disagree: PLAY is
//! lit while the song plays, a track button in MUTE mode is lit while its
//! track is muted, a pad on the step grid is lit while its step is on.
//!
//! Until Phosphor speaks, the firmware lights things itself — a pad as it is
//! struck, the last button picked in each set of choices — and it goes on
//! doing that for whatever Phosphor leaves alone. Phosphor leaves the pads
//! alone when they are notes rather than steps ([`Level::Deck`]), so a
//! struck pad still glows under the finger.

use phosphor_core::transport::TransportSnapshot;

use super::binding::{FnTarget, TrackMode};
use super::layout::{control, deck, ControlId, COLUMNS, PADS};
use crate::state::NavState;

/// Every light on the deck.
pub const LIGHTS: usize = 42;

/// Fully lit.
pub const ON: u8 = 127;

/// A track button over a track that exists but is not lit for the mode: a
/// faint glow, so the columns with something under them can be told from the
/// ones with nothing.
pub const PRESENT: u8 = 10;

/// The step the pattern is playing, when it is off: lit enough to follow.
pub const PLAYHEAD: u8 = 36;

/// An accented step is full; a plain one a little under, so accents show.
pub const STEP_ON: u8 = 90;

/// One light's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Phosphor sets it to this.
    Set(u8),
    /// The deck lights it itself.
    Deck,
}

/// The deck's own choices that the app holds: what the function knob is
/// on, what the track buttons do and which eight they hold, which half of
/// a thirty-two step pattern the pads show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeckChoices {
    pub function: FnTarget,
    pub track_mode: TrackMode,
    pub track_bank: usize,
    pub step_half: u8,
}

/// The step grid the pads are showing, when they are steps: the cursor
/// track's lane under the editor, and where its pattern is playing.
pub struct StepView<'a> {
    pub steps: &'a [phosphor_core::pattern::Step],
    /// The pattern's length in steps.
    pub length: usize,
    pub playhead: Option<usize>,
}

/// Every light, in chain order.
#[must_use]
pub fn levels(
    nav: &NavState,
    transport: &TransportSnapshot,
    choices: &DeckChoices,
    steps: Option<&StepView<'_>>,
) -> [Level; LIGHTS] {
    let mut out = [Level::Set(0); LIGHTS];
    let mut set = |id: ControlId, level: Level| {
        if let Some(light) = control(id).light {
            out[usize::from(light)] = level;
        }
    };
    let lit = |on: bool| Level::Set(if on { ON } else { 0 });

    // The transport row.
    set(ControlId::Play, lit(transport.playing || transport.count_in_remaining > 0));
    set(ControlId::Rec, lit(transport.recording));
    set(ControlId::Overdub, lit(transport.recording && transport.playing && transport.looping));
    set(ControlId::Loop, lit(nav.loop_editor.enabled));
    set(ControlId::Click, lit(transport.metronome));
    set(ControlId::CountIn, lit(transport.count_in_bars > 0));

    // The function knob's targets: the one it is on.
    for (id, target) in [
        (ControlId::FnTempo, FnTarget::Tempo),
        (ControlId::FnSwing, FnTarget::Swing),
        (ControlId::FnGrid, FnTarget::Grid),
        (ControlId::FnMaster, FnTarget::Master),
        (ControlId::FnLoop, FnTarget::Loop),
        (ControlId::FnLast, FnTarget::Last),
    ] {
        set(id, lit(choices.function == target));
    }

    // The track buttons' mode, and what each button says in it.
    for (id, mode) in [
        (ControlId::ModeSelect, TrackMode::Select),
        (ControlId::ModeMute, TrackMode::Mute),
        (ControlId::ModeSolo, TrackMode::Solo),
        (ControlId::ModeArm, TrackMode::Arm),
    ] {
        set(id, lit(choices.track_mode == mode));
    }
    for n in 0..COLUMNS {
        let idx = choices.track_bank * usize::from(COLUMNS) + usize::from(n);
        let level = match nav.tracks.get(idx) {
            None => 0,
            Some(track) => {
                let on = match choices.track_mode {
                    TrackMode::Select => nav.track_cursor == idx,
                    TrackMode::Mute => track.muted,
                    TrackMode::Solo => track.soloed,
                    TrackMode::Arm => track.armed,
                };
                if on { ON } else { PRESENT }
            }
        };
        set(ControlId::TrackButton(n), Level::Set(level));
    }

    // Which half of the steps the pads hold.
    set(ControlId::StepsLow, lit(choices.step_half == 0));
    set(ControlId::StepsHigh, lit(choices.step_half == 1));

    // The pads: the step grid when they are steps, the deck's own glow when
    // they are notes.
    for n in 0..PADS {
        let level = match steps {
            None => Level::Deck,
            Some(view) => {
                let at = usize::from(choices.step_half) * usize::from(PADS) + usize::from(n);
                let step = view.steps.get(at).filter(|_| at < view.length);
                Level::Set(match step {
                    None => 0,
                    Some(step) if step.on && step.accent => ON,
                    Some(step) if step.on => STEP_ON,
                    Some(_) if view.playhead == Some(at) => PLAYHEAD,
                    Some(_) => 0,
                })
            }
        };
        set(ControlId::Pad(n), level);
    }
    out
}

/// The note that sets light `light`: the note its control sends.
#[must_use]
pub fn note_of(light: usize) -> Option<u8> {
    deck().iter().find(|c| c.light.map(usize::from) == Some(light)).and_then(|c| c.note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{NavState, TrackState};
    use phosphor_core::project::TrackKind;

    fn snapshot() -> TransportSnapshot {
        TransportSnapshot {
            playing: false,
            recording: false,
            looping: false,
            metronome: false,
            position_ticks: 0,
            tempo_bpm: 120.0,
            loop_start_ticks: 0,
            loop_end_ticks: 0,
            count_in_bars: 0,
            count_in_remaining: 0,
        }
    }

    fn light(id: ControlId) -> usize {
        usize::from(control(id).light.expect("a lit control"))
    }

    #[test]
    fn every_light_has_a_note_to_set_it_with() {
        assert_eq!(deck().iter().filter(|c| c.light.is_some()).count(), LIGHTS);
        for i in 0..LIGHTS {
            assert!(note_of(i).is_some(), "light {i} has no note");
        }
    }

    /// The transport row follows the transport, and the choices follow the
    /// deck's own choices.
    #[test]
    fn the_buttons_say_what_is_on() {
        let nav = NavState::new(Vec::new());
        let mut t = snapshot();
        let choices = DeckChoices { function: FnTarget::Swing, track_mode: TrackMode::Mute, ..Default::default() };
        let quiet = levels(&nav, &t, &choices, None);
        assert_eq!(quiet[light(ControlId::Play)], Level::Set(0));
        t.playing = true;
        t.metronome = true;
        let playing = levels(&nav, &t, &choices, None);
        assert_eq!(playing[light(ControlId::Play)], Level::Set(ON));
        assert_eq!(playing[light(ControlId::Click)], Level::Set(ON));
        assert_eq!(playing[light(ControlId::FnSwing)], Level::Set(ON));
        assert_eq!(playing[light(ControlId::FnTempo)], Level::Set(0));
        assert_eq!(playing[light(ControlId::ModeMute)], Level::Set(ON));
        assert_eq!(playing[light(ControlId::Pad(0))], Level::Deck, "pads are the deck's while they are notes");
    }

    /// In MUTE mode a track button is lit while its track is muted, and
    /// glows faintly over any track that exists.
    #[test]
    fn track_buttons_show_their_tracks() {
        let tracks = (0..3).map(|i| TrackState::new("t", i, true, TrackKind::Instrument, vec![])).collect();
        let mut nav = NavState::new(tracks);
        let muted = 1;
        nav.tracks[muted].muted = true;
        let choices = DeckChoices { track_mode: TrackMode::Mute, ..Default::default() };
        let out = levels(&nav, &snapshot(), &choices, None);
        for n in 0..COLUMNS {
            let want = match usize::from(n) {
                i if i == muted => ON,
                i if i < nav.tracks.len() => PRESENT,
                _ => 0,
            };
            assert_eq!(out[light(ControlId::TrackButton(n))], Level::Set(want), "column {n}");
        }
    }

    /// On the step grid the pads are the steps of the half they hold.
    #[test]
    fn pads_show_the_steps() {
        let nav = NavState::new(Vec::new());
        let mut steps = [phosphor_core::pattern::Step::silent(); 32];
        steps[0].on = true;
        steps[1].on = true;
        steps[1].accent = true;
        steps[17].on = true;
        let view = StepView { steps: &steps, length: 32, playhead: Some(2) };
        let low = levels(&nav, &snapshot(), &DeckChoices::default(), Some(&view));
        assert_eq!(low[light(ControlId::Pad(0))], Level::Set(STEP_ON));
        assert_eq!(low[light(ControlId::Pad(1))], Level::Set(ON));
        assert_eq!(low[light(ControlId::Pad(2))], Level::Set(PLAYHEAD));
        assert_eq!(low[light(ControlId::Pad(3))], Level::Set(0));
        let high = levels(&nav, &snapshot(), &DeckChoices { step_half: 1, ..Default::default() }, Some(&view));
        assert_eq!(high[light(ControlId::Pad(1))], Level::Set(STEP_ON));
        assert_eq!(high[light(ControlId::StepsHigh)], Level::Set(ON));
        let short = StepView { steps: &steps, length: 16, playhead: None };
        let past = levels(&nav, &snapshot(), &DeckChoices { step_half: 1, ..Default::default() }, Some(&short));
        assert_eq!(past[light(ControlId::Pad(1))], Level::Set(0), "a step past the pattern's end is dark");
    }
}
