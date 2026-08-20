//! Real audio output via cpal.
//!
//! Creates a high-priority audio thread that calls our callback
//! each buffer cycle. This is the production audio path.
//!
//! The device has the last word on sample rate and block size, and whatever
//! it grants is what the engine must be built from — an engine synthesising
//! at 44100 into a stream running at 48000 plays every note 1.47 semitones
//! sharp and runs the transport 8.8% fast. So the format is resolved here,
//! once, before anything downstream is constructed, and
//! [`CpalBackend::format`] reports it.
//!
//! Nothing is requested unless it was asked for. On CoreAudio, pinning a
//! sample rate changes the machine's nominal rate for every other
//! application as well, and launching a DAW is not consent to that.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig, SupportedBufferSize};
use tracing;

use crate::AudioRequest;

/// The largest block we will pre-allocate for, however large a device claims
/// its blocks may get. A device that then hands the callback more than this
/// gets its buffers grown once and never again; a device that reports a
/// preposterous maximum does not get to reserve a preposterous amount of
/// memory up front.
const MAX_PREALLOC_FRAMES: u32 = 8192;

/// What became of one of the two things the command line can ask for.
///
/// Three states rather than a bool, because "nothing was asked for" and "what
/// was asked for was granted" are both silent but are not the same event, and
/// only the third has anything to tell the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requested {
    /// Nothing was asked for, so the device's own setting stands. The default,
    /// and the only outcome that touches no shared state.
    Unasked,
    /// What was asked for is what the stream runs at.
    Granted,
    /// The device would not take it. Carries the value that was asked for, so
    /// the divergence can be reported without the caller having to hold on to
    /// the original request.
    Refused(u32),
}

/// The stream format the device agreed to, and what became of the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    /// The rate the stream actually runs at. Every oscillator increment,
    /// envelope time and transport advance must be derived from this number.
    pub sample_rate: u32,
    /// The block size pinned on the stream, or `None` when the device was
    /// left to choose. Nominal even when it is `Some`: what the callback is
    /// actually handed varies from block to block and is scaled by any rate
    /// conversion in between. Use it for reporting latency, never for sizing
    /// a buffer.
    pub buffer_size: Option<u32>,
    /// The largest block the callback can be handed. Audio-thread buffers are
    /// sized from this so `process()` never has to grow one.
    pub max_buffer_frames: u32,
    /// Output channel count.
    pub channels: u16,
    /// What became of the sample rate on the command line.
    pub sample_rate_request: Requested,
    /// What became of the block size on the command line.
    pub buffer_size_request: Requested,
}

impl StreamFormat {
    /// What the player needs to be told, or `None` when there is nothing to
    /// tell them.
    ///
    /// Following the device is the ordinary case and says nothing. Only a
    /// refusal is news: they asked for something and are not getting it, and
    /// a session that is silently a semitone and a half sharp is exactly the
    /// failure this whole path exists to prevent.
    #[must_use]
    pub fn divergence_notice(&self) -> Option<String> {
        let rate = match self.sample_rate_request {
            Requested::Refused(asked) => {
                Some(format!("{}Hz (asked for {asked}Hz)", self.sample_rate))
            }
            Requested::Unasked | Requested::Granted => None,
        };
        let block = match (self.buffer_size_request, self.buffer_size) {
            (Requested::Refused(asked), Some(got)) => {
                Some(format!("{got}-frame blocks (asked for {asked})"))
            }
            (Requested::Refused(asked), None) => {
                Some(format!("blocks of its own choosing (asked for {asked})"))
            }
            _ => None,
        };
        match (rate, block) {
            (None, None) => None,
            (Some(one), None) | (None, Some(one)) => Some(format!("audio device gave {one}")),
            (Some(rate), Some(block)) => Some(format!("audio device gave {rate} and {block}")),
        }
    }
}

/// What to ask the device for, given a requested block size and the range it
/// offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BufferChoice {
    /// Frames to pin the stream to, or `None` to leave the choice to the
    /// device — either because nothing was asked for, or because the device
    /// would not name a range to choose from.
    pub request: Option<u32>,
    /// The largest block to pre-allocate for.
    pub max_frames: u32,
    /// What became of the request.
    pub status: Requested,
}

