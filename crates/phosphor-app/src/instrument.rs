//! The plugin behind an instrument type.
//!
//! One factory, because there were about to be several: a track being
//! created builds one, a sequencer track pointed at a different child
//! builds another, and the sampler's resampler builds a third — offline,
//! to render a performance into a pad. Two lists of instruments is one
//! list that eventually forgets an instrument.
//!
//! It lives here rather than beside the track code in the front end
//! because the offline render ([`crate::sampler::render`]) needs it and
//! that render belongs in this crate: it is application logic — capture,
//! trim, naming — with a plugin in the middle of it, not drawing. This
//! crate already depends on the DSP crate for parameter defaults, so
//! moving the factory down costs nothing and gives the render a factory
//! to call without the front end handing one in.
//!
//! The sequencer has no plugin — it drives one — and answers with the
//! phosphor synth so that the slot is never left empty; nothing reaches
//! this with it, because a sequencer track carries its child's type.

use crate::state::InstrumentType;

/// Build the instrument. Never on the audio thread: the mixer is handed a
/// finished box through [`MixerCommand::SetInstrument`], which is the same
/// shape every other "make a thing" in this application has.
#[must_use]
pub fn build_plugin(instrument: InstrumentType) -> Box<dyn phosphor_plugin::Plugin + Send> {
    match instrument {
        InstrumentType::Synth | InstrumentType::Sequencer => {
            Box::new(phosphor_dsp::synth::PhosphorSynth::new())
        }
        InstrumentType::Sampler => Box::new(phosphor_dsp::sampler::Sampler::new()),
        InstrumentType::DrumRack => Box::new(phosphor_dsp::drum_rack::DrumRack::new()),
        InstrumentType::DX7 => Box::new(phosphor_dsp::dx7::Dx7Synth::new()),
        InstrumentType::Jupiter8 => Box::new(phosphor_dsp::jupiter::Jupiter8Synth::new()),
        InstrumentType::Prophet6 => Box::new(phosphor_dsp::prophet6::Prophet6::new()),
        InstrumentType::Teo5 => Box::new(phosphor_dsp::teo5::Teo5::new()),
        InstrumentType::Odyssey => Box::new(phosphor_dsp::odyssey::OdysseySynth::new()),
        InstrumentType::Juno60 => Box::new(phosphor_dsp::juno::Juno60Synth::new()),
        InstrumentType::Rhodes => Box::new(phosphor_dsp::rhodes::RhodesPiano::new()),
        InstrumentType::LittlePhatty => Box::new(phosphor_dsp::phatty::LittlePhatty::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every instrument in the menu builds something, and what it builds
    /// takes the panel its defaults were written for. A factory that
    /// silently answered the wrong plugin would load every value of a
    /// session into the wrong control.
    #[test]
    fn every_instrument_builds_a_plugin_that_fits_its_panel() {
        for &instrument in InstrumentType::ALL {
            let plugin = build_plugin(instrument);
            if instrument.is_sequencer() {
                continue; // no panel of its own — the child carries one
            }
            assert_eq!(
                plugin.parameter_count(),
                crate::preset::defaults(instrument).len(),
                "{} builds a plugin with a different panel",
                instrument.label(),
            );
        }
    }
}
