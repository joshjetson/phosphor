//! The deck's lights, kept in step with the screen.
//!
//! Every frame the lights are worked out from the app's state
//! ([`phosphor_app::surface::lights::levels`]) and whatever changed goes to
//! the deck ([`crate::deck_out::DeckOut`]). With no deck plugged in this
//! returns before working anything out.

use super::*;

use phosphor_app::surface::lights::{levels, DeckChoices, StepView};
use phosphor_app::surface::screen::{screen, Screen};

impl App {
    pub(crate) fn refresh_deck_lights(&mut self) {
        let Some(out) = self.deck_out.as_mut() else { return };
        out.poll();
        if !out.connected() {
            return;
        }
        let choices = DeckChoices {
            function: self.deck.function,
            track_mode: self.deck.track_mode,
            track_bank: self.deck.track_bank,
            step_half: self.deck.step_half,
        };
        let transport = self.engine.transport.snapshot();
        let track_idx = self.nav.track_cursor;
        let sequencer = (screen(&self.nav) == Screen::Steps)
            .then(|| self.nav.tracks.get(track_idx).and_then(|t| t.sequencer.as_deref()))
            .flatten();
        let steps = sequencer.map(|seq| StepView {
            steps: &seq.lane().steps,
            length: usize::from(seq.pattern().steps),
            playhead: self.nav.sequencer_playhead(track_idx),
        });
        let lit = levels(&self.nav, &transport, &choices, steps.as_ref());
        if let Some(out) = self.deck_out.as_mut() {
            out.show(&lit);
        }
    }
}