/// Pick a sample rate: the device's own unless one was asked for, then the
/// one asked for if any offered range covers it.
///
/// `offered` is a list of inclusive `(min, max)` rate ranges, already filtered
/// to the channel count and sample format the stream will use — chasing a rate
/// into a different format would break the rest of the pipeline, which is
/// written for interleaved stereo f32.
pub(crate) fn resolve_sample_rate(
    requested: Option<u32>,
    device_default: u32,
    offered: &[(u32, u32)],
) -> (u32, Requested) {
    let Some(requested) = requested else {
        return (device_default, Requested::Unasked);
    };
    if offered.iter().any(|&(min, max)| (min..=max).contains(&requested)) {
        (requested, Requested::Granted)
    } else {
        (device_default, Requested::Refused(requested))
    }
}

/// Pick a block size from the range the device offers.
///
/// With nothing asked for, the device is left to choose and we only work out
/// how much room its choice could need. With something asked for,
/// `BufferSize::Default` is not a neutral answer — it means the requested size
/// is never asked for and the size we get is never known — so a concrete
/// number goes on the stream whenever the device names a range.
///
/// The number asked for is not the number that arrives. Two separate reasons,
/// both measured against CoreAudio rather than assumed:
///
/// * The count is only approximate. Asking a 48000 device for 44100 at 64
///   frames delivers blocks that alternate between 58 and 59.
/// * The device counts in its own frames. Asking that same device for 96000
///   at 64 frames delivers blocks of 128, because each device frame becomes
///   two after conversion. `rate_scale` — the rate we settled on over the
///   device's own — is what corrects for that.
///
/// Which is why `max_frames` is a headroom figure and not the requested size:
/// it is what buffers get allocated to so that no block, whatever its actual
/// length, makes the audio thread call the allocator.
pub(crate) fn resolve_buffer_size(
    requested: Option<u32>,
    offered: Option<(u32, u32)>,
    rate_scale: f64,
) -> BufferChoice {
    // NaN loses against 1.0 here, which is the fallback we want anyway.
    let scale = rate_scale.max(1.0);
    match offered {
        Some((min, max)) if min <= max => {
            let scaled = (f64::from(max) * scale).ceil();
            let scaled = if scaled >= f64::from(u32::MAX) { u32::MAX } else { scaled as u32 };
            let Some(requested) = requested else {
                return BufferChoice {
                    request: None,
                    max_frames: scaled.min(MAX_PREALLOC_FRAMES),
                    status: Requested::Unasked,
                };
            };
            let clamped = requested.clamp(min, max);
            BufferChoice {
                request: Some(clamped),
                max_frames: scaled.clamp(clamped, MAX_PREALLOC_FRAMES.max(clamped)),
                status: if clamped == requested {
                    Requested::Granted
                } else {
                    Requested::Refused(requested)
                },
            }
        }
        // Either the device would not name a range, or it named a nonsense
        // one. Nothing can be pinned to it, so pre-allocate for the worst
        // case we are prepared to absorb — and if a size was asked for, it
        // has been refused, whatever the device goes on to deliver.
        _ => BufferChoice {
            request: None,
            max_frames: MAX_PREALLOC_FRAMES.max(requested.unwrap_or(0)),
            status: requested.map_or(Requested::Unasked, Requested::Refused),
        },
    }
}

/// Real audio backend using cpal.
pub struct CpalBackend {
    stream: Option<Stream>,
    /// Kept so `start()` opens the stream on the same device the format was
    /// resolved against, rather than re-querying and possibly diverging.
    device: Device,
    config: StreamConfig,
    sample_format: SampleFormat,
    format: StreamFormat,
}

impl CpalBackend {
    /// Resolve the output format against the default device. Does NOT start
    /// the stream yet.
    ///
    /// Every field of `request` is a request and none of them is required.
    /// Read [`CpalBackend::format`] afterwards to find out what the device
    /// settled on, and build the engine from that.
    pub fn new(request: AudioRequest) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no audio output device found")?;

        let name = device.name().unwrap_or_else(|_| "unknown".into());
        tracing::info!("Audio device: {name}");

        let default = device.default_output_config()?;
        let channels = default.channels();
        let sample_format = default.sample_format();

        // Only ranges matching the default channel count and sample format are
        // candidates; see `resolve_sample_rate`.
        let candidates: Vec<_> = device
            .supported_output_configs()
            .map(|configs| {
                configs
                    .filter(|c| c.channels() == channels && c.sample_format() == sample_format)
                    .collect()
            })
            .unwrap_or_default();

        let offered_rates: Vec<(u32, u32)> = candidates
            .iter()
            .map(|c| (c.min_sample_rate().0, c.max_sample_rate().0))
            .collect();

