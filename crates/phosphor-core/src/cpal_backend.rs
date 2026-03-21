//! Real audio output via cpal.
//!
//! Creates a high-priority audio thread that calls our callback
//! each buffer cycle. This is the production audio path.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use tracing;

/// Real audio backend using cpal.
pub struct CpalBackend {
    stream: Option<Stream>,
    sample_rate: u32,
    channels: u16,
}

impl CpalBackend {
    /// Create a new cpal backend. Does NOT start the stream yet.
    pub fn new(_desired_sample_rate: u32, _desired_buffer_size: u32) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no audio output device found")?;

        let name = device.name().unwrap_or_else(|_| "unknown".into());
        tracing::info!("Audio device: {name}");

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        tracing::info!(
            "Audio config: {}Hz, {} channels, {:?}",
            sample_rate, channels, config.sample_format()
        );

        Ok(Self {
            stream: None,
            sample_rate,
            channels,
        })
    }

    /// Start the audio stream, calling `callback` for each buffer.
    /// The callback receives an interleaved f32 buffer: [L, R, L, R, ...]
    pub fn start<F>(&mut self, mut callback: F) -> Result<()>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no audio output device found")?;

        let supported = device.default_output_config()?;

        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        self.sample_rate = config.sample_rate.0;
        self.channels = config.channels;

        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    callback(data);
                },
                |err| tracing::error!("Audio stream error: {err}"),
                None,
            )?,
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    // Convert: call callback with f32 buffer, then convert to i16
                    let mut float_buf = vec![0.0f32; data.len()];
                    callback(&mut float_buf);
                    for (out, &inp) in data.iter_mut().zip(float_buf.iter()) {
                        *out = (inp * i16::MAX as f32) as i16;
                    }
                },
                |err| tracing::error!("Audio stream error: {err}"),
                None,
            )?,
            format => anyhow::bail!("Unsupported sample format: {format:?}"),
        };

        stream.play()?;
        tracing::info!("Audio stream started");
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(stream) = self.stream.take() {
            drop(stream);
            tracing::info!("Audio stream stopped");
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
}

impl Drop for CpalBackend {
    fn drop(&mut self) {
        self.stop();
    }
}
