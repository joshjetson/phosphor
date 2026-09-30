//! The chop screen's operations and keys.
//!
//! Two ways in, one screen: `c` chops the sound on the pad under the cursor,
//! and `C` picks a recording from the samples folder to chop. Either way the
//! screen opens over the pad map holding a [`ChopPlan`], and nothing in the
//! kit changes until `c` lands it — as one undo step, or not at all.
//!
//! # The grammar
//!
//! ```text
//! free    j/k picks a row · h/l changes it · H/L by an octave or a stride
//!         on the cuts row h/l walk the slices, each one heard as it is reached
//!         enter holds the cut under the cursor
//!         a adds a cut · d removes one · p plays the slice again
//!         c lands the chop · esc leaves with nothing changed
//!
//! held    h/l move the cut (the slice's start) · H/L move the slice's end
//!         j/k walk the unit deeper/wider · enter or esc lets go
//! ```
//!
//! Held is the loop brace's grammar and the trim strip's: the primary edge
//! needs no shift, the other one does, and `j`/`k` go deeper. A slice's end
//! is the next slice's cut, so `H`/`L` move that one — which is what a
//! player looking at a slice that runs too long means by "move its end".

use super::*;

use std::path::{Path, PathBuf};

use phosphor_app::sampler::chop::controls::ChopRow;
use phosphor_app::sampler::chop::plan::ChopPlan;
use phosphor_app::sampler::trim::NudgeUnit;
use phosphor_app::sampler::{LayerState, SamplerState, NUM_PADS};
use phosphor_app::state::{ChopScreen, PickerPurpose};
use phosphor_plugin::sample::{PadConfig, PreviewLayer, PreviewMode};

use crate::state::undo::UndoScope;

impl App {
    /// The chop screen, when it is open for the sampler under the cursor.
    fn chop_here(&self) -> Option<&ChopScreen> {
        self.cursor_sampler_track()?;
        self.nav.chop_here()
    }

    /// Whether the keys on the pad map belong to the chop screen.
    pub(crate) fn chop_has_the_keys(&self) -> bool {
        self.chop_here().is_some()
    }

    fn chop_mut(&mut self) -> Option<&mut ChopScreen> {
        self.chop_here()?;
        self.nav.sampler_chop.as_deref_mut()
    }

    // ── Ways in ──

    /// `c` on the pad map: chop the sound under the cursor.
    pub(crate) fn open_chop_here(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let layer_idx = self.nav.clip_view.sampler.layer;
        let layer = self.nav.tracks[track_idx]
            .sampler
            .as_ref()
            .and_then(|s| s.edited())
            .and_then(|p| p.layers.get(layer_idx))
            .cloned();
        match layer {
            Some(layer) if layer.pcm.is_some() => self.open_chop_screen(track_idx, layer),
            Some(_) => self.flash("this sound has lost its file \u{00b7} nothing to chop"),
            None => self.flash("nothing on this pad to chop \u{00b7} C picks a recording to chop"),
        }
    }

    /// `C` on the pad map: pick a recording to chop.
    pub(crate) fn open_chop_picker(&mut self) {
        if self.cursor_sampler_track().is_none() {
            return;
        }
        self.open_sample_picker_for(PickerPurpose::ChopSample);
        self.flash("choose a recording to chop \u{00b7} / types a path");
    }

    /// The typed field behind the chop picker.
    pub(crate) fn open_chop_typed_prompt(&mut self) {
        self.nav.input_modal.open_named(InputModalKind::ChopPath, "");
        self.flash("a bare name looks in samples/ and tries .wav");
    }

    /// Enter in the chop picker or its typed field: decode the file and open
    /// the screen on it. The pad under the cursor is not touched — the
    /// recording goes onto keys only when the chop lands.
    pub(crate) fn do_chop_file(&mut self, typed: &str) {
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let Some(track_idx) = self.cursor_sampler_track() else {
            self.flash("no sampler under the cursor");
            return;
        };
        let resolved = phosphor_app::paths::find_sample(Path::new(typed));
        match phosphor_app::sampler::wav::load_wav(&resolved) {
            // The path as typed, as a pad load keeps it: a bare name stays a
            // bare name in the session, and keeps resolving on any machine.
            Ok(pcm) => self.open_chop_screen(track_idx, LayerState::from_wav(PathBuf::from(typed), pcm)),
            Err(message) => self.flash(&message),
        }
    }

