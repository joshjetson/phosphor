//! A pad's controls: one list, in one order, with one set of steps.
//!
//! Named here rather than in either of the two places that use them,
//! because both have to agree: the keys turn the control under the cursor
//! by index, and the panel draws the same list in the same order. A knob
//! the keys know about and the panel does not is a knob nobody can reach —
//! the step grid's [`SeqKnob`](crate::state::SeqKnob) rule, applied to a
//! pad.
//!
//! Everything here is in natural units, because [`PadConfig`] is: a pad
//! bypasses the parameter system entirely and is delivered whole, so the
//! panel prints milliseconds and semitones and this module steps them.

use phosphor_plugin::sample::{
    clamp_pitch_cents, clamp_tune_st, TrigMode, CHOKE_MAX, PAN_RANGE, POLY_RANGE,
};

use crate::format::{db_text, ms_text, note_name, pan_label};

use super::{MapMode, PadRow, PadState, RowKind, Zone, NUM_PADS};

/// The top of an envelope stage. Ten seconds is longer than any sampler
/// envelope a player reaches for and short enough that the dial's travel
/// still means something.
///
/// The one limit on this panel that is the knob's own rather than the data
/// model's — see [`phosphor_plugin::sample::clamp_config`], which floors an
/// envelope time and deliberately does not ceiling it.
const MAX_ENV_MS: f32 = 10_000.0;

/// The top of a level control, linear.
///
/// The shared ceiling, re-exported rather than restated: the session loader
/// and the engine's delivery clamp to the same number, and a normalize stops
/// there too — a gain past the end of the travel is a gain the player cannot
/// turn back down by hand.
pub use phosphor_plugin::sample::MAX_GAIN;

/// The bottom of a level control's travel, in decibels. One step below it
/// is silence, so stepping down from the floor reaches it and stepping up
/// leaves it — the fader's rule, and for the same reason: twenty more
/// presses to reach an already inaudible level is travel nobody wants.
const FLOOR_DB: f32 = -40.0;

/// One press of a pan control: twenty detents from the centre to either
/// end. The strip's feel, chosen again here rather than borrowed, because a
/// pad's pan and a track's pan are different controls that happen to agree.
const PAN_STEP: f32 = 0.05;

/// Half the travel of a coarse pitch control, for the dials that draw
/// themselves either side of a centre. Read off the shared range rather than
/// written again, so a widened range moves the dial with it.
const TUNE_HALF: f64 = *phosphor_plugin::sample::TUNE_ST_RANGE.end() as f64;

/// The same for a fine one.
const CENTS_HALF: f64 = *phosphor_plugin::sample::PITCH_CENTS_RANGE.end() as f64;

/// What a control with nothing under it reads. Never a zero: a number is a
/// claim about a value, and there is no value here.
const DASH: &str = "\u{2014}";

/// One control on the pad panel.
///
/// The pad's own come first and the selected layer's after, which is the
/// order `j`/`k` walks: what the whole pad does, then what this one sound
/// inside it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKnob {
    /// The zone's two edges — keys mode only, and the first control there,
    /// because the brace is what the band is for.
    ///
    /// It is a control rather than a mode of its own so that it reaches the
    /// keys by the path every other control does: `enter` holds it, `h`/`l`
    /// move it, `esc` lets go. The one thing it reads differently is
    /// `H`/`L`, which on a span are not a stride but the *other edge* —
    /// the loop brace's grammar, which is what a player's hands already
    /// know about a region with two ends.
    Span,
    Trig,
    Poly,
    Choke,
    /// Round robin: successive hits hand out the pad's layers one at a time
    /// instead of stacking them all.
    ///
    /// Beside `choke` because the two are what a pad does with the sounds on
    /// it as a group — one decides what a hit silences elsewhere, the other
    /// what it wakes here — and because on hardware they sit together.
    Cycle,
    PitchSt,
    PitchCents,
    Attack,
    Decay,
    Sustain,
    Release,
    Level,
    Pan,
    Root,
    Keytrack,
    // ── The layer under the layer cursor ──
    LayerGain,
    LayerPan,
    LayerTuneSt,
    LayerTuneCents,
    LayerReverse,
    LayerMute,
    // ── The phrase under the row cursor ──
    /// What every recorded velocity is multiplied by.
    ///
    /// A velocity scale rather than a fader, because every phrase on the
    /// kit sounds through one child instrument rendered once a block: there
    /// is no per-phrase place in a shared render to put a fader. Which is
    /// also why it reads as a percentage and is called `vel`.
    PhraseGain,
    PhraseMute,
    PhraseKeytrack,
}

/// How many controls a pad or a zone offers before the row cursor's own.
const PAD_CONTROLS: usize = 14;

