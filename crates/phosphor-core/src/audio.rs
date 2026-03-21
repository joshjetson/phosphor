//! Audio I/O backend abstraction.
//!
//! The real backend uses cpal for hardware audio. The test backend captures
//! output to a `Vec<f32>` so tests run without a sound card.

use anyhow::Result;

/// A processed audio buffer returned by the test backend.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}

/// Audio callback type alias.
pub type AudioCallback = Box<dyn FnMut(&mut [f32]) + Send>;

/// Trait for audio output backends. Allows swapping real hardware for
/// an in-memory test backend.
pub trait AudioBackend: Send {
    /// Start the audio stream, calling `callback` for each buffer.
    fn start(&mut self, callback: AudioCallback) -> Result<()>;
    /// Stop the audio stream.
    fn stop(&mut self) -> Result<()>;
    /// Sample rate the backend is running at.
    fn sample_rate(&self) -> u32;
    /// Buffer size in samples per channel.
    fn buffer_size(&self) -> u32;
    /// Number of output channels.
    fn channels(&self) -> u16;
}

/// In-memory audio backend for testing. No sound card required.
pub struct TestBackend {
    sample_rate: u32,
    buffer_size: u32,
    channels: u16,
    captured: Vec<f32>,
}

impl TestBackend {
    pub fn new(sample_rate: u32, buffer_size: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            buffer_size,
            channels,
            captured: Vec::new(),
        }
    }

    /// Run the callback for `num_buffers` cycles and capture the output.
    pub fn process_blocks(
        &mut self,
        num_buffers: usize,
        mut callback: impl FnMut(&mut [f32]),
    ) -> AudioBuffer {
        let block_size = self.buffer_size as usize * self.channels as usize;
        self.captured.clear();
        self.captured.reserve(block_size * num_buffers);

        for _ in 0..num_buffers {
            let start = self.captured.len();
            self.captured.resize(start + block_size, 0.0);
            callback(&mut self.captured[start..]);
        }

        AudioBuffer {
            samples: self.captured.clone(),
            channels: self.channels,
            sample_rate: self.sample_rate,
        }
    }
}

impl AudioBackend for TestBackend {
    fn start(&mut self, _callback: Box<dyn FnMut(&mut [f32]) + Send>) -> Result<()> {
        // TestBackend uses process_blocks() directly instead
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn buffer_size(&self) -> u32 {
        self.buffer_size
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_captures_silence() {
        let mut backend = TestBackend::new(44100, 64, 2);
        let result = backend.process_blocks(10, |_buf| {
            // callback does nothing — buffer stays zeroed
        });
        assert_eq!(result.samples.len(), 64 * 2 * 10);
        assert!(result.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_backend_captures_signal() {
        let mut backend = TestBackend::new(44100, 64, 1);
        let mut phase = 0.0f32;
        let result = backend.process_blocks(10, |buf| {
            for sample in buf.iter_mut() {
                *sample = phase.sin();
                phase += 440.0 * std::f32::consts::TAU / 44100.0;
            }
        });
        assert_eq!(result.samples.len(), 64 * 10);
        // Should contain non-zero samples
        assert!(result.samples.iter().any(|&s| s.abs() > 0.1));
        // All samples must be finite
        assert!(result.samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn test_backend_correct_metadata() {
        let backend = TestBackend::new(48000, 128, 2);
        assert_eq!(backend.sample_rate(), 48000);
        assert_eq!(backend.buffer_size(), 128);
        assert_eq!(backend.channels(), 2);
    }
}