    fn open_chop_screen(&mut self, track_idx: usize, source: LayerState) {
        let Some(track_id) = self.nav.tracks[track_idx].mixer_id else { return };
        let name = source.name.clone();
        let Some(mut plan) = ChopPlan::new(source) else {
            self.flash("this sound has no audio to chop");
            return;
        };
        if let Some(sampler) = self.nav.tracks[track_idx].sampler.as_deref() {
            plan.place(sampler);
        }
        // One mode on the map at a time: a trim strip left open under the
        // chop would take the keys back the moment the chop closed.
        self.nav.clip_view.sampler.trim = None;
        self.nav.clip_view.sampler.locked = false;
        self.nav.sampler_chop = Some(Box::new(ChopScreen::new(track_id, plan)));
        self.audition_chop_slice();
        let count = self.chop_here().map_or(0, |c| c.plan.cuts().len());
        self.flash(format!(
            "chop \u{00b7} {name} \u{00b7} {count} slice{} \u{00b7} j/k picks a row, h/l changes it \u{00b7} c lands it",
            if count == 1 { "" } else { "s" },
        ));
    }

    /// `esc`: leave with nothing changed.
    fn close_chop(&mut self, words: &str) {
        self.nav.sampler_chop = None;
        self.stop_sampler_preview();
        self.flash(words);
    }

    // ── The audition ──

    /// Sound the slice under the cursor, once.
    fn audition_chop_slice(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let Some(mixer_id) = self.nav.tracks[track_idx].mixer_id else { return };
        let Some(chop) = self.chop_here() else { return };
        let (Some((start, end)), Some(layer)) =
            (chop.plan.selected_slice(), chop.plan.source().engine_layer())
        else {
            self.stop_sampler_preview();
            return;
        };
        let note = SamplerState::note_of_pad((chop.plan.first + chop.plan.selected).min(NUM_PADS - 1));
        let preview = PreviewLayer {
            config: PadConfig::for_key(note),
            layer: phosphor_plugin::sample::PadLayer {
                start_frame: start,
                end_frame: end,
                reverse: false,
                mute: false,
                ..layer
            },
            mode: PreviewMode::Once,
        };
        self.send_sampler_preview(mixer_id, preview);
    }

    // ── Landing ──

    /// `c` in the screen: put the slices on the keys, or say why not.
    fn land_chop(&mut self) {
        let Some(track_idx) = self.cursor_sampler_track() else { return };
        let Some(chop) = self.nav.sampler_chop.as_deref() else { return };
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_deref() else { return };
        // Asked before a checkpoint is spent: a refusal changes nothing and
        // must leave nothing on the undo stack either.
        if let Some(reason) = chop.plan.refusal(sampler) {
            self.flash(reason);
            return;
        }
        let plan = chop.plan.clone();
        let before = self.nav.undo_checkpoint(UndoScope::Sampler { track_idx });
        let Some(sampler) = self.nav.tracks[track_idx].sampler.as_mut() else { return };
        let landing = match plan.land(sampler) {
            Ok(landing) => landing,
            Err(reason) => {
                self.flash(reason);
                return;
            }
        };
        sampler.cursor = landing.first;
        // The keys the chop covers — and, when the bed turned from keys to
        // pads, every key either mode was sounding, because the zones have
        // just gone quiet and the engine has to hear that too.
        let pads: Vec<usize> = if landing.switched_to_pads {
            sampler.sounding_pads()
        } else {
            (landing.first..landing.first + landing.count).collect()
        };
        self.nav.commit_undo(before, "chop");
        self.sync_sampler_pads(track_idx, pads);
        self.nav.sampler_chop = None;
        self.nav.clip_view.sampler.layer = 0;
        self.stop_sampler_preview();

        let choke = match landing.choke {
            Some(group) => format!(" \u{00b7} choke {group}"),
            None if plan.feel == phosphor_app::sampler::chop::land::Feel::Break => {
                " \u{00b7} every choke group was taken".into()
            }
            None => String::new(),
        };
        let bed = if landing.switched_to_pads { " \u{00b7} the bed is on pads" } else { "" };
        self.flash(format!(
            "chopped \u{00b7} {} slice{} on {}{choke}{bed} \u{00b7} u takes it back",
            landing.count,
            if landing.count == 1 { "" } else { "s" },
            phosphor_app::sampler::chop::land::span_label(landing.first, landing.count),
        ));
    }

    // ── Keys ──

    /// One key, in the chop screen. Every key is answered here: a key that
    /// fell through to the pad map would edit a kit the screen is hiding.
    pub(crate) fn handle_chop_keys(&mut self, key: crossterm::event::KeyEvent) {
        let Some(chop) = self.chop_mut() else { return };
        chop.clamp();
        if chop.held {
            return self.handle_held_cut_keys(key);
        }
        let row = chop.current();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_chop_row(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_chop_row(-1),
            KeyCode::Char('H') => self.change_chop_row(row, -1, true),
            KeyCode::Char('L') => self.change_chop_row(row, 1, true),
            KeyCode::Char('h') | KeyCode::Left => self.change_chop_row(row, -1, false),
            KeyCode::Char('l') | KeyCode::Right => self.change_chop_row(row, 1, false),
            KeyCode::Enter => self.hold_chop_cut(),
            KeyCode::Char('a') => self.add_chop_cut(),
            KeyCode::Char('d') => self.remove_chop_cut(),
            KeyCode::Char('p') => self.audition_chop_slice(),
            KeyCode::Char('c') => self.land_chop(),
            KeyCode::Esc | KeyCode::Char('q') => self.close_chop("chop cancelled \u{00b7} nothing changed"),
            _ => self.flash(
                "chop \u{00b7} j/k row \u{00b7} h/l change \u{00b7} enter holds a cut \u{00b7} c lands it \u{00b7} esc cancels",
            ),
        }
    }