/// The pad's own, in the order `j`/`k` walks them.
const PAD: [PadKnob; PAD_CONTROLS] = [
    PadKnob::Trig,
    PadKnob::Poly,
    PadKnob::Choke,
    PadKnob::Cycle,
    PadKnob::PitchSt,
    PadKnob::PitchCents,
    PadKnob::Attack,
    PadKnob::Decay,
    PadKnob::Sustain,
    PadKnob::Release,
    PadKnob::Level,
    PadKnob::Pan,
    PadKnob::Root,
    PadKnob::Keytrack,
];

/// A zone's own. The span takes the top, and `keytrk` is gone: a zone
/// always tracks the keyboard — one that did not would be a stretch of keys
/// all playing one pitch, which is a pad with extra steps — so the switch
/// would be a control with nothing on the other side of it. That leaves the
/// same number of controls above the row cursor's, which is what lets one
/// constant split every list here.
const ZONE: [PadKnob; PAD_CONTROLS] = [
    PadKnob::Span,
    PadKnob::Trig,
    PadKnob::Poly,
    PadKnob::Choke,
    PadKnob::Cycle,
    PadKnob::PitchSt,
    PadKnob::PitchCents,
    PadKnob::Attack,
    PadKnob::Decay,
    PadKnob::Sustain,
    PadKnob::Release,
    PadKnob::Level,
    PadKnob::Pan,
    PadKnob::Root,
];

/// What a sampled layer adds under the pad's own.
const LAYER_TAIL: [PadKnob; 6] = [
    PadKnob::LayerGain,
    PadKnob::LayerPan,
    PadKnob::LayerTuneSt,
    PadKnob::LayerTuneCents,
    PadKnob::LayerReverse,
    PadKnob::LayerMute,
];

/// What a phrase adds instead.
///
/// Three, not six. A phrase is note traffic for an instrument shared by
/// every phrase on the kit, so it has no pan, no tune and nothing to play
/// backwards — and a control that answers keys and changes nothing is worse
/// than a control that is not there.
///
/// `keytrk` is last so that keys mode can take the first two: a zone's
/// phrases always transpose with the keyboard, the same bargain that keeps
/// `keytrk` off a zone's own list.
const PHRASE_TAIL: [PadKnob; 3] =
    [PadKnob::PhraseGain, PadKnob::PhraseMute, PadKnob::PhraseKeytrack];

/// One head plus the layer tail, spelled once.
const fn with_layer(head: [PadKnob; PAD_CONTROLS]) -> [PadKnob; PAD_CONTROLS + 6] {
    let mut out = [PadKnob::Trig; PAD_CONTROLS + 6];
    let mut i = 0;
    while i < PAD_CONTROLS {
        out[i] = head[i];
        i += 1;
    }
    let mut j = 0;
    while j < LAYER_TAIL.len() {
        out[PAD_CONTROLS + j] = LAYER_TAIL[j];
        j += 1;
    }
    out
}

/// The same, with the phrase tail. Two builders rather than one because
/// the two tails are different lengths and an output size cannot be
/// computed from an input's on stable Rust.
const fn with_phrase(head: [PadKnob; PAD_CONTROLS]) -> [PadKnob; PAD_CONTROLS + 3] {
    let mut out = [PadKnob::Trig; PAD_CONTROLS + 3];
    let mut i = 0;
    while i < PAD_CONTROLS {
        out[i] = head[i];
        i += 1;
    }
    let mut j = 0;
    while j < PHRASE_TAIL.len() {
        out[PAD_CONTROLS + j] = PHRASE_TAIL[j];
        j += 1;
    }
    out
}

impl PadKnob {
    /// Every control in pads mode, on a layer row.
    pub const ALL: [PadKnob; PAD_CONTROLS + 6] = with_layer(PAD);

    /// Every control in keys mode, on a layer row.
    pub const KEYS: [PadKnob; PAD_CONTROLS + 6] = with_layer(ZONE);

    /// Pads mode on a phrase row.
    pub const PHRASES: [PadKnob; PAD_CONTROLS + 3] = with_phrase(PAD);

    /// Keys mode on a phrase row — the first two of the phrase tail, for
    /// the reason [`PHRASE_TAIL`] gives.
    pub const KEYS_PHRASES: [PadKnob; PAD_CONTROLS + 3] = with_phrase(ZONE);

    /// How many of any list belong to the pad or zone itself, rather than
    /// to the row under the sound cursor.
    pub const PAD_CONTROLS: usize = PAD_CONTROLS;

