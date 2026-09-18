//! Source mode and the resampler: playing one of our own instruments
//! through a sampler track, recording what was played, and landing it on
//! the pad as audio.
//!
//! # The loan
//!
//! There is one plugin slot on a track, and while source mode is on it
//! holds the chosen instrument instead of the sampler. That is the whole
//! trick: the player hears exactly what will be recorded, through the
//! track's own MIDI effects, inserts, fader and sends, because it *is* the
//! track. Leaving the mode puts the sampler back — and then replays every
//! occupied pad into it, because the instance that comes back is a fresh
//! one that has never heard of the kit. Forgetting that replay is a silent
//! kit and the easiest bug in this file to write.
//!
//! # What is recorded, and what is rendered
//!
//! The capture is MIDI: note-ons and note-offs with the arrival stamps the
//! tap already carries ([`phosphor_app::sampler::capture`]). Nothing is
//! recorded from the audio thread, which is why this can never glitch a
//! performance and why a take is bar-exact rather than "however long the
//! buffer happened to be". The audio is made afterwards, offline, by
//! replaying those events through a fresh instrument
//! ([`phosphor_app::sampler::render`]) — deterministic, and free of any
//! chance of feeding the sampler its own output.
//!
//! # The doors
//!
//! Everything that changes a pad still goes through
//! [`super::sampler_ops`]'s rule: state, then the undo step, then
//! [`App::sync_sampler_pad`]. A landed take is one step, and the step
//! holds the only other reference to the audio, which is what the buffer
//! lifetime contract requires of anything that can be undone.

use super::*;

use std::sync::Arc;

use phosphor_app::sampler::capture::{CapturedEvent, SourceMode, TakeCapture, TakePlan};
use phosphor_app::sampler::{render, ChildChange, PadSource, SamplerState, TakeKind};
use phosphor_app::state::InstrumentPick;

use crate::state::undo::UndoScope;

impl App {
    // ── Entering and leaving ──

    /// `i` on the pad map: ask what this pad should be recorded from.
    pub(crate) fn open_pad_source_picker(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        // While the mode is already on *this* track, `i` swaps the
        // instrument rather than opening a second mode on another pad: the
        // pad is the mode's, and the answer replaces what is in the slot.
        // A mode running on some other track has no say over this one's
        // cursor — see the hand-off in [`App::enter_sampler_source`].
        let pad = match self.nav.sampler_source.as_deref() {
            Some(mode) if mode.track_idx == track_idx => mode.pad,
            _ => self.nav.tracks[track_idx].sampler.as_ref().map_or(0, |s| s.cursor),
        };
        // What it was recorded from last time, read from whatever the
        // cursor is editing: in keys mode that is the zone's own sound,
        // which is where the take is going to land.
        let current = self.nav.tracks[track_idx]
            .sampler
            .as_ref()
            .and_then(|s| s.edited())
            .and_then(|p| p.source.as_ref())
            .map(|s| s.instrument);
        self.nav.instrument_modal.open_for_pad(track_idx, pad, current);
    }

