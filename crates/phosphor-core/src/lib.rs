pub mod audio;
pub mod clip;
pub mod cpal_backend;
pub mod engine;
pub mod metronome;
pub mod mixer;
pub mod pattern;
pub mod project;
pub mod transport;

use serde::{Deserialize, Serialize};

// ── An allocation counter for the audio path ──
//
// The same device as `phosphor_dsp::synth::tests::allocations_during`, and for
// the same reason: "the callback never calls the allocator" is a property of
// the code rather than of its output, so no test that only reads the output
// can catch a breach of it. A global allocator has to be installed per test
// binary, which is why this exists here as well as there.
//
// Counted per thread rather than globally, because cargo runs tests in
// parallel and a global count would see every other test's work; the
// thread-local is declared with `const` so that reading it cannot itself
// allocate, and `try_with` is used so that an allocation during thread
// teardown cannot panic inside the allocator.
#[cfg(test)]
pub(crate) mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    }

    struct Counting;

    fn note_allocation() {
        let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
    }

    // SAFETY: every method forwards to the system allocator with the same
    // pointer and layout it was given, so the allocator's contract is the
    // system allocator's contract. The counter is a thread-local `Cell` of a
    // plain integer, which allocates nothing and cannot re-enter.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            note_allocation();
            System.alloc(layout)
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout);
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            note_allocation();
            System.alloc_zeroed(layout)
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            note_allocation();
            System.realloc(ptr, layout, new_size)
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// How many times the allocator was reached on this thread while `body`
    /// ran.
    pub(crate) fn allocations_during(body: impl FnOnce()) -> u64 {
        let before = ALLOCATIONS.with(Cell::get);
        body();
        ALLOCATIONS.with(Cell::get) - before
    }
}

/// Configuration for the audio engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Audio buffer size in samples. Lower = less latency, more CPU.
    /// Typical values: 32, 64, 128, 256, 512.
    pub buffer_size: u32,
    /// Sample rate in Hz. Typical values: 44100, 48000, 96000.
    pub sample_rate: u32,
}

impl Default for EngineConfig {
    /// The numbers to run at when there is no device to ask — `--no-audio`,
    /// or a backend that would not open.
    ///
    /// Deliberately not the general-purpose starting point it looks like. As
    /// soon as a device is open, the config comes from the device via
    /// `From<StreamFormat>`; reaching for this instead is how an engine ends
    /// up at a rate the stream is not running at. It is here for the case
    /// where nothing is listening, and in that case the numbers only have to
    /// be self-consistent.
    fn default() -> Self {
        Self {
            buffer_size: 64,
            sample_rate: 44100,
        }
    }
}

/// What the command line asked for. Both halves optional, and unspecified is
/// the ordinary case.
///
/// Separate from [`EngineConfig`] because they are different facts: this is a
/// request, that is what the engine runs at. Collapsing the two is what let an
/// engine synthesising at 44100 feed a stream running at 48000 — every note
/// 1.47 semitones sharp, 120 BPM playing back at 130.6.
///
/// `None` means follow the device. That is the default because on CoreAudio
/// pinning a sample rate changes the machine's nominal rate for every other
/// application too, and opening a DAW is not consent to that.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioRequest {
    /// Sample rate in Hz, or `None` to run at whatever the device is set to.
    pub sample_rate: Option<u32>,
    /// Block size in samples, or `None` to let the device choose.
    pub buffer_size: Option<u32>,
}

impl AudioRequest {
    /// Ask for nothing and take what the device is already doing.
    #[must_use]
    pub const fn follow_device() -> Self {
        Self { sample_rate: None, buffer_size: None }
    }

    /// The config to run at when there is no device to follow. Anything the
    /// command line named is honoured; the rest comes from
    /// [`EngineConfig::default`].
    #[must_use]
    pub fn without_device(self) -> EngineConfig {
        let fallback = EngineConfig::default();
        EngineConfig {
            sample_rate: self.sample_rate.unwrap_or(fallback.sample_rate),
            buffer_size: self.buffer_size.unwrap_or(fallback.buffer_size),
        }
    }
}

impl From<EngineConfig> for AudioRequest {
    /// "Run at exactly this" as a request. Used by callers that already hold
    /// concrete numbers — the headless test apps, which open no device at all.
    fn from(config: EngineConfig) -> Self {
        Self {
            sample_rate: Some(config.sample_rate),
            buffer_size: Some(config.buffer_size),
        }
    }
}

impl From<crate::cpal_backend::StreamFormat> for EngineConfig {
    /// The config the engine must be built from once a device has been opened.
    ///
    /// `buffer_size` falls back to the largest block the device may deliver
    /// when the device was left to choose its own: nothing sizes a buffer from
    /// this field any more — the mixer takes `max_buffer_frames` directly — so
    /// the honest value for a block size we were never told is the worst case
    /// rather than a guess.
    fn from(format: crate::cpal_backend::StreamFormat) -> Self {
        Self {
            buffer_size: format.buffer_size.unwrap_or(format.max_buffer_frames),
            sample_rate: format.sample_rate,
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
    fn an_empty_request_asks_for_nothing() {
        let request = AudioRequest::follow_device();
        assert_eq!(request.sample_rate, None);
        assert_eq!(request.buffer_size, None);
        assert_eq!(request, AudioRequest::default());
    }

    /// `--no-audio` has no device to follow, so it needs concrete numbers.
    #[test]
    fn without_a_device_the_gaps_are_filled_from_the_default() {
        assert_eq!(
            AudioRequest::follow_device().without_device(),
            EngineConfig::default()
        );
    }

    /// ...but anything the command line did name still stands, device or no
    /// device.
    #[test]
    fn without_a_device_what_was_asked_for_is_still_honoured() {
        let request = AudioRequest { sample_rate: Some(96000), buffer_size: None };
        let config = request.without_device();
        assert_eq!(config.sample_rate, 96000);
        assert_eq!(config.buffer_size, EngineConfig::default().buffer_size);
    }

    #[test]
    fn a_concrete_config_converts_to_a_request_for_exactly_it() {
        let config = EngineConfig { buffer_size: 256, sample_rate: 96000 };
        assert_eq!(
            AudioRequest::from(config),
            AudioRequest { sample_rate: Some(96000), buffer_size: Some(256) }
        );
        assert_eq!(AudioRequest::from(config).without_device(), config);
    }

    /// A block size the device was never pinned to has no honest nominal
    /// value, so the worst case stands in for it.
    #[test]
    fn a_device_chosen_block_size_reports_the_worst_case() {
        use crate::cpal_backend::{Requested, StreamFormat};
        let format = StreamFormat {
            sample_rate: 48000,
            buffer_size: None,
            max_buffer_frames: 4096,
            channels: 2,
            sample_rate_request: Requested::Unasked,
            buffer_size_request: Requested::Unasked,
        };
        let config = EngineConfig::from(format);
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.buffer_size, 4096);
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
