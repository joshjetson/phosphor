//! Putting a chop on the keys: one slice per pad, upward from a key.
//!
//! All or nothing. Every slice lands or none does, and a landing that
//! cannot happen says why in a sentence and touches nothing — the owner's
//! rule, and the one that keeps a chop from quietly overwriting a kit. The
//! three ways it cannot: there are no cuts, the run would go past the top
//! of the keyboard, or a key it needs already holds a sound. "Holds a
//! sound" means audio or a phrase on it; a pad whose knobs were turned with
//! nothing on it has nothing to lose, and is landed on.
//!
//! Each slice is the source layer again with a different window: the same
//! buffer, the same file or take, the same gain and tuning. So a chop costs
//! the recording once however many keys it covers, saves as one file, and
//! reopens as one buffer.

use phosphor_plugin::sample::{PadConfig, TrigMode, CHOKE_MAX, NUM_PADS};

use crate::sampler::{LayerState, MapMode, PadState, SamplerState};

/// How the slices behave once they are on the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Feel {
    /// A break: every slice a one-shot, and all of them in one new choke
    /// group so each hit cuts the one before it — the break plays back as
    /// one drummer rather than a pile-up. A choke group and not the mono
    /// trigger, which would cut every other pad on the sampler too.
    #[default]
    Break,
    /// A phrase to play: slices sound while the key is held, several at
    /// once, and die away when it lets go.
    Melodic,
}

/// Release on a melodic slice: long enough not to click off, short enough
/// that a held chord does not smear into the next.
const MELODIC_RELEASE_MS: f32 = 150.0;

/// Voices per melodic slice: a key played again rings over itself.
const MELODIC_POLY: u8 = 4;

/// What a landing did, for the words the player gets back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    /// The first pad and how many.
    pub first: usize,
    pub count: usize,
    /// The choke group a break took; `None` for a melodic chop, or when all
    /// eight groups were already in use and the slices landed unchoked.
    pub choke: Option<u8>,
    /// The bed was in keys mode, and is in pads mode now: chops live on
    /// pads, and a chop the player cannot hear is no chop.
    pub switched_to_pads: bool,
}

/// Whether a pad has something on it a landing would destroy.
pub fn holds_sound(pad: &PadState) -> bool {
    !pad.layers.is_empty() || !pad.phrases.is_empty()
}

/// The lowest choke group no pad or zone on this sampler uses.
fn free_choke(state: &SamplerState) -> Option<u8> {
    let mut used = [false; CHOKE_MAX as usize + 1];
    for config in state.pads.iter().chain(state.zones.iter().map(|z| &z.pad)).map(|p| p.config) {
        used[usize::from(config.choke.min(CHOKE_MAX))] = true;
    }
    (1..=CHOKE_MAX).find(|&g| !used[usize::from(g)])
}

/// The config a slice lands with, on the pad for `note`.
fn config_for(note: u8, feel: Feel, choke: Option<u8>) -> PadConfig {
    let base = PadConfig::for_key(note);
    match feel {
        Feel::Break => PadConfig { trig: TrigMode::OneShot, choke: choke.unwrap_or(0), ..base },
        Feel::Melodic => PadConfig {
            trig: TrigMode::Gate,
            poly: MELODIC_POLY,
            release_ms: MELODIC_RELEASE_MS,
            ..base
        },
    }
}

/// The keys `count` slices from `first` cover, as a player reads them:
/// `C2` for one, `C2–E2` for a run. The one spelling, for the header, the
/// refusal and the landing's own words.
pub fn span_label(first: usize, count: usize) -> String {
    let label = |pad: usize| SamplerState::pad_label(pad.min(NUM_PADS - 1));
    match count {
        0 | 1 => label(first),
        n => format!("{}\u{2013}{}", label(first), label(first + n - 1)),
    }
}

/// Whether every key `count` slices from `first` would cover is on the bed
/// and free.
pub fn fits(state: &SamplerState, first: usize, count: usize) -> bool {
    first + count <= NUM_PADS && !state.pads[first..first + count].iter().any(holds_sound)
}

