//! WAV decoding for the sampler — the only place in the tree a sample
//! file is opened.
//!
//! Everything lands as interleaved `f32` at the file's *native* rate; the
//! engine owns rate conversion, because it is the engine that knows what
//! rate the device is running at today. Files with more than two channels
//! keep their first two — a surround stem loaded into a drum pad wants
//! its front pair, not an error.

use std::path::Path;
use std::sync::Arc;

use phosphor_plugin::sample::SamplePcm;

/// Longest file accepted, in frames — ten minutes of 48 kHz. A sampler
/// pad is a sound, not an album side, and a decode this large would sit
/// on the UI thread while it runs.
///
/// Public because a rendered take has to respect it too: a resample long
/// enough to be refused by its own loader would vanish on the next
/// session load. See [`crate::sampler::render`].
pub const MAX_FRAMES: u32 = 48_000 * 60 * 10;

/// Decode `path` into PCM. The error is a sentence for the status bar,
/// not a code: the player reads it where they typed the path.
pub fn load_wav(path: &Path) -> Result<Arc<SamplePcm>, String> {
    let shown = path.display();
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("{shown}: {}", reason(e)))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(format!("{shown}: no channels"));
    }
    if reader.duration() > MAX_FRAMES {
        return Err(format!("{shown}: over ten minutes — too long for a pad"));
    }

    let channels = usize::from(spec.channels);
    let keep = channels.min(2);
    let frames = reader.duration() as usize;
    let mut data = Vec::with_capacity(frames * keep);

    // hound yields samples interleaved; fold every frame down to the
    // first two channels. The scale for ints is 2^(bits-1): 16-bit says
    // 32768, 24-bit says 8388608, and a full-scale negative sample maps
    // to exactly -1.0.
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for (i, s) in reader.samples::<f32>().enumerate() {
                let s = s.map_err(|e| format!("{shown}: {}", reason(e)))?;
                if i % channels < keep {
                    // A float file can carry NaN or infinity, and one such
                    // sample is enough to latch the engine's DC blocker and
                    // silence the whole track until reset. Audio that is
                    // not a number is silence, decided here at the door.
                    data.push(if s.is_finite() { s } else { 0.0 });
                }
            }
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            for (i, s) in reader.samples::<i32>().enumerate() {
                let s = s.map_err(|e| format!("{shown}: {}", reason(e)))?;
                if i % channels < keep {
                    data.push(s as f32 * scale);
                }
            }
        }
    }

    if data.is_empty() {
        return Err(format!("{shown}: no audio in the file"));
    }
    Ok(Arc::new(SamplePcm {
        data,
        channels: keep as u16,
        sample_rate: spec.sample_rate as f32,
    }))
}

/// hound's errors in the player's language.
fn reason(e: hound::Error) -> String {
    match e {
        hound::Error::IoError(io) if io.kind() == std::io::ErrorKind::NotFound => {
            "not found".into()
        }
        hound::Error::FormatError(_) => "not a WAV file".into(),
        hound::Error::Unsupported => "a WAV flavour this build cannot read".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("phosphor_wav_tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn write_wav(name: &str, spec: hound::WavSpec, write: impl Fn(&mut hound::WavWriter<std::io::BufWriter<std::fs::File>>)) -> std::path::PathBuf {
        let path = tmp(name);
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        write(&mut w);
        w.finalize().unwrap();
        path
    }

    #[test]
    fn sixteen_bit_mono_lands_at_the_right_scale() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav("m16.wav", spec, |w| {
            w.write_sample(i16::MAX as i32).unwrap();
            w.write_sample(i16::MIN as i32).unwrap();
            w.write_sample(0).unwrap();
        });
        let pcm = load_wav(&path).unwrap();
        assert_eq!(pcm.channels, 1);
        assert_eq!(pcm.frames(), 3);
        assert!((pcm.data[0] - (32_767.0 / 32_768.0)).abs() < 1e-6);
        assert_eq!(pcm.data[1], -1.0);
        assert_eq!(pcm.data[2], 0.0);
    }

    #[test]
    fn a_48k_stereo_float_file_keeps_its_rate_and_channels() {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let path = write_wav("s48.wav", spec, |w| {
            for i in 0..10 {
                w.write_sample(i as f32 * 0.1).unwrap(); // left
                w.write_sample(-0.5f32).unwrap(); // right
            }
        });
        let pcm = load_wav(&path).unwrap();
        assert_eq!(pcm.channels, 2);
        assert_eq!(pcm.sample_rate, 48_000.0);
        assert_eq!(pcm.frames(), 10);
        assert!((pcm.data[2] - 0.1).abs() < 1e-6); // frame 1 left
        assert_eq!(pcm.data[3], -0.5); // frame 1 right
    }

    #[test]
    fn twenty_four_bit_uses_its_own_scale() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav("m24.wav", spec, |w| {
            w.write_sample(-(1 << 23)).unwrap(); // full-scale negative
            w.write_sample((1 << 23) - 1).unwrap();
        });
        let pcm = load_wav(&path).unwrap();
        assert_eq!(pcm.data[0], -1.0);
        assert!((pcm.data[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_four_channel_file_keeps_its_front_pair() {
        let spec = hound::WavSpec {
            channels: 4,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav("quad.wav", spec, |w| {
            for frame in 0..3i32 {
                for ch in 0..4 {
                    w.write_sample(frame * 4 + ch).unwrap();
                }
            }
        });
        let pcm = load_wav(&path).unwrap();
        assert_eq!(pcm.channels, 2);
        assert_eq!(pcm.frames(), 3);
        // Frame 1 kept channels 0 and 1 (raw values 4 and 5), not 2 and 3.
        let scale = 1.0 / 32_768.0;
        assert!((pcm.data[2] - 4.0 * scale).abs() < 1e-9);
        assert!((pcm.data[3] - 5.0 * scale).abs() < 1e-9);
    }

    #[test]
    fn a_missing_file_reads_as_a_sentence() {
        let err = load_wav(Path::new("/nowhere/kick.wav")).unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn a_file_that_is_not_wav_says_so() {
        let path = tmp("not_audio.wav");
        std::fs::write(&path, b"this is a text file wearing a wav extension").unwrap();
        let err = load_wav(&path).unwrap_err();
        assert!(err.contains("not a WAV"), "{err}");
    }

    #[test]
    fn a_nan_in_a_float_file_becomes_silence_at_the_door() {
        // One non-finite sample used to latch the engine's DC blocker and
        // silence the whole track until reset. Audio that is not a number
        // is decided here, once, before the engine ever sees it.
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let path = write_wav("nan.wav", spec, |w| {
            w.write_sample(0.5f32).unwrap();
            w.write_sample(f32::NAN).unwrap();
            w.write_sample(f32::INFINITY).unwrap();
            w.write_sample(f32::NEG_INFINITY).unwrap();
            w.write_sample(-0.25f32).unwrap();
        });
        let pcm = load_wav(&path).unwrap();
        assert!(pcm.data.iter().all(|s| s.is_finite()), "a non-finite sample got through");
        assert_eq!(pcm.data[0], 0.5);
        assert_eq!(pcm.data[1], 0.0);
        assert_eq!(pcm.data[2], 0.0);
        assert_eq!(pcm.data[3], 0.0);
        assert_eq!(pcm.data[4], -0.25);
    }

    #[test]
    fn an_empty_wav_is_refused() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let path = write_wav("empty.wav", spec, |_| {});
        let err = load_wav(&path).unwrap_err();
        assert!(err.contains("no audio"), "{err}");
    }
}
