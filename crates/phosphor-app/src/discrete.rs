//! Discrete controls: the knobs that pick a thing rather than set a level.
//!
//! A drum kit, a factory patch, a cartridge, a waveform switch. Every one of
//! them is stored in `synth_params` as a normalised `f32` like every other
//! control, and the instrument turns that fraction into a position by
//! multiplying by the number of positions it has.
//!
//! Which means the fraction only names a thing *as long as the count does not
//! change*. It has changed twice: the drum rack went from ten kits to fifteen
//! and the Jupiter from 42 patches to 64, and both times every session saved
//! before the change reopened on a different instrument — the 909 became the
//! 707, and a Jupiter patch moved two thirds of the way down the bank. Nobody
//! notices at load time, because a drum kit that is not the one you left is
//! still a drum kit.
//!
//! So sessions store these controls by *position* instead, and this module is
//! the conversion. See [`index_of`] and [`knob_at`].
//!
//! ## How the positions are found
//!
//! Every instrument already answers two questions about its own panel:
//! `is_discrete(index)` — is this a selector — and `step_discrete(index,
//! value, up)` — where is the next position. Nothing exposes the *count*, and
//! rather than add a sixth spelling of that to five modules, this walks the
//! control with the instrument's own stepping function: down until it stops
//! moving, then up, counting, until it stops moving again.
//!
//! That makes the position numbering exactly what the player sees when they
//! hold the key down, and it needs nothing from an instrument that it does not
//! already publish.

use crate::state::InstrumentType;

/// A ceiling on how far a selector will be walked.
///
/// The longest in the project is the phosphor synth's 49-position coarse tune,
/// and the widest thing this could ever reasonably be is a bank of a few
/// hundred. It exists so that an instrument whose stepping function does not
/// converge cannot hang the save.
const MAX_POSITIONS: usize = 1_024;

/// Whether `param` picks a thing rather than sets a level.
#[must_use]
pub fn is_discrete(instrument: InstrumentType, param: usize) -> bool {
    match instrument {
        InstrumentType::Synth | InstrumentType::Sampler => phosphor_dsp::synth::is_discrete(param),
        InstrumentType::DrumRack => phosphor_dsp::drum_rack::is_discrete(param),
        InstrumentType::DX7 => phosphor_dsp::dx7::is_discrete(param),
        InstrumentType::Jupiter8 => phosphor_dsp::jupiter::is_discrete(param),
        InstrumentType::Odyssey => phosphor_dsp::odyssey::is_discrete(param),
        InstrumentType::Juno60 => phosphor_dsp::juno::is_discrete(param),
        InstrumentType::Rhodes => phosphor_dsp::rhodes::is_discrete(param),
        InstrumentType::LittlePhatty => phosphor_dsp::phatty::is_discrete(param),
        InstrumentType::Prophet6 => phosphor_dsp::prophet6::is_discrete(param),
    }
}

/// The knob position one step up or down from `value`, or `value` unchanged
/// when `param` is not a selector or the knob is already at the end.
#[must_use]
pub fn step(instrument: InstrumentType, param: usize, value: f32, up: bool) -> f32 {
    match instrument {
        InstrumentType::Synth | InstrumentType::Sampler => {
            phosphor_dsp::synth::step_discrete(param, value, up)
        }
        InstrumentType::DrumRack => phosphor_dsp::drum_rack::step_discrete(param, value, up),
        InstrumentType::DX7 => phosphor_dsp::dx7::step_discrete(param, value, up),
        InstrumentType::Jupiter8 => phosphor_dsp::jupiter::step_discrete(param, value, up),
        InstrumentType::Odyssey => phosphor_dsp::odyssey::step_discrete(param, value, up),
        InstrumentType::Juno60 => phosphor_dsp::juno::step_discrete(param, value, up),
        InstrumentType::Rhodes => phosphor_dsp::rhodes::step_discrete(param, value, up),
        InstrumentType::LittlePhatty => {
            phosphor_dsp::phatty::step_discrete(param, value, up)
        }
        InstrumentType::Prophet6 => phosphor_dsp::prophet6::step_discrete(param, value, up),
    }
}