        let (sample_rate, sample_rate_request) =
            resolve_sample_rate(request.sample_rate, default.sample_rate().0, &offered_rates);

        // The block-size range that goes with the rate we settled on.
        let offered_buffer = candidates
            .iter()
            .find(|c| (c.min_sample_rate().0..=c.max_sample_rate().0).contains(&sample_rate))
            .map_or_else(|| *default.buffer_size(), |c| *c.buffer_size());
        let offered_buffer = match offered_buffer {
            SupportedBufferSize::Range { min, max } => Some((min, max)),
            SupportedBufferSize::Unknown => None,
        };

        let rate_scale = f64::from(sample_rate) / f64::from(default.sample_rate().0.max(1));
        let buffer = resolve_buffer_size(request.buffer_size, offered_buffer, rate_scale);

        let config = StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: buffer
                .request
                .map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
        };

        let format = StreamFormat {
            sample_rate,
            buffer_size: buffer.request,
            max_buffer_frames: buffer.max_frames,
            channels,
            sample_rate_request,
            buffer_size_request: buffer.status,
        };

        if let Some(notice) = format.divergence_notice() {
            tracing::warn!("{notice}");
        }

        if channels != 2 {
            // Everything past this point — `Mixer::process`, `EngineAudio` —
            // is written for interleaved stereo and divides the block length
            // by two to get the frame count. A device with any other channel
            // count is not handled, only reported.
            tracing::warn!(
                "Audio device reports {channels} channels; the mixer is stereo and \
                 the output will be wrong"
            );
        }

        tracing::info!(
            "Audio config: {}Hz, {} channels, {:?}, buffer {:?} (max {} frames)",
            sample_rate,
            channels,
            sample_format,
            config.buffer_size,
            buffer.max_frames,
        );

        Ok(Self {
            stream: None,
            device,
            config,
            sample_format,
            format,
        })
    }

    /// Start the audio stream, calling `callback` for each buffer.
    /// The callback receives an interleaved f32 buffer: [L, R, L, R, ...]
    pub fn start<F>(&mut self, mut callback: F) -> Result<()>
    where
        F: FnMut(&mut [f32]) + Send + 'static,
    {
        let config = self.config.clone();
        let scratch_len = (self.format.max_buffer_frames as usize) * (self.format.channels as usize);

        let stream = match self.sample_format {
            SampleFormat::F32 => self.device.build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    callback(data);
                },
                |err| tracing::error!("Audio stream error: {err}"),
                None,
            )?,
            SampleFormat::I16 => {
                // Allocated here, not per callback: the conversion buffer used
                // to be a `vec!` inside the closure, which is a heap
                // allocation on the audio thread every single block.
                let mut float_buf = vec![0.0f32; scratch_len];
                self.device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        let n = data.len();
                        if float_buf.len() < n {
                            float_buf.resize(n, 0.0);
                        }
                        let float_buf = &mut float_buf[..n];
                        float_buf.fill(0.0);
                        callback(float_buf);
                        for (out, &inp) in data.iter_mut().zip(float_buf.iter()) {
                            *out = (inp * f32::from(i16::MAX)) as i16;
                        }
                    },
                    |err| tracing::error!("Audio stream error: {err}"),
                    None,
                )?
            }
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

    /// The format the device granted. Build the engine from this, not from
    /// what was requested.
    pub fn format(&self) -> StreamFormat {
        self.format
    }

    pub fn sample_rate(&self) -> u32 {
        self.format.sample_rate
    }

    /// The block size pinned on the stream, `None` when the device chose.
    pub fn buffer_size(&self) -> Option<u32> {
        self.format.buffer_size
    }

    /// The largest block the callback can be handed — the size audio-thread
    /// buffers must be pre-allocated to.
    pub fn max_buffer_frames(&self) -> u32 {
        self.format.max_buffer_frames
    }

    pub fn channels(&self) -> u16 {
        self.format.channels
    }
}

impl Drop for CpalBackend {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These cover the decision, not the driver: no sound card is involved.

    /// The device this was developed against: MacBook Pro Speakers, sitting at
    /// 48000 Hz, offering four discrete rates and blocks of 15..=4096.
    const OFFERED: [(u32, u32); 4] =
        [(44100, 44100), (48000, 48000), (88200, 88200), (96000, 96000)];
    const CORE_AUDIO: Option<(u32, u32)> = Some((15, 4096));

    // ── Sample rate ──

