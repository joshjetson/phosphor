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

use phosphor_plugin::sample::TrigMode;

use crate::format::{db_text, ms_text, note_name, pan_label};

use super::{LayerState, MapMode, PadState, Zone, NUM_PADS};

/// The top of an envelope stage. Ten seconds is longer than any sampler
/// envelope a player reaches for and short enough that the dial's travel
/// still means something.
const MAX_ENV_MS: f32 = 10_000.0;

/// The top of a level control, linear — the session clamps to this too, so
/// a hand-edited file cannot open with a pad the knob cannot reach.
///
/// Public because it is also the ceiling on a normalize: that sets the
/// same number this knob turns, and a gain past the end of the travel is a
/// gain the player cannot turn back down by hand.
pub const MAX_GAIN: f32 = 4.0;

/// The bottom of a level control's travel, in decibels. One step below it
/// is silence, so stepping down from the floor reaches it and stepping up
/// leaves it — the fader's rule, and for the same reason: twenty more
/// presses to reach an already inaudible level is travel nobody wants.
const FLOOR_DB: f32 = -40.0;

/// One press of a pan control: twenty detents from the centre to either
/// end. The strip's feel, chosen again here rather than borrowed, because a
/// pad's pan and a track's pan are different controls that happen to agree.
const PAN_STEP: f32 = 0.05;

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
}

impl PadKnob {
    /// Every control in pads mode, pad first.
    pub const ALL: [PadKnob; 19] = [
        Self::Trig,
        Self::Poly,
        Self::Choke,
        Self::PitchSt,
        Self::PitchCents,
        Self::Attack,
        Self::Decay,
        Self::Sustain,
        Self::Release,
        Self::Level,
        Self::Pan,
        Self::Root,
        Self::Keytrack,
        Self::LayerGain,
        Self::LayerPan,
        Self::LayerTuneSt,
        Self::LayerTuneCents,
        Self::LayerReverse,
        Self::LayerMute,
    ];

    /// Every control in keys mode.
    ///
    /// The span takes the top, and `keytrk` is gone: a zone always tracks
    /// the keyboard — one that did not would be a stretch of keys all
    /// playing one pitch, which is a pad with extra steps — so the switch
    /// would be a control with nothing on the other side of it. That
    /// leaves the same number of controls above the layer's, which
    /// [`PadKnob::PAD_CONTROLS`] counts for both lists.
    pub const KEYS: [PadKnob; 19] = [
        Self::Span,
        Self::Trig,
        Self::Poly,
        Self::Choke,
        Self::PitchSt,
        Self::PitchCents,
        Self::Attack,
        Self::Decay,
        Self::Sustain,
        Self::Release,
        Self::Level,
        Self::Pan,
        Self::Root,
        Self::LayerGain,
        Self::LayerPan,
        Self::LayerTuneSt,
        Self::LayerTuneCents,
        Self::LayerReverse,
        Self::LayerMute,
    ];

    /// How many of either list belong to the pad or zone itself, rather
    /// than to the layer under the layer cursor.
    pub const PAD_CONTROLS: usize = 13;

