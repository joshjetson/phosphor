//! The eight column knobs: the locked screen's controls, eight at a time.
//!
//! Each screen's controls are read from the lists its own panel draws — an
//! instrument's parameter names, a pad's knobs, the step grid's knob rows —
//! and laid out in pages of eight (an EQ's pages are its bands, because a
//! band is the unit an EQ is thought about in). A knob turned puts the
//! panel's own cursor on its control and moves it the way the panel's keys
//! do, through the same function, so the edit is the keyboard's edit: the
//! same steps, the same undo, the same refusals, and the cursor on screen
//! shows which control moved.

use super::*;

use phosphor_app::state::{FxType, FxView, SeqBand, TrackElement};
use phosphor_app::surface::layout::COLUMNS;
use phosphor_app::surface::screen::{screen, Screen};

use super::sequencer_keys::knobs_of;

const PAGE: usize = COLUMNS as usize;

/// What one column knob turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ctl {
    /// An instrument parameter, by panel index.
    Synth(usize),
    /// A row of an effect panel that is a list: reverb, delay, tape,
    /// compressor, a MIDI effect.
    FxRow(usize),
    /// One control of one EQ band.
    Eq { band: usize, control: usize },
    /// A pad-panel knob, by cursor index.
    Pad(usize),
    /// A step-grid knob: its band and its place on it.
    Seq(SeqBand, usize),
    /// A row of the chop screen.
    Chop(usize),
    /// A track's fader, pan or send.
    Mix { track: usize, element: TrackElement },
}

/// One knob's name and what it turns.
#[derive(Debug, Clone)]
pub(crate) struct Knob {
    pub(crate) label: String,
    pub(crate) ctl: Ctl,
}

fn knob(label: impl Into<String>, ctl: Ctl) -> Knob {
    Knob { label: label.into(), ctl }
}

/// A flat list in pages of eight.
fn paged(knobs: Vec<Knob>) -> Vec<Vec<Knob>> {
    let mut pages = Vec::new();
    let mut rest = knobs.into_iter().peekable();
    while rest.peek().is_some() {
        pages.push(rest.by_ref().take(PAGE).collect());
    }
    pages
}

impl App {
    /// The knob pages of the screen that has the keys. Empty where the
    /// screen has nothing a knob could turn.
    pub(crate) fn bank(&self) -> Vec<Vec<Knob>> {
        match screen(&self.nav) {
            Screen::Instrument => paged(self.instrument_knobs()),
            Screen::Effect => self.effect_pages(),
            Screen::MidiEffect => paged(self.midi_effect_knobs()),
            Screen::Pads | Screen::Zones => paged(
                self.sampler_knobs()
                    .iter()
                    .enumerate()
                    .map(|(i, k)| knob(k.label(), Ctl::Pad(i)))
                    .collect(),
            ),
            Screen::Steps => self.step_pages(),
            Screen::Chop => paged(self.chop_knobs()),
            Screen::Track | Screen::TrackCell => paged(self.strip_knobs()),
            Screen::Tracks => paged(self.pan_knobs()),
            _ => Vec::new(),
        }
    }

    fn instrument_knobs(&self) -> Vec<Knob> {
        let Some(view) = self.nav.panel() else { return Vec::new() };
        let names = phosphor_app::preset::param_names(view.instrument);
        (0..view.params.len())
            .map(|i| knob(names.get(i).map_or_else(|| (i + 1).to_string(), |n| (*n).to_string()), Ctl::Synth(i)))
            .collect()
    }