    /// The default, and the reason it is the default: pinning a rate on
    /// CoreAudio changes it for every other application on the machine, and
    /// launching a DAW is not consent to that.
    #[test]
    fn asking_for_nothing_follows_the_device() {
        assert_eq!(
            resolve_sample_rate(None, 48000, &OFFERED),
            (48000, Requested::Unasked)
        );
    }

    #[test]
    fn a_rate_the_device_offers_is_the_rate_we_get() {
        assert_eq!(
            resolve_sample_rate(Some(44100), 48000, &OFFERED),
            (44100, Requested::Granted)
        );
        assert_eq!(
            resolve_sample_rate(Some(96000), 48000, &OFFERED),
            (96000, Requested::Granted)
        );
    }

    /// The defect this whole path exists for: an engine built at a rate the
    /// stream is not running at.
    #[test]
    fn a_rate_the_device_refuses_falls_back_to_the_devices_own() {
        assert_eq!(
            resolve_sample_rate(Some(22050), 48000, &OFFERED),
            (48000, Requested::Refused(22050))
        );
    }

    #[test]
    fn a_device_that_lists_nothing_falls_back_to_its_default() {
        assert_eq!(
            resolve_sample_rate(Some(44100), 48000, &[]),
            (48000, Requested::Refused(44100))
        );
        assert_eq!(resolve_sample_rate(None, 48000, &[]), (48000, Requested::Unasked));
    }

    #[test]
    fn a_continuous_range_covers_the_rates_inside_it() {
        let offered = [(8000, 192_000)];
        assert_eq!(
            resolve_sample_rate(Some(44100), 48000, &offered),
            (44100, Requested::Granted)
        );
        assert_eq!(
            resolve_sample_rate(Some(300_000), 48000, &offered),
            (48000, Requested::Refused(300_000))
        );
    }

    // ── Block size ──

    #[test]
    fn asking_for_no_block_size_leaves_the_choice_to_the_device() {
        let choice = resolve_buffer_size(None, CORE_AUDIO, 1.0);
        assert_eq!(choice.request, None);
        assert_eq!(choice.status, Requested::Unasked);
        // Still has to be able to absorb whatever the device picks.
        assert_eq!(choice.max_frames, 4096);
    }

    #[test]
    fn a_block_size_in_range_is_asked_for_by_name() {
        let choice = resolve_buffer_size(Some(64), CORE_AUDIO, 1.0);
        assert_eq!(choice.request, Some(64));
        assert_eq!(choice.status, Requested::Granted);
    }

    #[test]
    fn a_block_size_out_of_range_is_clamped_and_reported() {
        let low = resolve_buffer_size(Some(4), CORE_AUDIO, 1.0);
        assert_eq!(low.request, Some(15));
        assert_eq!(low.status, Requested::Refused(4));

        let high = resolve_buffer_size(Some(99_999), CORE_AUDIO, 1.0);
        assert_eq!(high.request, Some(4096));
        assert_eq!(high.status, Requested::Refused(99_999));
    }

    #[test]
    fn we_pre_allocate_for_the_largest_block_the_device_admits_to() {
        // Asking for 64 does not mean 64 is all that can arrive: the device
        // said it may deliver up to 4096, so that is what the audio thread
        // has to be able to take without touching the allocator.
        let choice = resolve_buffer_size(Some(64), CORE_AUDIO, 1.0);
        assert_eq!(choice.max_frames, 4096);
    }

    /// Measured: asking a 48000 device for 96000 at 64 frames delivers blocks
    /// of 128. The device counts its own frames; we are handed the converted
    /// ones, and have to have room for them.
    #[test]
    fn a_rate_above_the_devices_own_widens_the_pre_allocation() {
        let choice = resolve_buffer_size(Some(64), CORE_AUDIO, 96_000.0 / 48_000.0);
        assert_eq!(choice.request, Some(64));
        assert_eq!(choice.status, Requested::Granted);
        assert_eq!(choice.max_frames, 8192);
    }

    /// A rate below the device's own gives shorter blocks, not longer ones —
    /// no reason to reserve less than the device's stated maximum for it.
    #[test]
    fn a_rate_below_the_devices_own_does_not_shrink_the_pre_allocation() {
        let choice = resolve_buffer_size(Some(64), CORE_AUDIO, 44_100.0 / 48_000.0);
        assert_eq!(choice.max_frames, 4096);
    }

