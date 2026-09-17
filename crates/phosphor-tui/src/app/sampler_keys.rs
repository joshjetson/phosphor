//! Keys → the pad map, and the trim strip that opens over it.
//!
//! # The grammar
//!
//! ```text
//! pad map    h/l walks the bed · H/L an octave · playing a key jumps to it
//!            j/k picks a control · enter holds it · h/l adjusts · H/L strides
//!            [ ] picks a layer · 1-8 jumps to one · m mutes · d removes
//!            a loads a sound onto the pad · t trims it · n normalizes it
//!            i records the pad from an instrument · esc goes back
//!            K switches the bed between pads and keys
//!
//! keys mode  the same, on zones instead of pads, plus
//!            w the whole bed · o this octave · s splits here · D removes
//!            R learns the root from the next key played
//!            the span is the first control: enter holds it, then
//!            h/l move the LOW edge and H/L the HIGH one
//!
//! trim strip h/l moves the START · H/L moves the END
//!            j/k walks the unit deeper/wider · z snaps · r reverses
//!            t loops the region · esc goes back to the map
//!
//! source     r starts the take · r again ends it · i swaps the instrument
//!            p swaps what r lands: audio, or the phrase itself
//!            esc puts the sampler back · the pad is fixed
//! ```
//!
//! `h`/`l` do two jobs on the map, and `enter` is what tells them apart:
//! free, they walk the keyboard, which is the thing a player is mostly doing
//! here; held, they turn the control under the cursor and nothing else gets
//! a look at the key. That is the fader's contract and the step grid's, and
//! it is what leaves the bed reachable without a modifier.
//!
//! In the strip they do the third job the box already knows — the loop
//! brace's. `h`/`l` take the left edge and `H`/`L` the right, because a
//! region has two ends and a player should not have to remember which mode
//! they are in to move the one they are looking at. `j`/`k` go *deeper*,
//! which on a brace is the grid and here is the nudge unit: bar, beat, a
//! sixteenth, ten milliseconds, one, one sample.
//!
//! The strip owns every key while it is open. That is why it is checked
//! first: `t`, `r` and `z` all mean something else on the map, and a key
//! that falls through from a mode is a key that edits something nobody can
//! see.
//!
//! Source mode owns them for the same reason and one more: while it is on,
//! the track is playing a synth rather than the sampler, so every key that
//! edits a pad would be editing something the player cannot hear. It takes
//! three keys and refuses the rest in words — including the ones that walk
//! the bed, because the mode belongs to the pad it was entered on and a
//! cursor that wandered off it would leave the banner naming one pad and
//! the take landing on another.
//!
//! Keys mode changes what the keys *address*, never what they mean: `a`
//! still loads a sound, `t` still trims one, `j`/`k` still walk the
//! controls — they simply land on the zone under the cursor instead of the
//! pad under it. That is why there is no second key table here. The four
//! keys it adds are the four things a pad map has no word for: the three
//! ways to make a zone, and the one that removes it.
//!
//! Every edit goes through the sampler's ops, which are the only things
//! that write a pad and tell the engine.

use super::*;

use phosphor_app::sampler::trim::TrimEdge;
use phosphor_app::sampler::MapMode;