    /// An EQ's pages are its bands and then its trim; every other effect's
    /// panel is a list.
    fn effect_pages(&self) -> Vec<Vec<Knob>> {
        use crate::ui::fx::comp_row_name;
        use phosphor_dsp::fx::{delay, reverb, tape};
        let rows = |count: usize, name: &dyn Fn(usize) -> &'static str| {
            paged((0..count).map(|i| knob(name(i), Ctl::FxRow(i))).collect())
        };
        match self.open_fx_type() {
            Some(FxType::Reverb) => {
                rows(reverb::PARAM_COUNT, &|i| reverb::natural_param(i).map_or("", |p| p.name))
            }
            Some(FxType::Delay) => rows(delay::PARAM_COUNT, &|i| delay::natural_param(i).map_or("", |p| p.name)),
            Some(FxType::Tape) => rows(tape::PARAM_COUNT, &|i| tape::natural_param(i).map_or("", |p| p.name)),
            Some(FxType::Compressor) => rows(crate::ui::fx::COMP_ROWS, &comp_row_name),
            Some(_) => {
                let mut pages: Vec<Vec<Knob>> = (0..FxView::TRIM)
                    .map(|band| {
                        crate::ui::fx::CONTROL_NAMES
                            .iter()
                            .enumerate()
                            .map(|(control, name)| knob(format!("{}{name}", band + 1), Ctl::Eq { band, control }))
                            .collect()
                    })
                    .collect();
                pages.push(vec![knob("trim", Ctl::Eq { band: FxView::TRIM, control: 0 })]);
                pages
            }
            None => Vec::new(),
        }
    }

    fn midi_effect_knobs(&self) -> Vec<Knob> {
        let Some(fx) = self
            .nav
            .clip_view
            .fx
            .midi_slot
            .and_then(|slot| self.nav.current_track()?.midi_fx.get(slot))
        else {
            return Vec::new();
        };
        fx.fx_type.params().iter().enumerate().map(|(i, p)| knob(p.name, Ctl::FxRow(i))).collect()
    }

    /// The step under the cursor on page one, the pattern after it.
    fn step_pages(&self) -> Vec<Vec<Knob>> {
        let Some(state) = self.nav.current_track().and_then(|t| t.sequencer.as_deref()) else {
            return Vec::new();
        };
        let band = |b: SeqBand| -> Vec<Knob> {
            knobs_of(state, b).iter().enumerate().map(|(i, k)| knob(k.label(), Ctl::Seq(b, i))).collect()
        };
        let mut pages = paged(band(SeqBand::Step));
        pages.extend(paged(band(SeqBand::Pattern)));
        pages
    }

    fn chop_knobs(&self) -> Vec<Knob> {
        let Some(chop) = self.nav.chop_here() else { return Vec::new() };
        phosphor_app::sampler::chop::controls::rows(chop.plan.mode)
            .iter()
            .enumerate()
            .map(|(i, row)| knob(row.label(), Ctl::Chop(i)))
            .collect()
    }

    /// The selected track's strip: its fader, pan and sends.
    fn strip_knobs(&self) -> Vec<Knob> {
        let track = self.nav.track_cursor;
        [
            ("vol", TrackElement::Volume),
            ("pan", TrackElement::Pan),
            ("send A", TrackElement::SendA),
            ("send B", TrackElement::SendB),
        ]
        .into_iter()
        .map(|(label, element)| knob(label, Ctl::Mix { track, element }))
        .collect()
    }

    /// The song: each fader's track's pan, above the fader.
    fn pan_knobs(&self) -> Vec<Knob> {
        self.deck_bank_tracks()
            .map(|track| knob(self.nav.tracks[track].name.clone(), Ctl::Mix { track, element: TrackElement::Pan }))
            .collect()
    }

    /// Column knob `n`, turned.
    pub(crate) fn turn_knob(&mut self, n: u8, detents: i8) {
        let now = screen(&self.nav);
        let bank = self.bank();
        let Some(k) = bank.get(self.deck.page).and_then(|page| page.get(usize::from(n))) else {
            return self.flash(format!("{}: knob {} turns nothing here", now.label(), n + 1));
        };
        self.deck.last_knob = Some((now, k.ctl));
        self.turn_ctl(k.ctl, detents);
    }

    /// LAST on the function knob: the column knob turned most recently,
    /// while its screen is still up.
    pub(crate) fn turn_last(&mut self, detents: i8) {
        match self.deck.last_knob {
            Some((on, ctl)) if on == screen(&self.nav) => self.turn_ctl(ctl, detents),
            _ => self.flash("LAST: turn a knob on this screen first"),
        }
    }

    fn turn_ctl(&mut self, ctl: Ctl, detents: i8) {
        let sign = i32::from(detents.signum());
        let presses = detents.unsigned_abs();
        let key = if detents >= 0 { 'l' } else { 'h' };
        for _ in 0..presses {
            match ctl {
                Ctl::Synth(i) => {
                    self.nav.clip_view.synth_param_cursor = i;
                    self.press_chord(phosphor_app::surface::binding::Chord::ch(key));
                }
                Ctl::FxRow(i) => {
                    self.nav.clip_view.fx.band = i;
                    self.press_chord(phosphor_app::surface::binding::Chord::ch(key));
                }
                Ctl::Eq { band, control } => {
                    self.nav.clip_view.fx.band = band;
                    self.nav.clip_view.fx.control = if band >= FxView::TRIM { 0 } else { control };
                    self.adjust_fx_control(sign, false);
                }
                Ctl::Pad(i) => {
                    self.nav.clip_view.sampler.knob = i;
                    self.adjust_sampler_knob(sign, false);
                }
                Ctl::Seq(band, i) => {
                    self.nav.clip_view.sequencer.band = band;
                    self.nav.clip_view.sequencer.knob = i;
                    self.sequencer_adjust(detents.signum(), false);
                }
                Ctl::Chop(i) => {
                    if let Some(chop) = self.nav.sampler_chop.as_deref_mut().filter(|c| !c.held) {
                        chop.row = i;
                    }
                    self.press_chord(phosphor_app::surface::binding::Chord::ch(key));
                }
                Ctl::Mix { track, element } => self.step_mix(track, element, sign),
            }
        }
    }

    /// One step of a track's fader, pan or send, from anywhere: the strip's
    /// own step functions, pointed at that track for the moment.
    pub(crate) fn step_mix(&mut self, track: usize, element: TrackElement, steps: i32) {
        if track >= self.nav.tracks.len() {
            return;
        }
        let saved = (self.nav.track_cursor, self.nav.track_element);
        self.nav.track_cursor = track;
        self.nav.track_element = element;
        if element == TrackElement::Volume {
            self.step_fader(steps);
        } else {
            self.step_routing(steps);
        }
        (self.nav.track_cursor, self.nav.track_element) = saved;
    }

    /// A column knob's push: say what it is.
    pub(crate) fn show_knob(&mut self, n: u8) {
        let bank = self.bank();
        match bank.get(self.deck.page).and_then(|page| page.get(usize::from(n))) {
            Some(k) => self.flash(format!("knob {}: {}", n + 1, k.label)),
            None => self.flash(format!("knob {}: nothing here", n + 1)),
        }
    }

    /// PAGE ◀ ▶: the next eight controls.
    pub(crate) fn deck_page(&mut self, delta: i8) {
        let pages = self.bank().len();
        if pages <= 1 {
            return self.flash("one page of knobs here");
        }
        let at = (self.deck.page as i32 + i32::from(delta)).clamp(0, pages as i32 - 1) as usize;
        self.deck.page = at;
        self.flash(format!("knobs: page {} of {pages}", at + 1));
    }
}