    #[test]
    fn a_device_that_names_no_range_still_gets_a_bounded_pre_allocation() {
        let unasked = resolve_buffer_size(None, None, 1.0);
        assert_eq!(unasked.request, None);
        assert_eq!(unasked.status, Requested::Unasked);
        assert_eq!(unasked.max_frames, MAX_PREALLOC_FRAMES);

        // Nothing can be pinned to a range that was never named, so a size
        // that was asked for has been refused.
        let asked = resolve_buffer_size(Some(64), None, 1.0);
        assert_eq!(asked.request, None);
        assert_eq!(asked.status, Requested::Refused(64));
    }

    #[test]
    fn a_preposterous_maximum_does_not_become_a_preposterous_allocation() {
        let choice = resolve_buffer_size(Some(64), Some((15, u32::MAX)), 4.0);
        assert_eq!(choice.request, Some(64));
        assert_eq!(choice.max_frames, MAX_PREALLOC_FRAMES);

        let unasked = resolve_buffer_size(None, Some((15, u32::MAX)), 4.0);
        assert_eq!(unasked.max_frames, MAX_PREALLOC_FRAMES);
    }

    #[test]
    fn a_nonsense_rate_scale_falls_back_to_no_scaling() {
        for scale in [f64::NAN, 0.0, -1.0, f64::NEG_INFINITY] {
            let choice = resolve_buffer_size(Some(64), CORE_AUDIO, scale);
            assert_eq!(choice.max_frames, 4096, "scale {scale}");
        }
    }

    #[test]
    fn the_pre_allocation_is_never_smaller_than_the_block_we_asked_for() {
        // A device demanding blocks larger than our own cap still gets buffers
        // big enough for them.
        let choice = resolve_buffer_size(Some(16384), Some((16384, 16384)), 1.0);
        assert_eq!(choice.request, Some(16384));
        assert!(choice.max_frames >= 16384);
    }

    #[test]
    fn a_nonsense_range_is_treated_as_no_range() {
        let choice = resolve_buffer_size(Some(64), Some((4096, 15)), 1.0);
        assert_eq!(choice.request, None);
        assert_eq!(choice.status, Requested::Refused(64));
    }

    // ── What the player is told ──

    fn format(rate: Requested, block: Requested, pinned: Option<u32>) -> StreamFormat {
        StreamFormat {
            sample_rate: 48000,
            buffer_size: pinned,
            max_buffer_frames: 4096,
            channels: 2,
            sample_rate_request: rate,
            buffer_size_request: block,
        }
    }

    /// Following the device is the ordinary case and is not news. The old
    /// wording would have printed "gave 48000Hz (asked for 48000Hz)" on every
    /// launch, which trains people to ignore the line that matters.
    #[test]
    fn following_the_device_says_nothing() {
        assert!(format(Requested::Unasked, Requested::Unasked, None)
            .divergence_notice()
            .is_none());
    }

    #[test]
    fn getting_what_was_asked_for_says_nothing() {
        assert!(format(Requested::Granted, Requested::Granted, Some(64))
            .divergence_notice()
            .is_none());
    }

    #[test]
    fn a_refused_rate_is_reported_with_both_numbers() {
        let notice = format(Requested::Refused(22050), Requested::Granted, Some(64))
            .divergence_notice()
            .expect("a silent divergence is the bug");
        assert!(notice.contains("48000"), "{notice}");
        assert!(notice.contains("22050"), "{notice}");
        assert!(!notice.contains("frame"), "no block size was refused: {notice}");
    }

    #[test]
    fn a_refused_block_size_is_reported_on_its_own() {
        let notice = format(Requested::Unasked, Requested::Refused(4), Some(15))
            .divergence_notice()
            .expect("a clamp the player did not ask for is news");
        assert!(notice.contains("15-frame"), "{notice}");
        assert!(notice.contains("asked for 4"), "{notice}");
        assert!(!notice.contains("Hz"), "the rate was never asked for: {notice}");
    }

    #[test]
    fn two_refusals_are_reported_together() {
        let notice = format(Requested::Refused(22050), Requested::Refused(4), Some(15))
            .divergence_notice()
            .unwrap();
        assert!(notice.contains("22050") && notice.contains("asked for 4"), "{notice}");
    }

    #[test]
    fn a_block_size_refused_outright_still_reads_sensibly() {
        let notice = format(Requested::Unasked, Requested::Refused(64), None)
            .divergence_notice()
            .unwrap();
        assert!(notice.contains("asked for 64"), "{notice}");
    }
}
