//! Plugin API definitions for Phosphor.
//!
//! This crate defines the trait that all plugins (instruments, effects,
//! analyzers) must implement. Built-in DSP and third-party plugins
//! use the same interface — no special casing.

use std::fmt;

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