    /// The controls the thing under the cursor offers right now.
    ///
    /// `row` is what the sound list's cursor is standing on. On nothing at
    /// all the list stops at the pad's own: a gain knob for a sound that is
    /// not there is a control that answers keys and changes nothing — and
    /// the same rule is why a phrase row offers three controls rather than
    /// a layer's six.
    pub fn visible(row: Option<RowKind>, mode: MapMode) -> &'static [PadKnob] {
        match (row, mode) {
            (None, MapMode::Pads) => &Self::ALL[..PAD_CONTROLS],
            (None, MapMode::Keys) => &Self::KEYS[..PAD_CONTROLS],
            (Some(RowKind::Layer), MapMode::Pads) => &Self::ALL,
            (Some(RowKind::Layer), MapMode::Keys) => &Self::KEYS,
            (Some(RowKind::Phrase), MapMode::Pads) => &Self::PHRASES,
            (Some(RowKind::Phrase), MapMode::Keys) => {
                &Self::KEYS_PHRASES[..Self::KEYS_PHRASES.len() - 1]
            }
        }
    }

    /// Whether this control belongs to the row under the sound cursor
    /// rather than to the pad.
    pub fn is_layer(self) -> bool {
        matches!(
            self,
            Self::LayerGain
                | Self::LayerPan
                | Self::LayerTuneSt
                | Self::LayerTuneCents
                | Self::LayerReverse
                | Self::LayerMute
                | Self::PhraseGain
                | Self::PhraseMute
                | Self::PhraseKeytrack
        )
    }

    /// The name on the panel. The layer's controls repeat the pad's words —
    /// both have a pan — because they are drawn under their own heading,
    /// and `pan` is what the control is called.
    pub fn label(self) -> &'static str {
        match self {
            Self::Span => "span",
            Self::Trig => "trig",
            Self::Poly => "poly",
            Self::Choke => "choke",
            // Renoise's word, and the industry's. Not "round robin", which
            // does not fit a knob label, and not "rr", which nobody reads.
            Self::Cycle => "cycle",
            Self::PitchSt => "pitch",
            Self::PitchCents => "fine",
            Self::Attack => "attack",
            Self::Decay => "decay",
            Self::Sustain => "sustain",
            Self::Release => "release",
            Self::Level | Self::LayerGain => "level",
            Self::Pan | Self::LayerPan => "pan",
            Self::Root => "root",
            Self::Keytrack | Self::PhraseKeytrack => "keytrk",
            Self::LayerTuneSt => "tune",
            Self::LayerTuneCents => "fine",
            Self::LayerReverse => "rev",
            Self::LayerMute | Self::PhraseMute => "mute",
            // Not `level`: it scales what the phrase plays rather than how
            // loud the result is, and calling it a fader would be the one
            // word on this panel that is not true.
            Self::PhraseGain => "vel",
        }
    }

    /// What the control reads, in its own unit.
    ///
    /// A row control with no row behind it reads as a dash rather than as a
    /// lie, and so does the span with no zone behind it — the one control
    /// here that is about the keyboard rather than about the sound, which
    /// is why it is the one that needs `zone`.
    pub fn value(
        self,
        pad: &PadState,
        row: Option<PadRow<'_>>,
        zone: Option<&Zone>,
    ) -> String {
        let c = &pad.config;
        match self {
            // The span and nothing else: how many keys that is, the dial
            // beside it already says, and a value wider than the panel is
            // a knob cut in half by the right edge of the pane.
            Self::Span => zone.map_or_else(|| DASH.into(), Zone::span_label),
            Self::Trig => match c.trig {
                TrigMode::OneShot => "one-shot".into(),
                TrigMode::Gate => "gate".into(),
                TrigMode::Mono => "mono".into(),
            },
            Self::Poly => c.poly.to_string(),
            Self::Choke => {
                if c.choke == 0 {
                    "off".into()
                } else {
                    c.choke.to_string()
                }
            }
            Self::Cycle => on_off(c.cycle),
            Self::PitchSt => format!("{:+} st", c.pitch_st),
            Self::PitchCents => format!("{:+} ct", c.pitch_cents),
            Self::Attack => ms_text(c.attack_ms),
            Self::Decay => ms_text(c.decay_ms),
            Self::Sustain => format!("{}%", (c.sustain * 100.0).round() as i32),
            Self::Release => ms_text(c.release_ms),
            Self::Level => db_text(c.level),
            Self::Pan => pan_label(c.pan),
            Self::Root => note_name(c.root),
            Self::Keytrack => on_off(c.keytrack),
            // A control the row under the cursor does not have reads as a
            // dash rather than as a lie — and a phrase knob can never read
            // a layer, because it is handed one row and not two options.
            _ => match row {
                Some(PadRow::Layer(l)) => match self {
                    Self::LayerGain => db_text(l.gain),
                    Self::LayerPan => pan_label(l.pan),
                    Self::LayerTuneSt => format!("{:+} st", l.tune_st),
                    Self::LayerTuneCents => format!("{:+} ct", l.tune_cents),
                    Self::LayerReverse => on_off(l.reverse),
                    Self::LayerMute => on_off(l.mute),
                    _ => DASH.into(),
                },
                Some(PadRow::Phrase(p)) => match self {
                    // A percentage of what was played, because that is
                    // what it scales. Unity reads 100%, not 0 dB.
                    Self::PhraseGain => format!("{}%", (p.gain * 100.0).round() as i32),
                    Self::PhraseMute => on_off(p.mute),
                    Self::PhraseKeytrack => on_off(p.transpose_with_key),
                    _ => DASH.into(),
                },
                None => DASH.into(),
            },
        }
    }

    /// Where the dial points, 0..=1.
    ///
    /// The envelope times take the square root of their travel: a dial that
    /// is linear over ten seconds does not move at all across the first
    /// fifty milliseconds, which is where most of a drum envelope lives.
    pub fn frac(self, pad: &PadState, row: Option<PadRow<'_>>, zone: Option<&Zone>) -> f64 {
        let c = &pad.config;
        match self {
            // How much of the bed the zone holds. The dial cannot show two
            // edges, so it shows the thing a player is watching while they
            // move one: how wide the zone has got.
            Self::Span => zone.map_or(0.0, |z| z.keys() as f64 / NUM_PADS as f64),
            // Three positions along the dial, in the order the knob steps:
            // one-shot, gate, mono.
            Self::Trig => match c.trig {
                TrigMode::OneShot => 0.0,
                TrigMode::Gate => 0.5,
                TrigMode::Mono => 1.0,
            },
            Self::Poly => {
                let poly = c.poly.clamp(*POLY_RANGE.start(), *POLY_RANGE.end());
                f64::from(poly - POLY_RANGE.start()) / f64::from(POLY_RANGE.end() - POLY_RANGE.start())
            }
            Self::Choke => f64::from(c.choke.min(CHOKE_MAX)) / f64::from(CHOKE_MAX),
            Self::Cycle => f64::from(u8::from(c.cycle)),
            Self::PitchSt => centred(f64::from(c.pitch_st), TUNE_HALF),
            Self::PitchCents => centred(f64::from(c.pitch_cents), CENTS_HALF),
            Self::Attack => env_frac(c.attack_ms),
            Self::Decay => env_frac(c.decay_ms),
            Self::Sustain => f64::from(c.sustain).clamp(0.0, 1.0),
            Self::Release => env_frac(c.release_ms),
            Self::Level => gain_frac(c.level),
            Self::Pan => centred(f64::from(c.pan), 1.0),
            Self::Root => f64::from(c.root) / 127.0,
            Self::Keytrack => f64::from(u8::from(c.keytrack)),
            _ => match row {
                Some(PadRow::Layer(l)) => match self {
                    Self::LayerGain => gain_frac(l.gain),
                    Self::LayerPan => centred(f64::from(l.pan), 1.0),
                    Self::LayerTuneSt => centred(f64::from(l.tune_st), TUNE_HALF),
                    Self::LayerTuneCents => centred(f64::from(l.tune_cents), CENTS_HALF),
                    Self::LayerReverse => f64::from(u8::from(l.reverse)),
                    Self::LayerMute => f64::from(u8::from(l.mute)),
                    _ => 0.0,
                },
                Some(PadRow::Phrase(p)) => match self {
                    // Full travel is four times what was played, so unity
                    // sits a quarter of the way round — the same shape the
                    // level controls have, in the unit a velocity is in.
                    Self::PhraseGain => f64::from(p.gain / MAX_GAIN).clamp(0.0, 1.0),
                    Self::PhraseMute => f64::from(u8::from(p.mute)),
                    Self::PhraseKeytrack => f64::from(u8::from(p.transpose_with_key)),
                    _ => 0.0,
                },
                None => 0.0,
            },
        }
    }

    /// Turn the control by one press, or by a stride under `H`/`L`.
    ///
    /// `row` is the sound cursor, over the layers and the phrases as one
    /// list; a row control with nothing under it does nothing at all,
    /// rather than editing whichever sound happens to be first.
    pub fn adjust(self, pad: &mut PadState, row: usize, delta: i32, stride: bool) {
        let up = delta > 0;
        let c = &mut pad.config;
        match self {
            // The span is the one control that does not live on the sound
            // it is drawn beside: its edges are clamped against the zones
            // either side of it, which only the zone list knows about. So
            // [`crate::sampler::SamplerState::move_zone_edge`] moves it and
            // this does nothing — see the `Span` arm of the keys.
            Self::Span => {}
            // one-shot ↔ gate ↔ mono, and it stops at the ends rather than
            // wrapping — a switch a player is stepping through should not
            // loop back on them without warning.
            Self::Trig => {
                c.trig = match (c.trig, up) {
                    (TrigMode::OneShot, true) => TrigMode::Gate,
                    (TrigMode::Gate, true) => TrigMode::Mono,
                    (TrigMode::Mono, true) => TrigMode::Mono,
                    (TrigMode::Mono, false) => TrigMode::Gate,
                    (TrigMode::Gate, false) => TrigMode::OneShot,
                    (TrigMode::OneShot, false) => TrigMode::OneShot,
                };
            }
            Self::Poly => {
                c.poly = step_int(
                    i32::from(c.poly),
                    delta,
                    i32::from(*POLY_RANGE.start()),
                    i32::from(*POLY_RANGE.end()),
                ) as u8;
            }
            Self::Choke => {
                c.choke = step_int(i32::from(c.choke), delta, 0, i32::from(CHOKE_MAX)) as u8;
            }
            Self::Cycle => c.cycle = up,
            Self::PitchSt => {
                c.pitch_st = clamp_tune_st(i32::from(c.pitch_st) + delta * stride_by(stride, 12));
            }
            Self::PitchCents => {
                c.pitch_cents =
                    clamp_pitch_cents(i32::from(c.pitch_cents) + delta * stride_by(stride, 10));
            }
            Self::Attack => c.attack_ms = step_ms(c.attack_ms, delta, stride),
            Self::Decay => c.decay_ms = step_ms(c.decay_ms, delta, stride),
            Self::Sustain => {
                let step = if stride { 0.2 } else { 0.05 };
                c.sustain = (c.sustain + delta as f32 * step).clamp(0.0, 1.0);
            }
            Self::Release => c.release_ms = step_ms(c.release_ms, delta, stride),
            Self::Level => c.level = step_gain(c.level, delta, stride),
            Self::Pan => c.pan = step_pan(c.pan, delta, stride),
            Self::Root => c.root = step_int(i32::from(c.root), delta * stride_by(stride, 12), 0, 127) as u8,
            Self::Keytrack => c.keytrack = up,
            Self::PhraseGain | Self::PhraseMute | Self::PhraseKeytrack => {
                let Some(p) = pad.phrase_at_row(row) else { return };
                match self {
                    // Ten points of velocity a press, fifty on a stride:
                    // the smallest step a player can hear in a performance,
                    // and the travel the dial is drawn over.
                    Self::PhraseGain => {
                        let step = 0.1 * stride_by(stride, 5) as f32;
                        p.gain = (p.gain + delta as f32 * step).clamp(0.0, MAX_GAIN);
                    }
                    Self::PhraseKeytrack => p.transpose_with_key = up,
                    _ => p.mute = up,
                }
            }
            _ => {
                let Some(l) = pad.layers.get_mut(row) else { return };
                match self {
                    Self::LayerGain => l.gain = step_gain(l.gain, delta, stride),
                    Self::LayerPan => l.pan = step_pan(l.pan, delta, stride),
                    Self::LayerTuneSt => {
                        l.tune_st =
                            clamp_tune_st(i32::from(l.tune_st) + delta * stride_by(stride, 12));
                    }
                    Self::LayerTuneCents => {
                        l.tune_cents =
                            clamp_pitch_cents(i32::from(l.tune_cents) + delta * stride_by(stride, 10));
                    }
                    Self::LayerReverse => l.reverse = up,
                    _ => l.mute = up,
                }
            }
        }
    }
}