    /// Enter on that picker: the track starts playing the instrument, and
    /// every key played is a performance waiting to be recorded.
    pub(crate) fn enter_sampler_source(&mut self, pick: InstrumentPick, instrument: InstrumentType) {
        let InstrumentPick::PadSource { track_idx, pad } = pick else { return };
        if self.nav.tracks.get(track_idx).and_then(|t| t.sampler.as_ref()).is_none() {
            return;
        }
        // A root-learn armed before the mode began must not survive it:
        // the source banner hides the blinking question, and the first
        // note played after leaving used to retune the zone instead of
        // being a note.
        self.nav.clip_view.sampler.root_learn = false;
        // An armed capture on the way out of the old instrument lands
        // first: swapping the sound under a performance would render it
        // through an instrument it was never played on.
        self.land_armed_take();
        // Only one track can have its slot on loan at a time. Starting the
        // mode on a second one gives the first its sampler back — without
        // this, the track left behind would play a synth forever with
        // nothing on the screen offering to put it right.
        if self.nav.sampler_source.as_deref().is_some_and(|m| m.track_idx != track_idx) {
            self.leave_sampler_source();
        }

        // Whatever the outgoing mode's panel was dialled to belongs to its
        // pad before anything else happens. `i` pressed a second time on the
        // same pad is the case that needs it: the answer below reads the
        // pad's memory back, and without this it would read the numbers the
        // mode *started* with rather than the ones on the screen.
        self.commit_source_panel();

        // The pad remembers what it was recorded from — and the panel it
        // was recorded with, so the next take of the same sound is one key
        // away. Keeping the numbers when the instrument has not changed is
        // the point: a player who tweaked the DX7 and pressed `i` again
        // gets the DX7 they tweaked.
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let (params, take) = {
            let Some(sampler) =
                self.nav.tracks.get_mut(track_idx).and_then(|t| t.sampler.as_deref_mut())
            else {
                return;
            };
            let Some(state) = sampler.edited_mut() else {
                self.flash(phosphor_app::sampler::SamplerState::no_zone_message());
                return;
            };
            let keep = state
                .source
                .as_ref()
                .filter(|s| s.instrument == instrument)
                .map(|s| s.params.clone());
            let params = keep.unwrap_or_else(|| phosphor_app::preset::defaults(instrument));
            state.source = Some(PadSource { instrument, params: params.clone() });
            (params, state.take)
        };
        self.nav.commit_undo(before, "pad source");

        // An audition through the sampler cannot survive the sampler
        // leaving the slot.
        self.stop_sampler_preview();
        self.nav.clip_view.sampler.trim = None;
        self.nav.clip_view.sampler.locked = false;
        self.install_instrument(track_idx, instrument, &params);
        self.nav.sampler_source =
            Some(Box::new(SourceMode::new(track_idx, pad, instrument, params, take)));
        // The panel under the cursor is the borrowed instrument's from here
        // until the sampler comes back, and the two are not the same length.
        self.nav.clamp_panel_cursor();
        self.flash(format!(
            "source: {} \u{00b7} pad {} \u{00b7} take: {} \u{00b7} play it \u{00b7} r records \u{00b7} p swaps \u{00b7} esc puts the sampler back",
            instrument.label(),
            SamplerState::pad_label(pad),
            take.label(),
        ));
    }

    /// `p` in source mode: swap what `r` will land.
    ///
    /// Audio is a render of the performance through the instrument, which
    /// costs a buffer and sounds the same forever. A phrase is the
    /// performance itself, replayed through the sampler's one child, which
    /// costs a few hundred events and follows whatever that child becomes.
    /// The pad remembers the answer with its source, so a player who
    /// records phrases finds the mode the way they left it.
    pub(crate) fn toggle_sampler_take_kind(&mut self) {
        let Some(mode) = self.nav.sampler_source.as_deref() else { return };
        if mode.is_armed() {
            self.flash("the take is running \u{00b7} r ends it, then p swaps what r lands");
            return;
        }
        let (track_idx, take) = (mode.track_idx, mode.take.other());
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        if let Some(state) = self
            .nav
            .tracks
            .get_mut(track_idx)
            .and_then(|t| t.sampler.as_deref_mut())
            .and_then(SamplerState::edited_mut)
        {
            state.take = take;
        }
        self.nav.commit_undo(before, "take kind");
        if let Some(mode) = self.nav.sampler_source.as_deref_mut() {
            mode.take = take;
        }
        self.flash(match take {
            TakeKind::Audio => {
                "take: audio \u{00b7} r renders the performance onto the pad \u{00b7} p swaps"
                    .to_string()
            }
            TakeKind::Phrase => format!(
                "take: phrase \u{00b7} r keeps the notes and plays them back through {} \u{00b7} p swaps",
                self.nav
                    .sampler_source
                    .as_deref()
                    .map_or_else(String::new, |m| m.instrument.label().to_string()),
            ),
        });
    }

