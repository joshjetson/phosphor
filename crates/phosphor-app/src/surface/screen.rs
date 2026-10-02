//! Which screen has the keys — the one fact every following control reads.
//!
//! The deck keeps no picture of its own of where the player is. It reads it
//! from the app every time, the way the key handler does and in the same
//! order, so a player who uses the keyboard and the deck together can never
//! leave the two disagreeing about what is locked.
//!
//! The ladder the deck talks about — song, track, part, page — is the app's
//! own: on the track list NAVIGATE walks tracks and LOCK (Enter) selects one;
//! with a track selected it walks the things belonging to that track; BACK
//! (Esc) steps out. [`Screen`] names each rung and each part.

use crate::state::{ClipTab, ClipViewFocus, FxPanelTab, NavState, Pane};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Screen {
    /// A question, a menu, a picker or a field: it takes every key.
    Prompt,
    /// The transport row.
    Transport,
    /// The loop brace.
    Loop,
    /// The track list with nothing selected — the song.
    Tracks,
    /// A track selected: its cells, walked one by one.
    Track,
    /// A cell held: a fader, a send, a clip.
    TrackCell,
    /// An instrument's own panel.
    Instrument,
    /// A track's insert chain.
    FxChain,
    /// An effect's panel.
    Effect,
    /// A MIDI effect's panel: the arp, the chord device.
    MidiEffect,
    /// The sampler's pad map.
    Pads,
    /// The sampler's pad map in keys mode.
    Zones,
    Trim,
    Chop,
    /// Recording a pad from an instrument.
    Source,
    /// The step sequencer.
    Steps,
    /// The piano roll.
    Notes,
    /// The piano roll's note-by-note edit mode.
    EditMode,
    /// The piano roll's automation lane.
    Automation,
    /// A clip's settings.
    Settings,
    /// The practice room.
    Practice,
    /// The progression editor.
    Progressions,
}

/// Which way NAVIGATE turns on a screen: down a list (`j`/`k`) or along a
/// row (`h`/`l`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    List,
    Row,
}

impl Screen {
    /// Every screen, for the audits that have to visit each.
    pub const ALL: [Screen; 22] = [
        Self::Prompt,
        Self::Transport,
        Self::Loop,
        Self::Tracks,
        Self::Track,
        Self::TrackCell,
        Self::Instrument,
        Self::FxChain,
        Self::Effect,
        Self::MidiEffect,
        Self::Pads,
        Self::Zones,
        Self::Trim,
        Self::Chop,
        Self::Source,
        Self::Steps,
        Self::Notes,
        Self::EditMode,
        Self::Automation,
        Self::Settings,
        Self::Practice,
        Self::Progressions,
    ];

    /// The screen's name, for the line that says what is locked.
    pub fn label(self) -> &'static str {
        match self {
            Self::Prompt => "Question",
            Self::Transport => "Transport",
            Self::Loop => "Loop",
            Self::Tracks => "Song",
            Self::Track => "Track",
            Self::TrackCell => "Held",
            Self::Instrument => "Instrument",
            Self::FxChain => "Effects",
            Self::Effect => "Effect",
            Self::MidiEffect => "MIDI effect",
            Self::Pads => "Pads",
            Self::Zones => "Zones",
            Self::Trim => "Trim",
            Self::Chop => "Chop",
            Self::Source => "Record a pad",
            Self::Steps => "Steps",
            Self::Notes => "Notes",
            Self::EditMode => "Edit notes",
            Self::Automation => "Automation",
            Self::Settings => "Clip settings",
            Self::Practice => "Practice",
            Self::Progressions => "Progressions",
        }
    }

    /// What NAVIGATE walks here.
    pub fn axis(self) -> Axis {
        match self {
            Self::Transport
            | Self::Loop
            | Self::Track
            | Self::TrackCell
            | Self::Pads
            | Self::Zones
            | Self::Trim
            | Self::Notes
            | Self::EditMode
            | Self::Automation => Axis::Row,
            _ => Axis::List,
        }
    }
}