    fn move_chop_row(&mut self, delta: i32) {
        if let Some(chop) = self.chop_mut() {
            chop.move_row(delta);
        }
    }

    /// `h`/`l` on a row: walk the slices on the cuts row, change the setting
    /// on any other.
    fn change_chop_row(&mut self, row: ChopRow, delta: i32, stride: bool) {
        let Some(chop) = self.chop_mut() else { return };
        if row == ChopRow::Cuts {
            chop.plan.select(delta * if stride { 4 } else { 1 });
            return self.audition_chop_slice();
        }
        if !chop.plan.adjust(row, delta, stride) {
            return;
        }
        chop.clamp();
        let words = format!(
            "{} {} \u{00b7} {} slice{}",
            row.label(),
            row.value(&chop.plan),
            chop.plan.cuts().len(),
            if chop.plan.cuts().len() == 1 { "" } else { "s" },
        );
        self.flash(words);
    }

    fn hold_chop_cut(&mut self) {
        let Some(chop) = self.chop_mut() else { return };
        if chop.current() != ChopRow::Cuts {
            return self.flash("enter holds a cut \u{00b7} it works on the cuts row");
        }
        if chop.plan.cuts().is_empty() {
            return self.flash("no cuts to hold \u{00b7} a adds one");
        }
        chop.held = true;
        let unit = chop.unit.label();
        self.flash(format!(
            "held \u{00b7} h/l moves the cut \u{00b7} H/L moves the slice's end \u{00b7} j/k unit {unit} \u{00b7} esc lets go",
        ));
    }

    fn add_chop_cut(&mut self) {
        let Some(chop) = self.chop_mut() else { return };
        // The cuts row is where the new cut is, so that is where the cursor
        // goes: the next thing a player does with it is hold it.
        chop.row = 0;
        if chop.plan.add_cut() {
            self.audition_chop_slice();
        } else {
            self.flash("no room for a cut there \u{00b7} cuts stay 40 ms apart");
        }
    }

    fn remove_chop_cut(&mut self) {
        let Some(chop) = self.chop_mut() else { return };
        if chop.plan.remove_cut() {
            chop.clamp();
            self.audition_chop_slice();
        } else {
            self.flash("no cuts to remove");
        }
    }

    /// The held keys: the cut and the slice's end, and the unit they move by.
    fn handle_held_cut_keys(&mut self, key: crossterm::event::KeyEvent) {
        let bpm = self.engine.transport.tempo_bpm();
        let Some(chop) = self.chop_mut() else { return };
        let step = chop.unit.frames(bpm, chop.plan.pcm().sample_rate) as i64;
        // A single-sample move is the one a snap would undo.
        let snap = chop.unit != NudgeUnit::Sample;
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.nudge_chop_cut(0, -step, snap),
            KeyCode::Char('l') | KeyCode::Right => self.nudge_chop_cut(0, step, snap),
            KeyCode::Char('H') => self.nudge_chop_cut(1, -step, snap),
            KeyCode::Char('L') => self.nudge_chop_cut(1, step, snap),
            KeyCode::Char('j') | KeyCode::Down => chop.unit = chop.unit.stepped(1),
            KeyCode::Char('k') | KeyCode::Up => chop.unit = chop.unit.stepped(-1),
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                chop.held = false;
                self.status_message = None;
            }
            _ => {}
        }
    }

    /// Move a cut: `which` 0 is the slice's own start, 1 its end — the next
    /// slice's cut. The last slice ends where the region does, and that end
    /// is the source's trim, not a cut.
    fn nudge_chop_cut(&mut self, which: usize, delta: i64, snap: bool) {
        let Some(chop) = self.chop_mut() else { return };
        let here = chop.plan.selected;
        let target = here + which;
        if target >= chop.plan.cuts().len() {
            return self.flash("the last slice ends where the sound does \u{00b7} t trims the sound");
        }
        chop.plan.selected = target;
        let moved = chop.plan.nudge_cut(delta, snap);
        chop.plan.selected = here;
        let rate = chop.plan.pcm().sample_rate.max(1.0);
        let start = chop.plan.region().0;
        if let Some(frame) = moved {
            let at = (frame - start) as f32 / rate;
            let what = if which == 0 { "cut" } else { "end" };
            self.flash(format!("{what} at {at:.3}s"));
        }
        self.audition_chop_slice();
    }
}