impl App {
    /// One key, on the pad map.
    pub(crate) fn handle_sampler_keys(&mut self, key: crossterm::event::KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        self.clamp_sampler_cursors();

        if self.in_sampler_source() {
            self.handle_source_keys(key);
            return;
        }

        if self.nav.clip_view.sampler.trim.is_some() {
            self.handle_trim_keys(key);
            return;
        }

        // Root-learn is a question the screen has asked, and `esc` is the
        // answer "never mind". Checked before the held-control branch
        // because a player can arm it with a knob held and would otherwise
        // have to press `esc` twice to mean one thing.
        if self.nav.clip_view.sampler.root_learn && key.code == KeyCode::Esc {
            self.nav.clip_view.sampler.root_learn = false;
            self.flash("root learn off \u{00b7} nothing changed");
            return;
        }

        if self.nav.clip_view.sampler.locked {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    self.nav.clip_view.sampler.locked = false;
                    self.status_message = None;
                }
                KeyCode::Char('H') => self.adjust_sampler_knob(-1, true),
                KeyCode::Char('L') => self.adjust_sampler_knob(1, true),
                KeyCode::Char('h') | KeyCode::Left => self.adjust_sampler_knob(-1, shift),
                KeyCode::Char('l') | KeyCode::Right => self.adjust_sampler_knob(1, shift),
                _ => {}
            }
            return;
        }

        let knobs = self.sampler_knobs().len();
        match key.code {
            // An octave is the stride along a keyboard, the way twelve
            // semitones are one gesture on every other pitch control here.
            KeyCode::Char('H') => self.move_sampler_pad(-12),
            KeyCode::Char('L') => self.move_sampler_pad(12),
            KeyCode::Char('h') | KeyCode::Left => self.move_sampler_pad(-1),
            KeyCode::Char('l') | KeyCode::Right => self.move_sampler_pad(1),
            KeyCode::Char('j') | KeyCode::Down => {
                self.nav.clip_view.sampler.move_knob(1, knobs);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.nav.clip_view.sampler.move_knob(-1, knobs);
            }
            KeyCode::Char('[') => self.move_sampler_layer(-1),
            KeyCode::Char(']') => self.move_sampler_layer(1),
            KeyCode::Char(digit @ '1'..='8') => {
                self.select_sampler_layer(digit as usize - '1' as usize);
            }
            KeyCode::Enter => {
                self.nav.clip_view.sampler.locked = true;
                // The span is a brace and the rest are knobs, and the two
                // want different halves of the same sentence: a brace has
                // two edges, a knob has a stride.
                let span = self.sampler_knobs().get(self.nav.clip_view.sampler.knob)
                    == Some(&phosphor_app::sampler::knobs::PadKnob::Span);
                self.status_message = Some((
                    if span {
                        "held: h/l moves the low edge, H/L the high one, esc lets go".into()
                    } else {
                        "held: h/l adjusts, H/L strides, esc lets go".to_string()
                    },
                    std::time::Instant::now(),
                ));
            }
            KeyCode::Char('m') => self.toggle_sampler_layer_mute(),
            KeyCode::Char('d') => self.request_sampler_layer_delete(),
            KeyCode::Char('a') => self.open_sample_prompt(),
            KeyCode::Char('i') => self.open_pad_source_picker(),
            KeyCode::Char('n') => self.normalize_sampler_layer(),
            KeyCode::Char('t') => self.open_trim_strip(),
            KeyCode::Char('K') => self.toggle_sampler_map_mode(),
            // The zone keys. They say so rather than doing nothing in pads
            // mode, because a key that is silent is a key a player thinks
            // is broken — and `K` is the answer.
            KeyCode::Char('w') => self.zone_whole(),
            KeyCode::Char('o') => self.zone_octave(),
            KeyCode::Char('s') => self.zone_split(),
            KeyCode::Char('D') if self.sampler_mode() == MapMode::Keys => {
                self.request_zone_delete();
            }
            KeyCode::Char('R') if self.sampler_mode() == MapMode::Keys => {
                self.toggle_root_learn();
            }
            // `r` and `p` outside source mode are keys with nowhere to go,
            // and the thing a player pressing either wants is one key away.
            KeyCode::Char('r') => {
                self.flash("i picks an instrument to record this pad from");
            }
            KeyCode::Char('p') => {
                self.flash(
                    "audio or phrase is a source-mode choice \u{00b7} i picks an instrument first",
                );
            }
            KeyCode::Esc | KeyCode::Char('q') => self.nav.escape(),
            _ => {}
        }
    }

    /// One key, in source mode.
    ///
    /// Three do something and everything else says what the three are. A
    /// mode that silently swallows keys is a mode a player thinks has
    /// crashed.
    fn handle_source_keys(&mut self, key: crossterm::event::KeyEvent) {
        let armed = self
            .nav
            .sampler_source
            .as_deref()
            .is_some_and(phosphor_app::sampler::capture::SourceMode::is_armed);
        match key.code {
            KeyCode::Char('r') => self.toggle_sampler_take(),
            KeyCode::Char('p') => self.toggle_sampler_take_kind(),
            KeyCode::Char('i') => self.open_pad_source_picker(),
            // Esc ends a running take before it leaves — a performance is
            // too expensive to throw away on a key that means "back". The
            // second press is the one that leaves.
            KeyCode::Esc | KeyCode::Char('q') => {
                if armed {
                    self.land_armed_take();
                } else {
                    self.leave_sampler_source();
                }
            }
            _ => self.flash_sampler_source_keys(),
        }
    }

    /// One key, in the trim strip.
    ///
    /// `H`/`L` are matched before `h`/`l` rather than through the shift
    /// modifier, because a terminal reports the uppercase character and the
    /// modifier both, and matching on the character is the one form that
    /// works on every terminal the rest of this file was tested against.
    fn handle_trim_keys(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('H') => self.nudge_trim(TrimEdge::End, -1),
            KeyCode::Char('L') => self.nudge_trim(TrimEdge::End, 1),
            KeyCode::Char('h') | KeyCode::Left => self.nudge_trim(TrimEdge::Start, -1),
            KeyCode::Char('l') | KeyCode::Right => self.nudge_trim(TrimEdge::Start, 1),
            KeyCode::Char('j') | KeyCode::Down => self.walk_trim_unit(1),
            KeyCode::Char('k') | KeyCode::Up => self.walk_trim_unit(-1),
            KeyCode::Char('z') => self.toggle_trim_snap(),
            KeyCode::Char('r') => self.toggle_trim_reverse(),
            KeyCode::Char('t') => self.toggle_trim_loop(),
            KeyCode::Esc | KeyCode::Char('q') => self.close_trim_strip(),
            _ => {}
        }
    }
}