fn on_off(on: bool) -> String {
    if on { "on".into() } else { "off".into() }
}

/// A dial for a control that lives either side of a centre.
fn centred(value: f64, half: f64) -> f64 {
    ((value / half) + 1.0).clamp(0.0, 2.0) / 2.0
}

fn env_frac(ms: f32) -> f64 {
    (f64::from(ms) / f64::from(MAX_ENV_MS)).clamp(0.0, 1.0).sqrt()
}

fn gain_frac(gain: f32) -> f64 {
    if gain <= 0.0 {
        return 0.0;
    }
    let top = f64::from(20.0 * MAX_GAIN.log10());
    let db = f64::from(20.0 * gain.log10());
    ((db - f64::from(FLOOR_DB)) / (top - f64::from(FLOOR_DB))).clamp(0.0, 1.0)
}

fn stride_by(stride: bool, by: i32) -> i32 {
    if stride { by } else { 1 }
}

fn step_int(value: i32, delta: i32, lo: i32, hi: i32) -> i32 {
    (value + delta).clamp(lo, hi)
}

/// An envelope time steps by a slice of where it already is, so the same
/// press is 1 ms at the top of an attack and 50 ms out in a long decay.
///
/// Two details, both of them bugs when left out. The step is read from just
/// *under* the value when going down, so that a value sitting on a band
/// boundary steps back by the size it arrived with. And the value is put on
/// the step's own grid — downwards going up, upwards going down — so a time
/// loaded off the grid moves onto it with the first press and steps by
/// exactly one after, instead of living forever between two detents.
fn step_ms(value: f32, delta: i32, stride: bool) -> f32 {
    if delta == 0 {
        return value;
    }
    let at = if delta < 0 { value - 0.001 } else { value };
    let step = match at {
        v if v < 20.0 => 1.0,
        v if v < 100.0 => 5.0,
        v if v < 500.0 => 10.0,
        v if v < 2_000.0 => 50.0,
        _ => 100.0,
    } * if stride { 10.0 } else { 1.0 };
    let detents = if delta > 0 { (value / step).floor() } else { (value / step).ceil() };
    ((detents + delta as f32) * step).clamp(0.0, MAX_ENV_MS)
}