/// The screen that has the keys right now.
///
/// In the key handler's own order of precedence: whatever would swallow a
/// key first is the screen.
pub fn screen(nav: &NavState) -> Screen {
    if nav.whats_new.open
        || nav.confirm_modal.open
        || nav.quantize_modal.open
        || nav.space_menu.open
        || nav.input_modal.open
        || nav.file_picker.open
        || nav.instrument_modal.open
        || nav.fx_menu.open
        || nav.preset_modal.open
    {
        return Screen::Prompt;
    }
    if nav.practice.open {
        return Screen::Practice;
    }
    if nav.prog_editor.open {
        return Screen::Progressions;
    }
    if nav.loop_editor.active {
        return Screen::Loop;
    }
    match nav.focused_pane {
        Pane::Transport => Screen::Transport,
        Pane::Tracks if nav.element_locked => Screen::TrackCell,
        Pane::Tracks if nav.track_selected => Screen::Track,
        Pane::Tracks => Screen::Tracks,
        Pane::ClipView => clip_view_screen(nav),
    }
}

fn clip_view_screen(nav: &NavState) -> Screen {
    let view = &nav.clip_view;
    if view.piano_roll.edit_mode {
        return Screen::EditMode;
    }
    if view.focus == ClipViewFocus::FxPanel {
        return match view.fx_panel_tab {
            FxPanelTab::TrackFx => Screen::FxChain,
            FxPanelTab::Synth => Screen::Instrument,
        };
    }
    match view.clip_tab {
        ClipTab::Fx if view.fx.midi_slot.is_some() => Screen::MidiEffect,
        ClipTab::Fx => Screen::Effect,
        ClipTab::Sequencer => Screen::Steps,
        ClipTab::Pads if nav.panel_source().is_some() => Screen::Source,
        ClipTab::Pads if nav.chop_here().is_some() => Screen::Chop,
        ClipTab::Pads if view.sampler.trim.is_some() => Screen::Trim,
        ClipTab::Pads
            if nav
                .current_track()
                .and_then(|t| t.sampler.as_deref())
                .is_some_and(|s| s.mode == crate::sampler::MapMode::Keys) =>
        {
            Screen::Zones
        }
        ClipTab::Pads => Screen::Pads,
        ClipTab::InstConfig => Screen::Instrument,
        ClipTab::Settings => Screen::Settings,
        ClipTab::PianoRoll if view.piano_roll.automation_focus => Screen::Automation,
        ClipTab::PianoRoll => Screen::Notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav() -> NavState {
        NavState::new(Vec::new())
    }

    #[test]
    fn the_track_list_is_the_song_until_a_track_is_locked() {
        let mut n = nav();
        n.focused_pane = Pane::Tracks;
        assert_eq!(screen(&n), Screen::Tracks);
        n.track_selected = true;
        assert_eq!(screen(&n), Screen::Track);
        n.element_locked = true;
        assert_eq!(screen(&n), Screen::TrackCell);
    }

    /// A question asked over any screen is the screen: it takes the keys.
    #[test]
    fn a_question_or_a_menu_takes_the_keys_over_whatever_is_under_it() {
        let mut n = nav();
        n.focused_pane = Pane::ClipView;
        n.space_menu.open = true;
        assert_eq!(screen(&n), Screen::Prompt);
        n.space_menu.open = false;
        n.confirm_modal.open = true;
        assert_eq!(screen(&n), Screen::Prompt);
    }

    #[test]
    fn the_clip_view_names_its_parts() {
        let mut n = nav();
        n.focused_pane = Pane::ClipView;
        n.clip_view.focus = ClipViewFocus::FxPanel;
        n.clip_view.fx_panel_tab = FxPanelTab::TrackFx;
        assert_eq!(screen(&n), Screen::FxChain);
        n.clip_view.fx_panel_tab = FxPanelTab::Synth;
        assert_eq!(screen(&n), Screen::Instrument);
        n.clip_view.focus = ClipViewFocus::PianoRoll;
        n.clip_view.clip_tab = ClipTab::PianoRoll;
        assert_eq!(screen(&n), Screen::Notes);
        n.clip_view.piano_roll.edit_mode = true;
        assert_eq!(screen(&n), Screen::EditMode);
    }

    #[test]
    fn navigate_walks_lists_down_and_rows_along() {
        assert_eq!(Screen::Tracks.axis(), Axis::List);
        assert_eq!(Screen::Track.axis(), Axis::Row);
        assert_eq!(Screen::Pads.axis(), Axis::Row);
        assert_eq!(Screen::Instrument.axis(), Axis::List);
    }
}