    /// The controls the thing under the cursor offers right now. An empty
    /// pad stops at its own: a gain knob for a sound that is not there is a
    /// control that answers keys and changes nothing.
    pub fn visible(has_layer: bool, mode: MapMode) -> &'static [PadKnob] {
        let all: &'static [PadKnob] = match mode {
            MapMode::Pads => &Self::ALL,
            MapMode::Keys => &Self::KEYS,
        };
        if has_layer {
            all
        } else {
            &all[..Self::PAD_CONTROLS]
        }
    }

    /// Whether this control belongs to the selected layer rather than to
    /// the pad.
    pub fn is_layer(self) -> bool {
        matches!(
            self,
            Self::LayerGain
                | Self::LayerPan
                | Self::LayerTuneSt
                | Self::LayerTuneCents
                | Self::LayerReverse
                | Self::LayerMute
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
            Self::PitchSt => "pitch",
            Self::PitchCents => "fine",
            Self::Attack => "attack",
            Self::Decay => "decay",
            Self::Sustain => "sustain",
            Self::Release => "release",
            Self::Level | Self::LayerGain => "level",
            Self::Pan | Self::LayerPan => "pan",
            Self::Root => "root",
            Self::Keytrack => "keytrk",
            Self::LayerTuneSt => "tune",
            Self::LayerTuneCents => "fine",
            Self::LayerReverse => "rev",
            Self::LayerMute => "mute",
        }
    }

    /// What the control reads, in its own unit.
    ///
    /// A layer control with no layer behind it reads as a dash rather than
    /// as a lie, and so does the span with no zone behind it — the one
    /// control here that is about the keyboard rather than about the sound,
    /// which is why it is the one that needs `zone`.
    pub fn value(
        self,
        pad: &PadState,
        layer: Option<&LayerState>,
        zone: Option<&Zone>,
    ) -> String {
        let c = &pad.config;
        match self {
            // The span and nothing else: how many keys that is, the dial
            // beside it already says, and a value wider than the panel is
            // a knob cut in half by the right edge of the pane.
            Self::Span => zone.map_or_else(|| "\u{2014}".into(), Zone::span_label),
            Self::Trig => match c.trig {
                TrigMode::OneShot => "one-shot".into(),
                TrigMode::Gate => "gate".into(),
            },
            Self::Poly => c.poly.to_string(),
            Self::Choke => {
                if c.choke == 0 {
                    "off".into()
                } else {
                    c.choke.to_string()
                }
            }
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
            _ => match layer {
                None => "\u{2014}".into(),
                Some(l) => match self {
                    Self::LayerGain => db_text(l.gain),
                    Self::LayerPan => pan_label(l.pan),
                    Self::LayerTuneSt => format!("{:+} st", l.tune_st),
                    Self::LayerTuneCents => format!("{:+} ct", l.tune_cents),
                    Self::LayerReverse => on_off(l.reverse),
                    _ => on_off(l.mute),
                },
            },
        }
    }

    /// Where the dial points, 0..=1.
    ///
    /// The envelope times take the square root of their travel: a dial that
    /// is linear over ten seconds does not move at all across the first
    /// fifty milliseconds, which is where most of a drum envelope lives.
    pub fn frac(self, pad: &PadState, layer: Option<&LayerState>, zone: Option<&Zone>) -> f64 {
        let c = &pad.config;
        match self {
            // How much of the bed the zone holds. The dial cannot show two
            // edges, so it shows the thing a player is watching while they
            // move one: how wide the zone has got.
            Self::Span => zone.map_or(0.0, |z| z.keys() as f64 / NUM_PADS as f64),
            Self::Trig => f64::from(u8::from(c.trig == TrigMode::Gate)),
            Self::Poly => f64::from(c.poly.clamp(1, 8) - 1) / 7.0,
            Self::Choke => f64::from(c.choke.min(8)) / 8.0,
            Self::PitchSt => centred(f64::from(c.pitch_st), 48.0),
            Self::PitchCents => centred(f64::from(c.pitch_cents), 50.0),
            Self::Attack => env_frac(c.attack_ms),
            Self::Decay => env_frac(c.decay_ms),
            Self::Sustain => f64::from(c.sustain).clamp(0.0, 1.0),
            Self::Release => env_frac(c.release_ms),
            Self::Level => gain_frac(c.level),
            Self::Pan => centred(f64::from(c.pan), 1.0),
            Self::Root => f64::from(c.root) / 127.0,
            Self::Keytrack => f64::from(u8::from(c.keytrack)),
            _ => match layer {
                None => 0.0,
                Some(l) => match self {
                    Self::LayerGain => gain_frac(l.gain),
                    Self::LayerPan => centred(f64::from(l.pan), 1.0),
                    Self::LayerTuneSt => centred(f64::from(l.tune_st), 48.0),
                    Self::LayerTuneCents => centred(f64::from(l.tune_cents), 50.0),
                    Self::LayerReverse => f64::from(u8::from(l.reverse)),
                    _ => f64::from(u8::from(l.mute)),
                },
            },
        }
    }

    /// Turn the control by one press, or by a stride under `H`/`L`.
    ///
    /// `layer` is the layer cursor; a layer control with nothing under it
    /// does nothing at all, rather than editing whichever layer happens to
    /// be first.
    pub fn adjust(self, pad: &mut PadState, layer: usize, delta: i32, stride: bool) {
        let up = delta > 0;
        let c = &mut pad.config;
        match self {
            // The span is the one control that does not live on the sound
            // it is drawn beside: its edges are clamped against the zones
            // either side of it, which only the zone list knows about. So
            // [`crate::sampler::SamplerState::move_zone_edge`] moves it and
            // this does nothing — see the `Span` arm of the keys.
            Self::Span => {}
            Self::Trig => c.trig = if up { TrigMode::Gate } else { TrigMode::OneShot },
            Self::Poly => c.poly = step_int(i32::from(c.poly), delta, 1, 8) as u8,
            Self::Choke => c.choke = step_int(i32::from(c.choke), delta, 0, 8) as u8,
            Self::PitchSt => {
                c.pitch_st = step_int(i32::from(c.pitch_st), delta * stride_by(stride, 12), -48, 48)
                    as i8;
            }
            Self::PitchCents => {
                c.pitch_cents =
                    step_int(i32::from(c.pitch_cents), delta * stride_by(stride, 10), -50, 50) as i8;
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
            _ => {
                let Some(l) = pad.layers.get_mut(layer) else { return };
                match self {
                    Self::LayerGain => l.gain = step_gain(l.gain, delta, stride),
                    Self::LayerPan => l.pan = step_pan(l.pan, delta, stride),
                    Self::LayerTuneSt => {
                        l.tune_st =
                            step_int(i32::from(l.tune_st), delta * stride_by(stride, 12), -48, 48)
                                as i8;
                    }
                    Self::LayerTuneCents => {
                        l.tune_cents = step_int(
                            i32::from(l.tune_cents),
                            delta * stride_by(stride, 10),
                            -50,
                            50,
                        ) as i8;
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
        target.clamp(-1.0, 1.0)
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

    #[test]
    fn an_empty_pad_offers_no_layer_controls() {
        for mode in [MapMode::Pads, MapMode::Keys] {
            assert_eq!(PadKnob::visible(false, mode).len(), PadKnob::PAD_CONTROLS);
            assert_eq!(PadKnob::visible(true, mode).len(), PadKnob::ALL.len());
            assert!(
                PadKnob::visible(false, mode).iter().all(|k| !k.is_layer()),
                "a layer control is reachable with no layers, in {mode:?}",
            );
        }
    }

    /// The two lists carry the same number of controls above the layer's,
    /// which is what lets one constant split either of them. The defect
    /// this catches is a control added to one list and not the other: the
    /// panel would draw it under the layer heading, and `j`/`k` would
    /// walk into a knob with the wrong name on it.
    #[test]
    fn both_control_lists_split_in_the_same_place() {
        for list in [&PadKnob::ALL, &PadKnob::KEYS] {
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

    /// The span reads the zone and turns nothing on the sound it is drawn
    /// beside — its edges are the zone list's business, not a pad's.
    #[test]
    fn the_span_reads_the_zone_and_edits_nothing_here() {
        let mut pad = pad_with_layer();
        let zone = Zone::new(0, 11, PadState::empty(60));
        assert_eq!(PadKnob::Span.value(&pad, None, Some(&zone)), zone.span_label());
        assert_eq!(PadKnob::Span.value(&pad, None, None), "\u{2014}");
        assert!(PadKnob::Span.frac(&pad, None, Some(&zone)) > 0.0);

        let before = pad.clone();
        for delta in [-1, 1] {
            PadKnob::Span.adjust(&mut pad, 0, delta, true);
        }
        assert_eq!(pad, before, "the span turned a control on the sound");
    }

    /// The defect this catches: a layer control turned on a pad whose layer
    /// cursor points past the end, quietly editing layer zero instead.
    #[test]
    fn a_layer_control_with_no_layer_under_it_changes_nothing() {
        let mut pad = pad_with_layer();
        let before = pad.clone();
        for knob in PadKnob::visible(true, MapMode::Pads).iter().filter(|k| k.is_layer()) {
            knob.adjust(&mut pad, MAX_LAYERS + 3, 1, false);
            assert_eq!(knob.value(&pad, None, None), "\u{2014}", "{knob:?} invented a value");
        }
        assert_eq!(pad, before, "an out-of-range layer cursor edited a real layer");
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
                let f = knob.frac(&pad, pad.layers.first(), None);
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

    /// Every control says something, on a pad with a layer and on one
    /// without — a blank readout is a control the player cannot verify.
    #[test]
    fn no_control_reads_as_nothing() {
        let pad = pad_with_layer();
        let empty = PadState { config: pad.config, layers: Vec::new(), source: None };
        for knob in PadKnob::ALL.iter().chain(PadKnob::KEYS.iter()).copied() {
            assert!(!knob.label().is_empty());
            assert!(!knob.value(&pad, pad.layers.first(), None).is_empty(), "{knob:?}");
            assert!(
                !knob.value(&empty, None, None).is_empty(),
                "{knob:?} on an empty pad",
            );
        }
    }
}
