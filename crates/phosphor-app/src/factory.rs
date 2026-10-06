//! Every sound an instrument ships with, as the panel that loads it.
//!
//! The factory sets are spread over the instruments in four shapes: a patch
//! knob that carries the whole panel (the phosphor synth, Jupiter, Juno,
//! Odyssey, Rhodes, Phatty), a bank and a program that carry it together
//! (Prophet-6, TEO-5), a bank and a patch that pick a voice inside the plugin
//! and leave the panel alone (DX7), and a kit knob (the drum rack). This
//! walks all four the way a player does — through [`crate::discrete`]'s own
//! positions and [`crate::discrete::reload_panel_for_selector`] — so the list
//! is exactly what the panel can reach, and a bank that grows grows it.
//!
//! It exists for the tools that have to hear every one of them: the sound
//! fingerprint (`examples/fingerprint.rs`), which proves that a change made
//! for speed changed no sound, and the fade measurement
//! (`examples/tail_cost.rs`).

use crate::discrete;
use crate::state::InstrumentType;

/// One factory sound: where it lives on the selectors, and the whole panel.
#[derive(Debug, Clone, PartialEq)]
pub struct FactorySound {
    /// The selector positions that name it, counting from zero — `[bank,
    /// program]` for the two-knob banks, `[patch]` for the rest, and empty
    /// for an instrument with no factory set.
    pub position: Vec<usize>,
    /// Every control, as the instrument's panel holds them.
    pub params: Vec<f32>,
}

impl FactorySound {
    /// A name for reports: the instrument and the selector positions.
    #[must_use]
    pub fn name(&self, instrument: InstrumentType) -> String {
        let at: Vec<String> = self.position.iter().map(|p| format!("{p:03}")).collect();
        if at.is_empty() {
            instrument.label().to_string()
        } else {
            format!("{} {}", instrument.label(), at.join("/"))
        }
    }
}

/// The selectors that together name a factory sound, outermost first.
///
/// The bank comes first on the two-knob instruments so that the list reads
/// in the order the instrument's own manual numbers them.
fn selectors(instrument: InstrumentType) -> Vec<usize> {
    use phosphor_dsp::{drum_rack, dx7, juno, jupiter, odyssey, phatty, prophet6, rhodes, synth, teo5};
    match instrument {
        InstrumentType::Synth => vec![synth::P_PATCH],
        InstrumentType::Jupiter8 => vec![jupiter::P_PATCH],
        InstrumentType::Juno60 => vec![juno::P_PATCH],
        InstrumentType::Odyssey => vec![odyssey::P_PATCH],
        InstrumentType::Rhodes => vec![rhodes::P_PATCH],
        InstrumentType::LittlePhatty => vec![phatty::P_PATCH],
        InstrumentType::Prophet6 => vec![prophet6::P_BANK, prophet6::P_PROGRAM],
        InstrumentType::Teo5 => vec![teo5::P_BANK, teo5::P_PROGRAM],
        InstrumentType::DX7 => vec![dx7::P_BANK, dx7::P_PATCH],
        InstrumentType::DrumRack => vec![drum_rack::P_KIT],
        // No factory set: the sampler plays what it is given, and the
        // sequencer has no panel of its own.
        InstrumentType::Sampler | InstrumentType::Sequencer => Vec::new(),
    }
}

/// Every factory sound `instrument` has, in bank order. An instrument with
/// no factory set answers its default panel, once, so that every instrument
/// has at least one sound to play.
#[must_use]
pub fn sounds(instrument: InstrumentType) -> Vec<FactorySound> {
    let defaults = crate::preset::defaults(instrument);
    let axes: Vec<(usize, Vec<f32>)> = selectors(instrument)
        .into_iter()
        .filter_map(|param| discrete::positions(instrument, param).map(|at| (param, at)))
        .collect();

    // Every combination of positions, the last selector turning fastest.
    let mut combos: Vec<Vec<usize>> = vec![Vec::new()];
    for (_, at) in &axes {
        combos = combos
            .into_iter()
            .flat_map(|prefix| {
                (0..at.len()).map(move |i| {
                    let mut next = prefix.clone();
                    next.push(i);
                    next
                })
            })
            .collect();
    }

    combos
        .into_iter()
        .map(|position| {
            let mut params = defaults.clone();
            for ((param, at), &i) in axes.iter().zip(&position) {
                params[*param] = at[i];
            }
            // The last selector stands for all of them: on the two-knob banks
            // the reload reads both from the panel either way.
            if let Some((param, _)) = axes.last() {
                discrete::reload_panel_for_selector(instrument, &mut params, *param);
            }
            FactorySound { position, params }
        })
        .collect()
}

/// A fresh instrument at `sample_rate`, with `params` on its panel and
/// nothing sounding: what a track holds the moment a patch is loaded, and
/// what an offline render starts from.
#[must_use]
pub fn load(
    instrument: InstrumentType,
    params: &[f32],
    sample_rate: f64,
    max_block: usize,
) -> Box<dyn phosphor_plugin::Plugin + Send> {
    let mut plugin = crate::instrument::build_plugin(instrument);
    plugin.init(sample_rate, max_block);
    for (index, &value) in params.iter().enumerate() {
        plugin.set_parameter(index, value);
    }
    plugin.reset();
    plugin
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::{drum_rack, dx7, juno, jupiter, odyssey, phatty, prophet6, rhodes, synth, teo5};

    /// The walk reaches every factory sound each instrument publishes, and
    /// no more.
    #[test]
    fn every_factory_set_is_counted_in_full() {
        use InstrumentType as I;
        let count = |i| sounds(i).len();
        assert_eq!(count(I::DX7), dx7::VOICE_COUNT);
        assert_eq!(count(I::Prophet6), prophet6::PROGRAM_COUNT);
        assert_eq!(count(I::Teo5), teo5::PROGRAM_COUNT);
        assert_eq!(count(I::Jupiter8), jupiter::PATCH_COUNT);
        assert_eq!(count(I::Juno60), juno::PATCH_COUNT);
        assert_eq!(count(I::Odyssey), odyssey::PATCH_COUNT);
        assert_eq!(count(I::Rhodes), rhodes::PATCH_COUNT);
        assert_eq!(count(I::LittlePhatty), phatty::PATCH_COUNT);
        assert_eq!(count(I::Synth), synth::PATCH_COUNT);
        assert_eq!(count(I::DrumRack), drum_rack::KIT_COUNT);
        assert_eq!(count(I::Sampler), 1);
    }

    /// Each one is a different panel — a walk that set the selector and then
    /// lost it would hand back the same sound five hundred times.
    #[test]
    fn every_factory_sound_is_its_own_panel() {
        for &instrument in InstrumentType::ALL {
            if instrument.is_sequencer() {
                continue;
            }
            let all = sounds(instrument);
            for (i, a) in all.iter().enumerate() {
                assert_eq!(a.params.len(), crate::preset::param_count(instrument));
                for b in &all[i + 1..] {
                    assert_ne!(a.params, b.params, "{} and {}", a.name(instrument), b.name(instrument));
                }
            }
        }
    }
}