/// Why `slices` cannot land from `first`, in the player's words — or
/// `None` when they can.
pub fn refusal(state: &SamplerState, slices: usize, first: usize) -> Option<String> {
    if slices == 0 {
        return Some("no cuts to land \u{00b7} add one, or turn the sensitivity up".into());
    }
    let from = SamplerState::pad_label(first.min(NUM_PADS - 1));
    if first + slices > NUM_PADS {
        return Some(format!(
            "{slices} slices from {from} run past the top key \u{00b7} start lower, or fit them to fewer keys",
        ));
    }
    let taken = state.pads[first..first + slices].iter().filter(|p| holds_sound(p)).count();
    (taken > 0).then(|| match slices {
        1 => format!("{from} already holds a sound \u{00b7} clear it, or land somewhere else"),
        _ => format!(
            "{taken} of the {slices} keys {} already {} \u{00b7} clear them, or land somewhere else",
            span_label(first, slices),
            if taken == 1 { "holds a sound" } else { "hold sounds" },
        ),
    })
}

/// Lay `slices` of `source` on the pads from `first` upward, one each.
///
/// `slices` are buffer frames, as the markers give them. `Err` is the
/// [`refusal`], and nothing has changed.
pub fn land(
    state: &mut SamplerState,
    source: &LayerState,
    slices: &[(u64, u64)],
    first: usize,
    feel: Feel,
) -> Result<Landing, String> {
    if let Some(reason) = refusal(state, slices.len(), first) {
        return Err(reason);
    }
    let choke = match feel {
        Feel::Break => free_choke(state),
        Feel::Melodic => None,
    };
    for (offset, &(start, end)) in slices.iter().enumerate() {
        let pad = first + offset;
        let note = SamplerState::note_of_pad(pad);
        let slot = &mut state.pads[pad];
        slot.config = config_for(note, feel, choke);
        slot.layers = vec![LayerState {
            start_frame: start,
            end_frame: end,
            // The window is the slice; a reversed or muted source is the
            // player's choice about that sound, not about its pieces.
            reverse: false,
            mute: false,
            ..source.clone()
        }];
    }
    let switched_to_pads = state.mode == MapMode::Keys;
    state.set_mode(MapMode::Pads);
    Ok(Landing { first, count: slices.len(), choke, switched_to_pads })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::zones::Zone;
    use crate::sampler::LayerSource;
    use phosphor_plugin::sample::SamplePcm;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn source() -> LayerState {
        let pcm = Arc::new(SamplePcm { data: vec![0.25; 10_000], channels: 1, sample_rate: 48_000.0 });
        let mut layer = LayerState::from_wav(PathBuf::from("amen.wav"), pcm);
        layer.gain = 0.5;
        layer.reverse = true;
        layer
    }

    const SLICES: [(u64, u64); 3] = [(100, 900), (900, 4_000), (4_000, 10_000)];

    /// MIDI 36 — C2 here — where Ableton's and Logic's slicers land slice
    /// one (their C1), and so where a player expects it.
    fn c2() -> usize {
        SamplerState::pad_of_note(36).unwrap()
    }

    #[test]
    fn each_slice_lands_on_its_own_key_upward_sharing_one_buffer() {
        let mut state = SamplerState::new();
        let src = source();
        let landing = land(&mut state, &src, &SLICES, c2(), Feel::Break).unwrap();
        assert_eq!((landing.first, landing.count), (c2(), 3));
        for (offset, &(start, end)) in SLICES.iter().enumerate() {
            let pad = &state.pads[c2() + offset];
            assert_eq!(pad.layers.len(), 1);
            let layer = &pad.layers[0];
            assert_eq!((layer.start_frame, layer.end_frame), (start, end));
            assert!(Arc::ptr_eq(layer.pcm.as_ref().unwrap(), src.pcm.as_ref().unwrap()));
            assert_eq!(layer.path, src.path, "a slice forgot the file it came from");
            assert_eq!(layer.gain, 0.5, "a slice lost the source's gain");
            assert!(!layer.reverse, "a slice inherited the source's reverse");
            assert_eq!(pad.config.root, SamplerState::note_of_pad(c2() + offset));
        }
        assert!(!holds_sound(&state.pads[c2() + 3]), "a fourth key was touched");
    }

    /// The owner's rule: a kit is never overwritten by a chop.
    #[test]
    fn a_key_with_a_sound_on_it_refuses_the_whole_landing() {
        let mut state = SamplerState::new();
        state.add_wav_layer(c2() + 1, PathBuf::from("snare.wav"), source().pcm.unwrap()).unwrap();
        let before = state.clone();
        let err = land(&mut state, &source(), &SLICES, c2(), Feel::Break).unwrap_err();
        assert!(err.contains("1 of the 3 keys C2\u{2013}D2 already holds a sound"), "{err}");
        assert_eq!(state, before, "a refused landing changed the kit");
    }

    /// A pad whose knobs were turned but which has nothing on it has
    /// nothing to lose.
    #[test]
    fn a_pad_with_only_settings_is_landed_on() {
        let mut state = SamplerState::new();
        state.pads[c2()].config.level = 0.2;
        assert!(land(&mut state, &source(), &SLICES, c2(), Feel::Break).is_ok());
        assert_eq!(state.pads[c2()].config.level, 1.0);
    }

    #[test]
    fn one_key_is_named_as_one_key() {
        assert_eq!(span_label(c2(), 1), "C2");
        assert_eq!(span_label(c2(), 3), "C2\u{2013}D2");
        let mut state = SamplerState::new();
        state.add_wav_layer(c2(), PathBuf::from("kick.wav"), source().pcm.unwrap()).unwrap();
        let err = land(&mut state, &source(), &SLICES[..1], c2(), Feel::Break).unwrap_err();
        assert!(err.starts_with("C2 already holds a sound \u{00b7} clear it"), "{err}");
        assert!(!fits(&state, c2(), 1) && fits(&state, c2() + 1, 3));
        assert!(!fits(&state, NUM_PADS - 2, 3), "a run off the top fits");
    }

    #[test]
    fn a_run_past_the_top_key_or_no_cuts_at_all_is_refused() {
        let mut state = SamplerState::new();
        let before = state.clone();
        let err = land(&mut state, &source(), &SLICES, NUM_PADS - 2, Feel::Break).unwrap_err();
        assert!(err.contains("past the top key"), "{err}");
        let err = land(&mut state, &source(), &[], c2(), Feel::Break).unwrap_err();
        assert!(err.contains("no cuts"), "{err}");
        assert_eq!(state, before);
        // The last key on the bed is reachable exactly.
        assert!(land(&mut state, &source(), &SLICES, NUM_PADS - 3, Feel::Break).is_ok());
    }

    #[test]
    fn a_break_takes_a_choke_group_nobody_else_uses() {
        let mut state = SamplerState::new();
        state.pads[0].config.choke = 1; // a hat pair already on the kit
        let first = land(&mut state, &source(), &SLICES, c2(), Feel::Break).unwrap();
        assert_eq!(first.choke, Some(2));
        assert!((0..3).all(|o| {
            let config = state.pads[c2() + o].config;
            config.choke == 2 && config.trig == TrigMode::OneShot && config.poly == 1
        }));
        let second = land(&mut state, &source(), &SLICES, c2() + 12, Feel::Break).unwrap();
        assert_eq!(second.choke, Some(3), "two breaks would cut each other");
    }

    #[test]
    fn with_every_group_taken_a_break_lands_unchoked() {
        let mut state = SamplerState::new();
        for g in 1..=CHOKE_MAX {
            state.pads[usize::from(g)].config.choke = g;
        }
        let landing = land(&mut state, &source(), &SLICES, c2(), Feel::Break).unwrap();
        assert_eq!(landing.choke, None);
        assert_eq!(state.pads[c2()].config.choke, 0);
    }

    #[test]
    fn a_zones_choke_group_counts_as_taken() {
        let mut state = SamplerState::new();
        let mut zone_pad = PadState::empty(60);
        zone_pad.config.choke = 1;
        state.zones.push(Zone::new(50, 60, zone_pad));
        assert_eq!(land(&mut state, &source(), &SLICES, c2(), Feel::Break).unwrap().choke, Some(2));
    }

    #[test]
    fn a_melodic_chop_plays_while_held_and_chokes_nothing() {
        let mut state = SamplerState::new();
        let landing = land(&mut state, &source(), &SLICES, c2(), Feel::Melodic).unwrap();
        assert_eq!(landing.choke, None);
        let config = state.pads[c2()].config;
        assert_eq!(config.trig, TrigMode::Gate);
        assert_eq!(config.choke, 0);
        assert_eq!(config.poly, MELODIC_POLY);
        assert_eq!(config.release_ms, MELODIC_RELEASE_MS);
    }

    /// Chops live on pads; landing one in keys mode shows the pads, and
    /// keeps the zones for `K` to bring back.
    #[test]
    fn landing_in_keys_mode_turns_the_bed_to_pads_and_keeps_the_zones() {
        let mut state = SamplerState::new();
        state.zones.push(Zone::new(60, 70, PadState::empty(80)));
        state.set_mode(MapMode::Keys);
        let landing = land(&mut state, &source(), &SLICES, c2(), Feel::Break).unwrap();
        assert!(landing.switched_to_pads);
        assert_eq!(state.mode, MapMode::Pads);
        assert_eq!(state.zones.len(), 1, "a chop cost the player a zone");
        let again = land(&mut state, &source(), &SLICES, c2() + 12, Feel::Break).unwrap();
        assert!(!again.switched_to_pads);
    }

// ── Heard through the real engine ──
    //
    // The tests above check the kit; these check the sound. A break is cut at
    // its kicks, landed, and the pads the app would ship are loaded into the
    // sampler engine itself and played.

    use crate::sampler::chop::markers::Markers;
    use crate::sampler::chop::onset::{OnsetCurve, DEFAULT_SENSITIVITY};
    use crate::sampler::chop::testkit::boom_bap;
    use crate::sampler::chop::Band;
    use phosphor_plugin::{MidiEvent, Plugin};

    const RATE: f32 = 48_000.0;
    const BLOCK: usize = 512;

    /// A break from a file, cut at its kicks and landed from C2 as `feel`,
    /// with the engine holding exactly what the app would send it.
    fn chopped_break(feel: Feel) -> (phosphor_dsp::sampler::Sampler, Arc<SamplePcm>, Vec<(u64, u64)>) {
        let pcm = Arc::new(boom_bap(RATE).0);
        let curve = OnsetCurve::analyse(&pcm, 0, pcm.frames(), Band::Low);
        let mut markers = Markers::new(0, pcm.frames(), RATE);
        markers.propose(curve.pick(&pcm, DEFAULT_SENSITIVITY));
        let slices = markers.slices();
        let mut state = SamplerState::new();
        let src = LayerState::from_wav(PathBuf::from("break.wav"), Arc::clone(&pcm));
        let landing = land(&mut state, &src, &slices, c2(), feel).unwrap();

        let mut engine = phosphor_dsp::sampler::Sampler::new();
        engine.init(f64::from(RATE), BLOCK);
        for pad in landing.first..landing.first + landing.count {
            let shipped = state.engine_pad(pad).unwrap();
            engine.set_sampler_pad(pad as u8, &shipped.config, &shipped.layers);
        }
        (engine, pcm, slices)
    }

    /// `frames` of the left output, with each `(frame, key)` struck in turn.
    fn play(engine: &mut phosphor_dsp::sampler::Sampler, strikes: &[(usize, usize)], frames: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames);
        let mut at = 0;
        while at < frames {
            let n = BLOCK.min(frames - at);
            let events: Vec<MidiEvent> = strikes
                .iter()
                .filter(|&&(f, _)| (at..at + n).contains(&f))
                .map(|&(f, key)| MidiEvent {
                    sample_offset: (f - at) as u32,
                    status: 0x90,
                    data1: SamplerState::note_of_pad(c2() + key),
                    data2: 127,
                })
                .collect();
            let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
            engine.process(&[], &mut [&mut l[..], &mut r[..]], &events);
            out.extend_from_slice(&l);
            at += n;
        }
        out
    }

    /// How alike two stretches of sound are, whatever their level: 1.0 is the
    /// same shape.
    fn likeness(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm(a) * norm(b)).max(1e-12)
    }

    /// The slice as the engine's output stage hands it on: through its DC
    /// blocker, a one-pole high pass at 10 Hz. At a kick's 50 Hz that is an
    /// 11° phase shift — enough on its own to hold the raw comparison under
    /// 0.99 — so the reference goes through the same pole.
    fn as_the_engine_hears(slice: &[f32]) -> Vec<f32> {
        let r = 1.0 - std::f32::consts::TAU * 10.0 / RATE;
        let (mut x1, mut y1) = (0.0f32, 0.0f32);
        slice
            .iter()
            .map(|&x| {
                y1 = x - x1 + r * y1;
                x1 = x;
                y1
            })
            .collect()
    }

    /// Each key plays its own slice of the break and nothing else.
    #[test]
    fn each_key_plays_its_own_slice() {
        let (mut engine, pcm, slices) = chopped_break(Feel::Break);
        assert_eq!(slices.len(), 3, "the break has three kicks: {slices:?}");
        for (key, &(start, end)) in slices.iter().enumerate() {
            // A fresh engine per key would hide a voice left over from the
            // key before; one engine, with silence between, does not.
            let heard = play(&mut engine, &[(0, key)], (end - start) as usize + 4_800);
            let slice = as_the_engine_hears(&pcm.data[start as usize..end as usize]);
            // Past the edge fades at both ends.
            let (a, b) = (480, slice.len() - 480);
            let shape = likeness(&heard[a..b], &slice[a..b]);
            assert!(shape > 0.999, "key {key} did not sound like its slice: {shape}");
            let after = heard[slice.len() + 480..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(after < 1e-3, "key {key} kept sounding past its slice: {after}");
        }
    }

    /// A break's slices share a choke group: the second hit silences the
    /// first, so what sounds after it is the second slice alone. A melodic
    /// chop lets them ring together.
    #[test]
    fn a_breaks_next_hit_cuts_the_one_before_and_a_melodic_one_does_not() {
        let hit = 2_400; // 50 ms into the first slice
        // Well past the choke's 3 ms fade. Not the fade itself: the two
        // renders have different histories, and the engine's DC blocker
        // carries a one-signed tail of that difference that halves every
        // 10 ms. The kick it would take to fail this is still ringing at
        // a third of full scale here.
        let settle = hit + 4_800;
        let length = hit + 9_600;
        let (mut both, _, _) = chopped_break(Feel::Break);
        let (mut alone, _, _) = chopped_break(Feel::Break);
        let together = play(&mut both, &[(0, 0), (hit, 1)], length);
        let second = play(&mut alone, &[(hit, 1)], length);
        let leftover = together[settle..].iter().zip(&second[settle..]).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(leftover < 2e-3, "the first slice was still sounding under the second: {leftover}");

        let (mut both, _, _) = chopped_break(Feel::Melodic);
        let (mut alone, _, _) = chopped_break(Feel::Melodic);
        // Melodic slices are gated: holding means never letting go here.
        let together = play(&mut both, &[(0, 0), (hit, 1)], length);
        let second = play(&mut alone, &[(hit, 1)], length);
        let leftover = together[settle..].iter().zip(&second[settle..]).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(leftover > 0.05, "a melodic slice was cut by its neighbour");
    }

    /// A take chopped stays a take — its slices go to the sidecar with it.
    #[test]
    fn a_takes_slices_are_takes() {
        let mut state = SamplerState::new();
        let mut take = source();
        take.source = LayerSource::Take;
        take.path = PathBuf::new();
        land(&mut state, &take, &SLICES, c2(), Feel::Break).unwrap();
        assert!(state.pads[c2()].layers[0].source == LayerSource::Take);
        assert_eq!(state.takes().count(), 3);
    }
}
