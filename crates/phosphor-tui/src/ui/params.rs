//! How an instrument's controls read, in one place.
//!
//! Two panels draw the same parameters: the narrow strip on the left of the
//! clip view, and the full panel in the `[inst]` tab beside it. They differ
//! only in how much room they have, so the answer to "what is this control
//! called, and what does it say" lives here rather than being written twice
//! and drifting.
//!
//! Nothing here decides anything. Every question is forwarded to the
//! instrument that owns the parameter — the same functions the session
//! format and the discrete-step logic consult — so a panel cannot disagree
//! with the sound.

use super::*;

/// The names of an instrument's controls, in panel order.
pub(super) fn names(instrument: Option<InstrumentType>) -> &'static [&'static str] {
    match instrument {
        Some(InstrumentType::DrumRack) => &phosphor_dsp::drum_rack::PARAM_NAMES,
        Some(InstrumentType::DX7) => &phosphor_dsp::dx7::PARAM_NAMES,
        Some(InstrumentType::Jupiter8) => &phosphor_dsp::jupiter::PARAM_NAMES,
        Some(InstrumentType::Odyssey) => &phosphor_dsp::odyssey::PARAM_NAMES,
        Some(InstrumentType::Juno60) => &phosphor_dsp::juno::PARAM_NAMES,
        Some(InstrumentType::Rhodes) => &phosphor_dsp::rhodes::PARAM_NAMES,
        Some(InstrumentType::LittlePhatty) => &phosphor_dsp::phatty::PARAM_NAMES,
        Some(InstrumentType::Prophet6) => &phosphor_dsp::prophet6::PARAM_NAMES,
        Some(InstrumentType::Teo5) => &phosphor_dsp::teo5::PARAM_NAMES,
        // The phosphor synth and the sampler share a panel; a track with no
        // instrument on it has none, and gets that one's names rather than an
        // empty list, since it also has no values to draw under them.
        _ => &phosphor_dsp::synth::PARAM_NAMES,
    }
}

/// The word a control shows when it is a selector rather than a knob — a
/// patch name, a waveform, a switch position — or `None` when it is a knob.
///
/// Some instruments answer from the one value; two of them (the DX7's voice,
/// the Prophet-6's program) need the whole block, because the name depends on
/// a bank selector as well as on the knob itself.
pub(super) fn discrete_label(
    instrument: Option<InstrumentType>,
    params: &[f32],
    index: usize,
) -> Option<&'static str> {
    let value = params.get(index).copied().unwrap_or(0.0);
    match instrument {
        Some(InstrumentType::DrumRack) => phosphor_dsp::drum_rack::discrete_label(index, value),
        Some(InstrumentType::DX7) => phosphor_dsp::dx7::discrete_label(params, index),
        Some(InstrumentType::Jupiter8) => phosphor_dsp::jupiter::discrete_label(index, value),
        Some(InstrumentType::Odyssey) => phosphor_dsp::odyssey::discrete_label(index, value),
        Some(InstrumentType::Juno60) => phosphor_dsp::juno::discrete_label(index, value),
        Some(InstrumentType::Rhodes) => phosphor_dsp::rhodes::discrete_label(index, value),
        Some(InstrumentType::LittlePhatty) => phosphor_dsp::phatty::discrete_label(index, value),
        Some(InstrumentType::Prophet6) => phosphor_dsp::prophet6::discrete_label(params, index),
        Some(InstrumentType::Teo5) => phosphor_dsp::teo5::discrete_label(params, index),
        _ => phosphor_dsp::synth::discrete_label(index, value),
    }
}

/// What a knob reads as: a time where the instrument says the control is one,
/// and a percentage otherwise.
///
/// No instrument's time controls are linear in time and no two of them sit at
/// the same indices, so each reports its own seconds. The DX7 is the one
/// panel with no time slider on it — its rates are the operators' own — so it
/// is the only instrument that falls through to a percentage everywhere.
pub(super) fn value_text(
    instrument: Option<InstrumentType>,
    params: &[f32],
    index: usize,
) -> String {
    let value = params.get(index).copied().unwrap_or(0.0);
    let seconds = match instrument {
        Some(InstrumentType::DX7) => None,
        Some(InstrumentType::Prophet6) => phosphor_dsp::prophet6::param_seconds(index, value),
        Some(InstrumentType::Teo5) => phosphor_dsp::teo5::param_seconds(index, value),
        Some(InstrumentType::Juno60) => phosphor_dsp::juno::param_seconds(index, value),
        Some(InstrumentType::LittlePhatty) => phosphor_dsp::phatty::param_seconds(index, value),
        Some(InstrumentType::Rhodes) => phosphor_dsp::rhodes::param_seconds(index, value),
        Some(InstrumentType::Odyssey) => phosphor_dsp::odyssey::param_seconds(index, value),
        Some(InstrumentType::Jupiter8) => phosphor_dsp::jupiter::param_seconds(index, value),
        Some(InstrumentType::DrumRack) => {
            // The rack's decay times belong to the selected machine, not to
            // one machine's answer for all fifteen, so the kit selector is
            // read as well as the knob.
            let kit = phosphor_dsp::drum_rack::DrumKit::from_param(
                params.get(phosphor_dsp::drum_rack::P_KIT).copied().unwrap_or(0.0),
            );
            phosphor_dsp::drum_rack::param_seconds(kit, index, value)
        }
        _ => phosphor_dsp::synth::param_seconds(index, value),
    };
    match seconds {
        Some(seconds) if seconds < 1.0 => format!("{:.0}ms", seconds * 1000.0),
        Some(seconds) => format!("{seconds:.1}s"),
        None => format!("{:.0}%", value * 100.0),
    }
}

/// A knob's travel as a bar `width` cells wide.
pub(super) fn bar(value: f32, width: usize) -> String {
    let filled = ((value.clamp(0.0, 1.0) * width as f32) as usize).min(width);
    "\u{2588}".repeat(filled) + &"\u{2591}".repeat(width - filled)
}