    /// Write the mode's panel onto the pad it belongs to, when it differs
    /// from what the pad already remembers. Returns whether it did.
    ///
    /// [`SourceMode::params`] is the working copy: the numbers the borrowed
    /// slot is playing, the numbers the render will replay, and the numbers
    /// the `[inst]` panel edits while the mode is on. The pad's own memory of
    /// them is written at the three moments the working copy stops being the
    /// live one — a take landing, the mode ending, and the instrument being
    /// swapped under it — because those are the moments the player would
    /// otherwise lose a sound they dialled.
    ///
    /// **Only when it differs.** A trip through the mode that touched no knob
    /// must not put a step on the undo stack or a change in the session file;
    /// a player who pressed `i`, listened, and pressed `esc` changed nothing
    /// and should find nothing changed.
    ///
    /// One step rather than one per knob, and it is the only undo a panel
    /// edit inside the mode gets: the whole dial-up is the act, the same way
    /// a knob sweep on a normal track coalesces into one.
    pub(crate) fn commit_source_panel(&mut self) -> bool {
        let Some(mode) = self.nav.sampler_source.as_deref() else { return false };
        let track_idx = mode.track_idx;
        let stored = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_deref())
            .and_then(SamplerState::edited)
            .and_then(|pad| pad.source.as_ref());
        if stored
            .is_some_and(|s| s.instrument == mode.instrument && s.params == mode.params)
        {
            return false;
        }
        let source = PadSource { instrument: mode.instrument, params: mode.params.clone() };

        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        if let Some(state) = self
            .nav
            .tracks
            .get_mut(track_idx)
            .and_then(|t| t.sampler.as_deref_mut())
            .and_then(SamplerState::edited_mut)
        {
            state.source = Some(source);
        }
        self.nav.commit_undo(before, "pad source panel");
        true
    }

    /// Source mode's panel follows the pad when history moves under it.
    ///
    /// The pad is the truth and [`SourceMode::params`] is a working copy, so
    /// undoing the step that wrote a panel onto the pad has to take the panel
    /// on the screen — and the sound in the slot — back with it. Without
    /// this, `u` is a key that visibly does nothing and then loses its work
    /// again the moment the mode ends.
    ///
    /// Only while the restored source names the same instrument. Undoing past
    /// the moment the mode began names a different one, and putting a
    /// different instrument in the slot is the mode's own job — `i`'s — not
    /// history's.
    pub(crate) fn resync_source_panel(&mut self, track_idx: usize) {
        let Some(mode) = self.nav.sampler_source.as_deref() else { return };
        if mode.track_idx != track_idx {
            return;
        }
        let Some(source) = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_deref())
            .and_then(SamplerState::edited)
            .and_then(|pad| pad.source.as_ref())
        else {
            return;
        };
        if source.instrument != mode.instrument || source.params == mode.params {
            return;
        }
        let params = source.params.clone();
        if let Some(mode) = self.nav.sampler_source.as_deref_mut() {
            mode.params.clone_from(&params);
        }
        self.send_params_to_slot(track_idx, &params);
        self.nav.clamp_panel_cursor();
    }

    /// `esc` in source mode: the sampler comes back, with its kit.
    ///
    /// The instance the mixer builds is empty, so every occupied pad is
    /// replayed into it. A take landed during the mode is already in that
    /// set, which is why nothing has to be shipped twice.
    pub(crate) fn leave_sampler_source(&mut self) {
        // The panel goes home before the mode does, or a sound dialled up and
        // never recorded dies with the mode that was playing it.
        let dialled = self.commit_source_panel();
        let Some(mode) = self.nav.sampler_source.take() else { return };
        self.reload_child_instrument(mode.track_idx);
        self.restore_sampler_pads(mode.track_idx);
        // Back to the sampler's two globals, which is a much shorter panel
        // than the one the cursor may have been walking.
        self.nav.clamp_panel_cursor();
        self.flash(if dialled {
            format!(
                "sampler back \u{00b7} the pads are playing again \u{00b7} pad {} kept the panel you dialled",
                SamplerState::pad_label(mode.pad),
            )
        } else {
            "sampler back \u{00b7} the pads are playing again".to_string()
        });
    }

    /// What source mode says to a key it does not take.
    ///
    /// One sentence, one place, because it is said from two: the mode's own
    /// key table answers every key it has no use for with it, and the space
    /// menu is refused with it from anywhere — a mode that took the track's
    /// plugin slot has to be able to say so however the player arrived at
    /// the key.
    ///
    /// It is also where the instrument's own panel is advertised. A player
    /// pressing keys on the pad map looking for a way to change the sound is
    /// exactly the player who needs to be told, and this is the sentence they
    /// will hit first — the banner has no room left for it.
    pub(crate) fn flash_sampler_source_keys(&mut self) {
        let Some(mode) = self.nav.sampler_source.as_deref() else { return };
        let pad = phosphor_app::sampler::SamplerState::pad_label(mode.pad);
        let take = mode.take.label();
        let ends = if mode.is_armed() { "ends the take" } else { "records" };
        self.flash(format!(
            "source mode is on pad {pad} \u{00b7} take: {take} \u{00b7} r {ends} \u{00b7} tab dials it \u{00b7} esc puts the sampler back",
        ));
    }

    /// Whether the mode is on for the track under the cursor.
    pub(crate) fn in_sampler_source(&self) -> bool {
        self.nav
            .sampler_source
            .as_deref()
            .is_some_and(|mode| Some(mode.track_idx) == self.cursor_sampler_track())
    }

    /// Drop the mode without putting the sampler back — the session load's
    /// exit, where the track it was running on is about to stop existing.
    pub(crate) fn abandon_sampler_source(&mut self) {
        self.nav.sampler_source = None;
    }

    /// Drop the mode if the track it was borrowing no longer exists, or is
    /// no longer a sampler.
    ///
    /// Asked once per keystroke rather than remembered at each of the ways
    /// a track can go — deleted, undone away, replaced by a session load.
    /// A mode pointing at a track that is gone is a take waiting to land on
    /// a pad nobody can see, and the index it holds would be somebody
    /// else's track by then.
    pub(crate) fn reconcile_sampler_source(&mut self) {
        let Some(mode) = self.nav.sampler_source.as_deref() else { return };
        let alive = self
            .nav
            .tracks
            .get(mode.track_idx)
            .is_some_and(|t| t.sampler.is_some() && mode.pad < phosphor_app::sampler::NUM_PADS);
        if !alive {
            self.nav.sampler_source = None;
        }
    }

    // ── Arming ──

    /// `r` in source mode: start a take, or end the one that is running.
    pub(crate) fn toggle_sampler_take(&mut self) {
        if self.nav.sampler_source.as_deref().is_some_and(SourceMode::is_armed) {
            self.land_armed_take();
            return;
        }
        let Some(mode) = self.nav.sampler_source.as_deref() else {
            self.flash("i picks an instrument to record this pad from");
            return;
        };
        let (track_idx, kind) = (mode.track_idx, mode.take);
        // Refused *before* the performance, not after it: finding out that
        // a pad was full once the playing is over is losing the take. Which
        // bed is full depends on what `r` is about to land — eight layers
        // and four phrases are different beds on the same pad.
        let full = self
            .nav
            .tracks
            .get(track_idx)
            .and_then(|t| t.sampler.as_ref())
            .and_then(|s| match kind {
                TakeKind::Audio => s.room_here().err(),
                TakeKind::Phrase => s.phrase_room_here().err(),
            });
        if let Some(message) = full {
            self.flash(format!("{message} \u{00b7} d removes one"));
            return;
        }
        let rate = self.nav.sample_rate as f32;
        let tempo = self.engine.transport.tempo_bpm();
        let rolling = self.engine.transport.is_playing();
        let capture = if rolling {
            TakeCapture::bars(
                phosphor_midi::clock::now_micros(),
                self.engine.transport.position_ticks(),
                tempo,
                rate,
            )
        } else {
            TakeCapture::free(tempo, rate)
        };
        let message = if rolling {
            "recording at the bar line \u{00b7} whole bars, so it loops \u{00b7} r ends it"
        } else {
            "recording \u{00b7} the take starts on your first note \u{00b7} r ends it"
        };
        if let Some(mode) = self.nav.sampler_source.as_deref_mut() {
            mode.capture = Some(capture);
        }
        self.flash(message);
    }

    /// The take is over: render it and put it on the pad.
    ///
    /// Called by `r`, by `esc`, and by the transport stopping — a stop is
    /// the end of a bar-quantised performance whichever key caused it.
    pub(crate) fn land_armed_take(&mut self) {
        let Some(mode) = self.nav.sampler_source.as_deref_mut() else { return };
        let Some(capture) = mode.capture.take() else { return };
        let track_idx = mode.track_idx;
        let (instrument, params, kind) = (mode.instrument, mode.params.clone(), mode.take);
        // The panel this take was played on is the pad's from now on: the
        // next `i` on it opens the sound that was just recorded, not the one
        // the mode started with. Its own step, before the take's.
        self.commit_source_panel();

        let Some(plan) = capture.close(phosphor_midi::clock::now_micros()) else {
            self.flash("nothing played \u{00b7} no take landed \u{00b7} r tries again");
            return;
        };
        // A take is rendered through the track's own MIDI devices, because
        // that is what the player was listening to while they played it —
        // and a phrase is baked through the same rack for the same reason.
        let Some(rack) = self.nav.tracks.get(track_idx).map(|t| t.midi_fx.clone()) else {
            return;
        };
        if kind == TakeKind::Phrase {
            self.land_phrase(track_idx, &plan, &rack, PadSource { instrument, params });
            return;
        }
        let take = render::render_take(&plan, &rack, instrument, &params);

        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) =
            self.nav.tracks.get_mut(track_idx).and_then(|t| t.sampler.as_deref_mut())
        else {
            return;
        };
        let landed = match sampler.add_take_here(&take) {
            Ok(index) => index,
            Err(message) => {
                self.flash(message);
                return;
            }
        };
        let title = sampler.edit_title();
        let name = sampler
            .edited()
            .map_or_else(String::new, |state| state.layers[landed].name.clone());
        // One step, and the step is also the only other hand on the audio:
        // a take undone off a pad stays alive in history, so the engine's
        // own drop is never the last one.
        self.nav.commit_undo(before, "record take");
        // The panel points at what just arrived — the thing a player is
        // about to trim, turn down, or record over.
        self.nav.clip_view.sampler.layer = landed;
        self.sync_sampler_edit(track_idx);
        self.clamp_sampler_cursors();

        let peak = match take.peak_db() {
            Some(db) => format!("{db:+.1} dB"),
            None => "silent".to_string(),
        };
        let capped = if plan.capped {
            match plan.start_tick {
                Some(_) => " \u{00b7} stopped at 64 bars",
                None => " \u{00b7} stopped at 60s",
            }
        } else {
            ""
        };
        self.flash(format!(
            "{title} \u{00b7} {name} \u{00b7} {:.2}s \u{00b7} peak {peak}{capped} \u{00b7} u undoes",
            take.seconds(),
        ));
    }

    /// The take was a phrase: keep the notes, and point the sampler's one
    /// child at the instrument they were played on.
    ///
    /// One undo step for both halves, because they are one act. A phrase
    /// with no child is silently inert in the engine — it has nothing to
    /// play through — so landing one always sets it, and the flash says so
    /// whenever that changed what every other phrase on the kit sounds
    /// like. There is exactly one child by design; being told is the price
    /// of that, and being told is cheaper than finding out.
    fn land_phrase(
        &mut self,
        track_idx: usize,
        plan: &TakePlan,
        rack: &[phosphor_app::state::MidiFxInstance],
        source: PadSource,
    ) {
        let phrase = render::render_phrase(plan, rack);
        let instrument = source.instrument;
        // A rack that swallowed the whole performance leaves nothing to
        // play. The engine's answer to a phrase with no events is silence,
        // so it does not land: a row that makes no sound and has no
        // explanation is worse than a take the player has to do again.
        if phrase.note_count() == 0 {
            self.flash(
                "the devices left no notes \u{00b7} no phrase landed \u{00b7} r tries again",
            );
            return;
        }

        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) =
            self.nav.tracks.get_mut(track_idx).and_then(|t| t.sampler.as_deref_mut())
        else {
            return;
        };
        let landed = match sampler.add_phrase_here(
            Arc::clone(&phrase.events),
            phrase.frames,
            phrase.sample_rate,
            phrase.root,
        ) {
            Ok(index) => index,
            Err(message) => {
                self.flash(message);
                return;
            }
        };
        let change = sampler.set_child(source);
        let title = sampler.edit_title();
        let (name, row) = match sampler.edited() {
            Some(state) => {
                (state.phrases[landed].name.clone(), state.layers.len() + landed)
            }
            None => return,
        };
        // One step, and the step is also the only other hand on the event
        // list: a phrase undone off a pad stays alive in history, so the
        // engine's own drop is never the last one — the take's contract,
        // kept for notes instead of samples.
        self.nav.commit_undo(before, "record phrase");
        // The panel points at what just arrived — the thing a player is
        // about to turn down, mute, or record over.
        self.nav.clip_view.sampler.layer = row;
        if change != ChildChange::Same {
            self.sync_sampler_child(track_idx);
        }
        self.sync_sampler_edit(track_idx);
        self.clamp_sampler_cursors();

        let capped = if plan.capped {
            match plan.start_tick {
                Some(_) => " \u{00b7} stopped at 64 bars",
                None => " \u{00b7} stopped at 60s",
            }
        } else {
            ""
        };
        let child = match change {
            ChildChange::Same => String::new(),
            ChildChange::Panel => format!(
                " \u{00b7} the child took {}'s panel \u{00b7} every phrase plays through it",
                instrument.label(),
            ),
            ChildChange::Instrument => format!(
                " \u{00b7} child is now {} \u{00b7} every phrase plays through it",
                instrument.label(),
            ),
        };
        self.flash(format!(
            "{title} \u{00b7} {name} \u{00b7} {:.2}s \u{00b7} {} notes{capped}{child} \u{00b7} u undoes",
            phrase.seconds(plan.sample_rate),
            phrase.note_count(),
        ));
    }

    /// One note from the MIDI tap, while source mode is on.
    ///
    /// The mode takes the stream the way the practice room does — see
    /// [`App::handle_tap_event`] — because the keys are a performance now:
    /// they must not step-record, and they must not walk the pad cursor out
    /// from under the take that is going to land on it. Unarmed, the note
    /// is simply played and nothing is kept.
    pub(crate) fn sampler_source_note(&mut self, note: u8, velocity: u8, on: bool, at: u64) {
        let Some(mode) = self.nav.sampler_source.as_deref_mut() else { return };
        let Some(capture) = mode.capture.as_mut() else { return };
        capture.note(CapturedEvent { micros: at, note, velocity, on });
    }

    // ── Normalize ──

    /// `n` on a layer: bring its trimmed region up to just under full
    /// scale, or back to unity.
    ///
    /// A gain value, never a rewrite. The buffer is shared — by the engine,
    /// by undo history, one day by two pads pointing into one recording —
    /// and rewriting it would change every one of them, destructively, for
    /// a decision the player may want back. The level knob and this are the
    /// same number; `n` is the one that does the arithmetic for you.
    pub(crate) fn normalize_sampler_layer(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        self.clamp_sampler_cursors();
        let cursor = self.nav.clip_view.sampler.layer;
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_ref() else { return };
        let Some(layer) = sampler.edited().and_then(|p| p.layers.get(cursor)) else {
            // A phrase has no level to measure — its control scales the
            // velocities it plays — so say that rather than "nothing here",
            // which on a pad that plainly has sounds on it reads as a bug.
            self.flash(match self.sampler_row() {
                Some(phosphor_app::sampler::PadRow::Phrase(_)) => {
                    "a phrase has no level to normalize \u{00b7} vel scales what it plays"
                        .to_string()
                }
                _ => format!("nothing on {} to normalize", sampler.edit_title()),
            });
            return;
        };
        if layer.pcm.is_none() {
            self.flash("this layer has lost its file \u{00b7} nothing to measure");
            return;
        }
        let peak = layer.region_peak();
        if peak <= 0.0 {
            self.flash("this take is silent \u{00b7} there is nothing to normalize");
            return;
        }
        // Half a decibel of headroom: a sample that peaks at exactly full
        // scale clips the moment anything downstream — a pan law, an
        // insert, the pad's own level — touches it.
        //
        // The level control's own ceiling is the ceiling here, because this
        // *is* that control: a gain the knob cannot reach is a number a
        // player cannot turn back down by hand. Our instruments render with
        // a lot of headroom, so a single quiet note can want more than the
        // twelve decibels there are — and then it goes as far as it goes and
        // the flash says how far short that left it.
        let wanted = (phosphor_core::fx::db_to_gain(-0.5) / peak).min(phosphor_app::sampler::knobs::MAX_GAIN);
        let short_by = 20.0 * (peak * wanted).log10() + 0.5;
        let back_to_unity = (layer.gain - 1.0).abs() > 1e-4;

        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let Some(layer) = sampler.edited_mut().and_then(|p| p.layers.get_mut(cursor)) else {
            return;
        };
        layer.gain = if back_to_unity { 1.0 } else { wanted };
        let (gain, name) = (layer.gain, layer.name.clone());
        self.nav.commit_undo(before, "normalize layer");
        self.sync_sampler_edit(track_idx);
        self.stop_sampler_preview();
        self.flash(if back_to_unity {
            format!("{name} \u{00b7} level back to unity \u{00b7} n normalizes again")
        } else if short_by < -0.1 {
            format!(
                "{name} \u{00b7} level {} \u{00b7} as far as it goes \u{00b7} peak {short_by:+.1} dB \u{00b7} n undoes it",
                phosphor_app::format::db_text(gain),
            )
        } else {
            format!(
                "{name} \u{00b7} normalized to -0.5 dB \u{00b7} level {} \u{00b7} n undoes the gain",
                phosphor_app::format::db_text(gain),
            )
        });
    }
}