/// A level steps in whole decibels, rounding onto the grid first so that a
/// value loaded from a file self-corrects on the first press instead of
/// staying forever between two detents.
fn step_gain(gain: f32, delta: i32, stride: bool) -> f32 {
    let top_db = 20.0 * MAX_GAIN.log10();
    let silent_db = FLOOR_DB - 1.0;
    let current = if gain > 0.0 { (20.0 * gain.log10()).round() } else { silent_db };
    let target =
        (current + (delta * stride_by(stride, 6)) as f32).clamp(silent_db, top_db);
    if target < FLOOR_DB {
        0.0
    } else {
        10.0f32.powf(target / 20.0).clamp(0.0, MAX_GAIN)
    }
}

/// Pan snaps onto the centre as it passes: the centre is the position the
/// player has to be able to get back to exactly.
fn step_pan(pan: f32, delta: i32, stride: bool) -> f32 {
    let step = PAN_STEP * if stride { 5.0 } else { 1.0 };
    let target = pan + delta as f32 * step;
    if target.abs() < PAN_STEP * 0.5 {
        0.0
    } else {
        target.clamp(*PAN_RANGE.start(), *PAN_RANGE.end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::{SamplerState, MAX_LAYERS};
    use crate::sampler::zones::Zone;
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn pad_with_layer() -> PadState {
        let mut state = SamplerState::new();
        let pcm = Arc::new(SamplePcm { data: vec![0.0; 100], channels: 1, sample_rate: 44_100.0 });
        state.add_wav_layer(0, PathBuf::from("a.wav"), pcm).unwrap();
        state.pads.remove(0)
    }

    /// One layer and one phrase, so the same pad answers both kinds of row.
    fn pad_with_both() -> PadState {
        let mut pad = pad_with_layer();
        let events = Arc::from(vec![phosphor_plugin::sample::PhraseEvent {
            frame: 0,
            status: 0x90,
            data1: 60,
            data2: 100,
        }]);
        pad.add_phrase(events, 1_000, 0.0, "pad").unwrap();
        pad
    }

    #[test]
    fn an_empty_pad_offers_no_row_controls() {
        for mode in [MapMode::Pads, MapMode::Keys] {
            assert_eq!(PadKnob::visible(None, mode).len(), PadKnob::PAD_CONTROLS);
            assert_eq!(PadKnob::visible(Some(RowKind::Layer), mode).len(), PadKnob::ALL.len());
            assert!(
                PadKnob::visible(None, mode).iter().all(|k| !k.is_layer()),
                "a row control is reachable with no sounds, in {mode:?}",
            );
        }
    }

    /// Every list carries the same number of controls above the row's,
    /// which is what lets one constant split any of them. The defect this
    /// catches is a control added to one list and not the other: the panel
    /// would draw it under the row heading, and `j`/`k` would walk into a
    /// knob with the wrong name on it.
    #[test]
    fn every_control_list_splits_in_the_same_place() {
        for list in [
            &PadKnob::ALL[..],
            &PadKnob::KEYS[..],
            &PadKnob::PHRASES[..],
            &PadKnob::KEYS_PHRASES[..],
        ] {
            assert_eq!(
                list.iter().position(|k| k.is_layer()),
                Some(PadKnob::PAD_CONTROLS),
                "{list:?} does not split where the constant says",
            );
        }
        assert!(
            !PadKnob::KEYS.contains(&PadKnob::Keytrack),
            "keys mode offers a switch a zone cannot turn off",
        );
        assert!(!PadKnob::ALL.contains(&PadKnob::Span), "pads mode offers a span");
    }

    /// The cycle switch sits beside choke, reads in words, and is reachable
    /// on a zone as well as on a pad — a zone stacks layers exactly as a pad
    /// does, so it has exactly as much use for a rotation.
    #[test]
    fn the_cycle_switch_is_on_both_lists_beside_choke() {
        for list in [&PadKnob::ALL[..], &PadKnob::KEYS[..]] {
            let at = list.iter().position(|&k| k == PadKnob::Cycle);
            let choke = list.iter().position(|&k| k == PadKnob::Choke);
            assert_eq!(at, choke.map(|i| i + 1), "cycle is not beside choke in {list:?}");
        }
        let mut pad = pad_with_layer();
        assert!(!pad.config.cycle, "a fresh pad cycles");
        assert_eq!(PadKnob::Cycle.value(&pad, None, None), "off");
        assert_eq!(PadKnob::Cycle.frac(&pad, None, None), 0.0);

        PadKnob::Cycle.adjust(&mut pad, 0, 1, false);
        assert!(pad.config.cycle);
        assert_eq!(PadKnob::Cycle.value(&pad, None, None), "on");
        assert_eq!(PadKnob::Cycle.frac(&pad, None, None), 1.0);
        // A switch is a switch however hard it is pressed: no stride, no
        // travel past either end.
        for _ in 0..20 {
            PadKnob::Cycle.adjust(&mut pad, 0, 1, true);
        }
        assert!(pad.config.cycle);
        PadKnob::Cycle.adjust(&mut pad, 0, -1, true);
        assert!(!pad.config.cycle);
    }

    /// A phrase has no pan, no tune and nothing to play backwards — and in
    /// keys mode it has no keytrack switch either, because a zone's phrases
    /// always transpose with the keyboard.
    #[test]
    fn a_phrase_row_offers_only_the_controls_a_phrase_has() {
        let pads = PadKnob::visible(Some(RowKind::Phrase), MapMode::Pads);
        assert_eq!(&pads[PadKnob::PAD_CONTROLS..], &PHRASE_TAIL);
        for gone in [PadKnob::LayerPan, PadKnob::LayerTuneSt, PadKnob::LayerReverse] {
            assert!(!pads.contains(&gone), "{gone:?} reached a phrase row");
        }
        let keys = PadKnob::visible(Some(RowKind::Phrase), MapMode::Keys);
        assert_eq!(keys.len(), pads.len() - 1);
        assert!(
            !keys.contains(&PadKnob::PhraseKeytrack),
            "keys mode offers a switch the materializer overrides",
        );
        assert!(keys.contains(&PadKnob::PhraseGain) && keys.contains(&PadKnob::PhraseMute));
    }

    /// The phrase controls turn the phrase under the row cursor, and the
    /// layer controls cannot reach it. The defect this catches is a row
    /// cursor past the layers quietly editing layer zero.
    #[test]
    fn the_phrase_controls_turn_the_phrase_the_cursor_is_on() {
        let mut pad = pad_with_both();
        let row = pad.layers.len(); // the first phrase
        let layer_before = pad.layers[0].clone();

        PadKnob::PhraseGain.adjust(&mut pad, row, -1, false);
        assert!((pad.phrases[0].gain - 0.9).abs() < 1e-5, "{}", pad.phrases[0].gain);
        assert_eq!(PadKnob::PhraseGain.value(&pad, pad.row(row), None), "90%");
        PadKnob::PhraseKeytrack.adjust(&mut pad, row, 1, false);
        assert!(pad.phrases[0].transpose_with_key);
        PadKnob::PhraseMute.adjust(&mut pad, row, 1, false);
        assert!(pad.phrases[0].mute);
        assert_eq!(pad.layers[0], layer_before, "a phrase control turned a layer");

        // ...and the layer controls, pointed at the phrase's row, turn
        // nothing at all rather than the layer that shares the pad.
        for knob in PHRASE_TAIL {
            knob.adjust(&mut pad, 99, 1, false);
        }
        PadKnob::LayerGain.adjust(&mut pad, row, 1, false);
        assert_eq!(pad.layers[0], layer_before, "a layer knob reached past its own row");
    }

    /// A phrase's velocity scale stops at both ends of its travel, and the
    /// dial never leaves its own.
    #[test]
    fn the_velocity_scale_stops_at_both_ends() {
        let mut pad = pad_with_both();
        let row = pad.layers.len();
        for direction in [-1, 1] {
            for _ in 0..400 {
                PadKnob::PhraseGain.adjust(&mut pad, row, direction, true);
                let f = PadKnob::PhraseGain.frac(&pad, pad.row(row), None);
                assert!((0.0..=1.0).contains(&f), "the dial left its travel at {f}");
            }
            assert!((0.0..=MAX_GAIN).contains(&pad.phrases[0].gain));
        }
        assert_eq!(pad.phrases[0].gain, MAX_GAIN);
    }

    /// The span reads the zone and turns nothing on the sound it is drawn
    /// beside — its edges are the zone list's business, not a pad's.
    #[test]
    fn the_span_reads_the_zone_and_edits_nothing_here() {
        let mut pad = pad_with_layer();
        let zone = Zone::new(0, 11, PadState::empty(60));
        assert_eq!(PadKnob::Span.value(&pad, None, Some(&zone)), zone.span_label());
        assert_eq!(PadKnob::Span.value(&pad, None, None), DASH);
        assert!(PadKnob::Span.frac(&pad, None, Some(&zone)) > 0.0);

        let before = pad.clone();
        for delta in [-1, 1] {
            PadKnob::Span.adjust(&mut pad, 0, delta, true);
        }
        assert_eq!(pad, before, "the span turned a control on the sound");
    }

    /// The defect this catches: a layer control turned on a pad whose row
    /// cursor points past the end, quietly editing layer zero instead.
    #[test]
    fn a_row_control_with_no_row_under_it_changes_nothing() {
        let mut pad = pad_with_layer();
        let before = pad.clone();
        let reachable = PadKnob::visible(Some(RowKind::Layer), MapMode::Pads)
            .iter()
            .chain(PadKnob::visible(Some(RowKind::Phrase), MapMode::Pads).iter());
        for knob in reachable.filter(|k| k.is_layer()) {
            knob.adjust(&mut pad, MAX_LAYERS + 3, 1, false);
            assert_eq!(knob.value(&pad, None, None), DASH, "{knob:?} invented a value");
        }
        assert_eq!(pad, before, "an out-of-range row cursor edited a real sound");
    }

    /// Every control stops at both ends of its travel, however long a key
    /// is held down.
    #[test]
    fn every_control_stops_at_the_ends_of_its_travel() {
        let mut pad = pad_with_layer();
        for knob in PadKnob::ALL {
            for direction in [-1, 1] {
                for _ in 0..400 {
                    knob.adjust(&mut pad, 0, direction, true);
                }
                let c = &pad.config;
                assert!((1..=8).contains(&c.poly), "poly {}", c.poly);
                assert!(c.choke <= 8);
                assert!((-48..=48).contains(&c.pitch_st));
                assert!((-50..=50).contains(&c.pitch_cents));
                assert!((0.0..=MAX_ENV_MS).contains(&c.attack_ms));
                assert!((0.0..=MAX_ENV_MS).contains(&c.decay_ms));
                assert!((0.0..=MAX_ENV_MS).contains(&c.release_ms));
                assert!((0.0..=1.0).contains(&c.sustain));
                assert!((0.0..=MAX_GAIN).contains(&c.level));
                assert!((-1.0..=1.0).contains(&c.pan));
                assert!(c.root <= 127);
                let l = &pad.layers[0];
                assert!((0.0..=MAX_GAIN).contains(&l.gain));
                assert!((-1.0..=1.0).contains(&l.pan));
                assert!((-48..=48).contains(&l.tune_st));
                assert!((-50..=50).contains(&l.tune_cents));
                // And the dial never leaves its own travel either.
                let f = knob.frac(&pad, pad.row(0), None);
                assert!((0.0..=1.0).contains(&f), "{knob:?} dial at {f}");
            }
        }
    }

    /// A press up and the same press back down lands where it started —
    /// the trap in a step size read from the value being stepped, and worst
    /// exactly on the boundaries between two step sizes.
    #[test]
    fn a_time_returns_to_where_it_started() {
        for start in [0.0, 1.0, 19.0, 20.0, 100.0, 400.0, 500.0, 2_000.0, 5_000.0] {
            let up = step_ms(start, 1, false);
            assert!(up > start, "a press up from {start} did not move it");
            let back = step_ms(up, -1, false);
            assert_eq!(back, start, "a round trip from {start} landed on {back}");
        }
        // A value off the grid — a hand-edited session — moves onto it in
        // the direction pressed, and is on the grid from then on.
        assert_eq!(step_ms(99.0, 1, false), 100.0);
        assert_eq!(step_ms(99.0, -1, false), 95.0);
    }

    /// The fader's self-correction: a level loaded off the decibel grid
    /// lands on it with the first press and steps by exactly one after.
    #[test]
    fn a_level_off_the_grid_snaps_onto_it() {
        let stepped = step_gain(0.75, -1, false); // -2.5 dB
        let db = 20.0 * stepped.log10();
        assert!((db + 3.0).abs() < 0.01, "landed at {db} dB");
        // And the bottom of the travel is silence, not a whisper.
        let mut gain = 0.1f32;
        for _ in 0..80 {
            gain = step_gain(gain, -1, false);
        }
        assert_eq!(gain, 0.0);
        assert_eq!(db_text(gain), "-oo");
        // Which one press back up leaves.
        assert!(step_gain(gain, 1, false) > 0.0);
    }

    /// Pan finds its centre coming from either side, and never sticks there.
    #[test]
    fn pan_snaps_to_the_centre_and_leaves_it_again() {
        assert_eq!(step_pan(0.05, -1, false), 0.0);
        assert_eq!(step_pan(-0.05, 1, false), 0.0);
        assert!(step_pan(0.0, 1, false) > 0.0);
        assert!(step_pan(0.0, -1, false) < 0.0);
    }

    /// Every control says something, on a pad with a sound under the row
    /// cursor and on one without — a blank readout is a control the player
    /// cannot verify.
    #[test]
    fn no_control_reads_as_nothing() {
        let pad = pad_with_both();
        let empty = PadState::empty(60);
        let every = PadKnob::ALL
            .iter()
            .chain(PadKnob::KEYS.iter())
            .chain(PadKnob::PHRASES.iter())
            .chain(PadKnob::KEYS_PHRASES.iter());
        for knob in every.copied() {
            assert!(!knob.label().is_empty());
            for row in [pad.row(0), pad.row(1)] {
                assert!(!knob.value(&pad, row, None).is_empty(), "{knob:?} on {row:?}");
            }
            assert!(
                !knob.value(&empty, None, None).is_empty(),
                "{knob:?} on an empty pad",
            );
        }
    }
}
