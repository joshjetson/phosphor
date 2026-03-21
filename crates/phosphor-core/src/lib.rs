pub mod audio;
pub mod cpal_backend;
pub mod engine;
pub mod mixer;
pub mod project;
pub mod transport;

use serde::{Deserialize, Serialize};

/// Configuration for the audio engine.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Audio buffer size in samples. Lower = less latency, more CPU.
    /// Typical values: 32, 64, 128, 256, 512.
    pub buffer_size: u32,
    /// Sample rate in Hz. Typical values: 44100, 48000, 96000.
    pub sample_rate: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            buffer_size: 64,
            sample_rate: 44100,
        }
    }
}

impl EngineConfig {
    /// Buffer duration in seconds.
    pub fn buffer_duration_secs(&self) -> f64 {
        self.buffer_size as f64 / self.sample_rate as f64
    }

    /// Buffer duration in milliseconds.
    pub fn buffer_duration_ms(&self) -> f64 {
        self.buffer_duration_secs() * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sensible() {
        let config = EngineConfig::default();
        assert_eq!(config.buffer_size, 64);
        assert_eq!(config.sample_rate, 44100);
    }

    #[test]
    fn buffer_duration_calculation() {
        let config = EngineConfig {
            buffer_size: 64,
            sample_rate: 44100,
        };
        let ms = config.buffer_duration_ms();
        assert!((ms - 1.451).abs() < 0.01, "Expected ~1.45ms, got {ms}ms");
    }

    #[test]
    fn buffer_duration_various_sizes() {
        for (size, rate, expected_ms) in [
            (64, 44100, 1.451),
            (128, 44100, 2.902),
            (256, 48000, 5.333),
            (64, 96000, 0.667),
        ] {
            let config = EngineConfig {
                buffer_size: size,
                sample_rate: rate,
            };
            let ms = config.buffer_duration_ms();
            assert!(
                (ms - expected_ms).abs() < 0.01,
                "size={size} rate={rate}: expected {expected_ms}ms, got {ms}ms"
            );
        }
    }
}
