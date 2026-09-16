//! Plugin API definitions for Phosphor.
//!
//! This crate defines the trait that all plugins (instruments, effects,
//! analyzers) must implement. Built-in DSP and third-party plugins
//! use the same interface — no special casing.

use std::fmt;

pub mod sample;

/// Plugin category — determines where it appears in the UI and how it's routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum PluginCategory {
    /// Generates audio from MIDI input (synths, samplers).
    Instrument,
    /// Processes audio (filters, delays, reverbs, compressors).
    Effect,
    /// Reads audio for display (spectrum analyzer, oscilloscope).
    Analyzer,
    /// Utility (gain, panner, test tone generator).
    Utility,
}

impl fmt::Display for PluginCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Instrument => write!(f, "Instrument"),
            Self::Effect => write!(f, "Effect"),
            Self::Analyzer => write!(f, "Analyzer"),
            Self::Utility => write!(f, "Utility"),
        }
    }
}

/// Metadata about a plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub category: PluginCategory,
}

/// A MIDI event with a sample-accurate offset within the current buffer.
#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    /// Sample offset within the buffer (0 = start of buffer).
    pub sample_offset: u32,
    /// MIDI status byte.
    pub status: u8,
    /// First data byte.
    pub data1: u8,
    /// Second data byte.
    pub data2: u8,
}

/// Parameter descriptor for plugin parameters.
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: String,
}

/// The core plugin trait. Every synth, effect, and utility implements this.
pub trait Plugin: Send {
    /// Plugin metadata.
    fn info(&self) -> PluginInfo;

    /// Called once when the plugin is loaded. Preallocate everything here.
    fn init(&mut self, sample_rate: f64, max_buffer_size: usize);

    /// Process audio. Called from the audio thread — must be real-time safe.
    ///
    /// - `inputs`: input audio buffers (one slice per channel). Empty for instruments.
    /// - `outputs`: output audio buffers to write into (one slice per channel).
    /// - `midi_events`: MIDI events for this buffer, sorted by `sample_offset`.
    fn process(
        &mut self,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        midi_events: &[MidiEvent],
    );

    /// Number of parameters this plugin exposes.
    fn parameter_count(&self) -> usize;

    /// Get info about a parameter.
    fn parameter_info(&self, index: usize) -> Option<ParameterInfo>;

    /// Get current parameter value (0.0..1.0 normalized).
    fn get_parameter(&self, index: usize) -> f32;

    /// Set parameter value. Clamped to 0.0..1.0.
    fn set_parameter(&mut self, index: usize, value: f32);

    /// Reset internal state (clear delay lines, reset envelopes, etc).
    fn reset(&mut self);

    /// Hand a sampler one pad's configuration and layers. A default no-op,
    /// because most instruments have no notion of a pad.
    ///
    /// Real-time contract: the implementation copies into storage it
    /// already owns — the `Arc` clones inside the layers are refcount
    /// increments, and the `Arc`s it replaces drop as refcount decrements,
    /// because the caller's side retains a reference to every buffer it
    /// has ever sent (see the ownership contract in [`sample`]).
    fn set_sampler_pad(
        &mut self,
        _pad: u8,
        _config: &sample::PadConfig,
        _layers: &[sample::PadLayer],
    ) {
    }

    /// Audition exactly one layer, or `None` to stop auditioning.
    ///
    /// The UI's way of making a sound without playing a note: the trim
    /// strip's nudges, the layer list's cursor. Whatever answers this plays
    /// outside the pad map's voice accounting — it must not steal a voice
    /// from the kit, be stolen from by one, or count toward poly or a choke
    /// group, because it is the player listening rather than playing.
    ///
    /// Real-time contract is [`Plugin::set_sampler_pad`]'s: the `Arc` inside
    /// the layer is cloned into storage the implementation already owns, and
    /// the one it replaces drops as a refcount decrement.
    fn set_sampler_preview(&mut self, _preview: Option<&sample::PreviewLayer>) {}

    /// Give a sampler the one child instrument its phrase layers play
    /// through, or `None` to take it away.
    ///
    /// One child per sampler, not one per pad: every phrase on every pad
    /// sounds through this instrument, so it is rendered once per block
    /// however many phrases are running.
    ///
    /// The box travels exactly as `MixerCommand::SetInstrument`'s does. It
    /// is built on the UI thread because that is the only thread allowed to
    /// allocate, the implementation calls `init` on it because that is where
    /// its voices are built, and the child it replaces is dropped — freed —
    /// on the audio thread. That last part is the accepted `SetInstrument`
    /// precedent: the command that carries it is charged the heavy rate
    /// precisely so the callback's budget has already paid for the free.
    fn set_sampler_child(&mut self, _child: Option<Box<dyn Plugin>>) {}

    /// Hand a sampler one pad's phrase layers, whole.
    ///
    /// Real-time contract is [`Plugin::set_sampler_pad`]'s: the `Arc` inside
    /// each phrase is cloned into fixed storage the implementation already
    /// owns, and the ones it replaces drop as refcount decrements, because
    /// the caller retains a reference to every event list it has sent (see
    /// the ownership contract in [`sample`]).
    fn set_sampler_phrases(&mut self, _pad: u8, _phrases: &[sample::PadPhrase]) {}
}

/// Clamp a parameter value to the valid range.
#[inline]
pub fn clamp_parameter(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_parameter_bounds() {
        assert_eq!(clamp_parameter(0.5), 0.5);
        assert_eq!(clamp_parameter(-1.0), 0.0);
        assert_eq!(clamp_parameter(2.0), 1.0);
        assert_eq!(clamp_parameter(0.0), 0.0);
        assert_eq!(clamp_parameter(1.0), 1.0);
    }

    #[test]
    fn clamp_parameter_nan_handling() {
        // NaN.clamp returns NaN in Rust — we should be aware of this
        let result = clamp_parameter(f32::NAN);
        assert!(result.is_nan(), "NaN input produces NaN — callers must validate");
    }

    #[test]
    fn plugin_category_display() {
        assert_eq!(format!("{}", PluginCategory::Instrument), "Instrument");
        assert_eq!(format!("{}", PluginCategory::Effect), "Effect");
    }

    #[test]
    fn midi_event_is_copy() {
        let event = MidiEvent {
            sample_offset: 0,
            status: 0x90,
            data1: 60,
            data2: 100,
        };
        let copy = event;
        assert_eq!(copy.status, event.status);
    }
}
