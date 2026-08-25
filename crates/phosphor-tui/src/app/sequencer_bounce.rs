//! Bounce: the pattern under the editor, written onto the timeline as a clip.
//!
//! The arithmetic is not here. [`phosphor_app::sequencer::compile`] runs the
//! same generator the audio thread runs, so a bounced clip and the pattern it
//! came from are the same notes at the same ticks by construction rather than
//! by agreement. What this file does is the part that belongs to the
//! application: find the clip a bounce becomes, tell the audio thread about
//! it, stop the pattern, and say where it landed.

use super::*;

use phosphor_app::sequencer::compile;
use phosphor_app::sequencer::ops::SeqOp;

impl App {
    /// Write the pattern — or the whole chain, when there is one — to the
    /// next free bar at or after the playhead.
    pub(crate) fn bounce_pattern(&mut self) {
        use crate::debug_log as dbg;

        let track_idx = self.nav.track_cursor;
        let playhead = self.engine.transport.position_ticks();

        let Some(track) = self.nav.tracks.get(track_idx) else { return };
        let Some(state) = track.sequencer.as_deref() else { return };
        let chained = state.is_chained();
        let Some(bounce) = compile::bounce_chain(state, playhead, &track.clips) else {
            self.status_message = Some((
                "nothing to bounce — the pattern has no hits in it".into(),
                std::time::Instant::now(),
            ));
            return;
        };

        // Stopped first, and stopped whatever else happens: a clip playing the
        // same notes as the pattern that produced it is every note flammed
        // against itself, which sounds like a broken instrument rather than
        // like two copies of one part.
        if bounce.stops_playback {
            self.sequencer_op(SeqOp::SetPlaying(false));
        }

        let clip = crate::state::Clip {
            number: 0, // renumbered below, with the rest of the track's
            width: ((bounce.length_ticks + Transport::PPQ - 1) / Transport::PPQ).max(2) as u16,
            has_content: true,
            start_tick: bounce.start_tick,
            length_ticks: bounce.length_ticks,
            notes: bounce.notes(),
            hidden_notes: Vec::new(),
        };

        let Some(track) = self.nav.tracks.get_mut(track_idx) else { return };
        let clip_index = track.clips.len();
        let mixer_id = track.mixer_id;
        track.clips.push(clip);
        for (index, clip) in track.clips.iter_mut().enumerate() {
            clip.number = index + 1;
        }

        // The compiled events go to the audio thread as they are, rather than
        // being rebuilt out of the note snapshots the piano roll draws: those
        // are fractions of a clip, and a round trip through them would move
        // every note by whatever the fraction could not hold.
        if let Some(track_id) = mixer_id {
            let tx = &self.engine.shared.mixer_command_tx;
            let _ = tx.send(MixerCommand::CreateClip {
                track_id,
                start_tick: bounce.start_tick,
                length_ticks: bounce.length_ticks,
            });
            let _ = tx.send(MixerCommand::UpdateClip {
                track_id,
                clip_index,
                events: bounce.events.clone(),
            });
        }

        self.nav.undo_stack.push(UndoAction::AddClip { track_idx, clip_idx: clip_index });

        let bars = bounce.bars();
        let stopped = if bounce.stops_playback { " · pattern stopped" } else { "" };
        let text = format!(
            "bounced {}{} bar{} to bar {}{}",
            if chained { "chain, " } else { "" },
            bars,
            if bars == 1 { "" } else { "s" },
            bounce.bar(),
            stopped,
        );
        dbg::system(&format!(
            "bounce: track={track_idx} start={} len={} events={} clip={clip_index}",
            bounce.start_tick,
            bounce.length_ticks,
            bounce.events.len(),
        ));
        self.status_message = Some((text, std::time::Instant::now()));

        // The clip view follows the bounce, so the notes that were just made
        // are what the piano roll is looking at when the player Tabs to it.
        self.nav.clip_view_target = Some((track_idx, clip_index));
    }
}