/// Every position of a selector, in order, as knob values.
///
/// `None` when `param` is not a selector. Never empty otherwise.
#[must_use]
pub fn positions(instrument: InstrumentType, param: usize) -> Option<Vec<f32>> {
    if !is_discrete(instrument, param) {
        return None;
    }

    // Down to the bottom. Starting from 0.0 rather than from the caller's
    // value so that the walk is the same every time and cannot be biased by a
    // knob that arrived out of range.
    let mut knob = 0.0f32;
    for _ in 0..MAX_POSITIONS {
        let down = step(instrument, param, knob, false);
        if down == knob {
            break;
        }
        knob = down;
    }

    let mut found = vec![knob];
    for _ in 0..MAX_POSITIONS {
        let up = step(instrument, param, knob, true);
        if up == knob {
            break;
        }
        knob = up;
        found.push(knob);
    }
    Some(found)
}

/// Which position `value` selects, counting from zero.
///
/// The nearest position, which for the five instruments that lay their steps
/// out as bucket centres is exactly the step the instrument itself reads —
/// the boundary between two centres is the boundary between two buckets.
#[must_use]
pub fn index_of(instrument: InstrumentType, param: usize, value: f32) -> Option<usize> {
    let found = positions(instrument, param)?;
    let mut best = 0;
    let mut best_distance = f32::INFINITY;
    for (i, knob) in found.iter().enumerate() {
        // NaN never compares less than anything, so a knob that arrived as one
        // lands on position zero rather than on whatever it was last compared
        // against.
        let distance = (knob - value).abs();
        if distance < best_distance {
            best = i;
            best_distance = distance;
        }
    }
    Some(best)
}

/// The knob position that selects `index`, clamped to the last one when the
/// bank has fewer entries than it did.
#[must_use]
pub fn knob_at(instrument: InstrumentType, param: usize, index: usize) -> Option<f32> {
    let found = positions(instrument, param)?;
    found.get(index).or_else(|| found.last()).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::{drum_rack, dx7, jupiter, juno, odyssey, phatty, prophet6, rhodes, synth};

    /// Every instrument's selectors, walked with that instrument's own
    /// stepping, come out at the counts the instrument publishes.
    #[test]
    fn the_walk_finds_every_position() {
        assert_eq!(
            positions(InstrumentType::DrumRack, drum_rack::P_KIT).unwrap().len(),
            drum_rack::KIT_COUNT
        );
        assert_eq!(
            positions(InstrumentType::Jupiter8, jupiter::P_PATCH).unwrap().len(),
            jupiter::PATCH_COUNT
        );
        assert_eq!(
            positions(InstrumentType::Juno60, juno::P_PATCH).unwrap().len(),
            juno::PATCH_COUNT
        );
        assert_eq!(
            positions(InstrumentType::Odyssey, odyssey::P_PATCH).unwrap().len(),
            odyssey::PATCH_COUNT
        );
        assert_eq!(
            positions(InstrumentType::Rhodes, rhodes::P_PATCH).unwrap().len(),
            rhodes::PATCH_COUNT
        );
        assert_eq!(
            positions(InstrumentType::LittlePhatty, phatty::P_PATCH).unwrap().len(),
            phatty::PATCH_COUNT
        );
        // Its velocity sensitivity is the instrument's own -8..+8, seventeen
        // positions, which is the second longest selector in the project.
        assert_eq!(
            positions(InstrumentType::LittlePhatty, phatty::P_VEL_SENS).unwrap().len(),
            17
        );
        assert_eq!(
            positions(InstrumentType::DX7, dx7::P_BANK).unwrap().len(),
            dx7::BANK_COUNT
        );
        // The Prophet-6's two selectors multiply out to its whole factory set,
        // and its program knob is the longest preset selector in the project
        // at a hundred positions — well inside MAX_POSITIONS, which is what
        // stops the walk from being a save-time hang.
        assert_eq!(
            positions(InstrumentType::Prophet6, prophet6::P_BANK).unwrap().len(),
            prophet6::BANK_COUNT
        );
        let programs = positions(InstrumentType::Prophet6, prophet6::P_PROGRAM).unwrap().len();
        assert_eq!(programs, prophet6::PROGRAMS_PER_BANK);
        assert_eq!(programs * prophet6::BANK_COUNT, prophet6::PROGRAM_COUNT);
        // Its effect-B type selector is a ten-position list, and its bend
        // range a thirteen-position one.
        assert_eq!(positions(InstrumentType::Prophet6, prophet6::P_FXB_TYPE).unwrap().len(), 10);
        assert_eq!(positions(InstrumentType::Prophet6, prophet6::P_BEND_RANGE).unwrap().len(), 13);
        // The DX7's two selectors multiply out to its whole factory set.
        let patches = positions(InstrumentType::DX7, dx7::P_PATCH).unwrap().len();
        assert_eq!(patches * dx7::BANK_COUNT, dx7::VOICE_COUNT);
        assert_eq!(
            positions(InstrumentType::Synth, synth::P_PATCH).unwrap().len(),
            synth::PATCH_COUNT
        );
        // The phosphor synth is also the one instrument whose panel the
        // sampler shares, so both have to walk the same way.
        assert_eq!(
            positions(InstrumentType::Sampler, synth::P_PATCH).unwrap().len(),
            synth::PATCH_COUNT
        );
        // Its coarse tune is the longest selector in the project: 49
        // positions, two octaves either way in semitones.
        assert_eq!(positions(InstrumentType::Synth, synth::P_A_TUNE).unwrap().len(), 49);
        // ...and a fader is still a fader.
        assert!(positions(InstrumentType::Synth, synth::P_CUTOFF).is_none());
    }

    /// The round trip that the session format depends on: a knob position
    /// names a step, and that step names the same knob position back.
    #[test]
    fn a_position_round_trips_through_its_index() {
        for instrument in InstrumentType::ALL {
            let count = crate::preset::param_count(*instrument);
            for param in 0..count {
                if !is_discrete(*instrument, param) {
                    assert!(index_of(*instrument, param, 0.5).is_none());
                    continue;
                }
                let found = positions(*instrument, param).unwrap();
                for (i, knob) in found.iter().enumerate() {
                    assert_eq!(
                        index_of(*instrument, param, *knob),
                        Some(i),
                        "{instrument:?} param {param} position {i} ({knob})"
                    );
                    assert_eq!(knob_at(*instrument, param, i), Some(*knob));
                }
                // Past the end is the last position, not a panic and not a
                // wrap round to the first.
                assert_eq!(
                    knob_at(*instrument, param, found.len() + 100),
                    found.last().copied()
                );
            }
        }
    }

    /// The defect this whole module exists for, played out on the control it
    /// happened to: a kit chosen when there were ten of them still names that
    /// kit when there are fifteen.
    #[test]
    fn an_index_survives_the_bank_growing() {
        // What a session saved against a ten-kit rack would have stored: the
        // 909 is position 1 of 10, which is the fraction 0.15.
        let ten_kit_knob = 1.5 / 10.0;
        let fifteen_kit_knob =
            knob_at(InstrumentType::DrumRack, drum_rack::P_KIT, 1).unwrap();

        // The fraction, reread against fifteen kits, is the 707.
        assert_eq!(drum_rack::discrete_label(drum_rack::P_KIT, ten_kit_knob), Some("707"));
        // The index is still the 909.
        assert_eq!(
            drum_rack::discrete_label(drum_rack::P_KIT, fifteen_kit_knob),
            Some("909")
        );
    }

    /// Nothing in the walk can be made to hang or to index off the end by a
    /// knob that arrived as nonsense — `synth_params` is public and a session
    /// file is a text file someone can edit.
    #[test]
    fn nonsense_knob_values_land_on_a_real_position() {
        for value in [-1.0f32, 0.0, 1.0, 2.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let index = index_of(InstrumentType::DrumRack, drum_rack::P_KIT, value).unwrap();
            assert!(index < drum_rack::KIT_COUNT, "{value} landed on position {index}");
        }
    }
}
