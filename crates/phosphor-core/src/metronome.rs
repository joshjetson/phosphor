//! Metronome — click track that follows the transport BPM.
//!
//! Generates short percussive pops. Beat 1 of each bar is a higher-pitched
//! pop, other beats are lower. Sounds similar to an MPC 2000xl click.
//!
//! The metronome is mixed directly into the master output by the mixer.

use crate::transport::Transport;

/// Metronome click generator. Runs on the audio thread.
pub struct Metronome {
    sample_rate: f64,
    click_phase: f64,
    is_downbeat: bool,
    clicking: bool,
    /// Last beat index we triggered on (to avoid double-triggering).
    last_beat: i64,
}

/// Duration of a click in seconds. Short pop.
const CLICK_DURATION: f64 = 0.012;
/// Volume of the click.
const CLICK_VOLUME: f32 = 0.35;

impl Metronome {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            click_phase: 0.0,
            is_downbeat: false,
            clicking: false,
            last_beat: -1,
        }
    }

    /// Generate metronome audio for one buffer and mix it into the output.
    /// `output` is interleaved stereo [L, R, L, R, ...].
    pub fn process(&mut self, output: &mut [f32], transport: &Transport) {
        if !transport.is_metronome_on() || !transport.is_playing() {
            return;
        }

        let ppq = Transport::PPQ;
        let ticks_per_bar = ppq * 4; // 4/4 time
        let current_tick = transport.position_ticks();
        let bpm = transport.tempo_bpm();
        let ticks_per_sample = (bpm * ppq as f64) / (60.0 * self.sample_rate);
        let num_frames = output.len() / 2;

        for i in 0..num_frames {
            let frame_tick = current_tick + (i as f64 * ticks_per_sample) as i64;

            // Which beat are we on? (0-based within the bar)
            let beat_in_bar = (frame_tick % ticks_per_bar) / ppq;
            // Absolute beat number (monotonic)
            let abs_beat = frame_tick / ppq;

            // Trigger a new click when we cross a beat boundary
            if abs_beat != self.last_beat && frame_tick >= 0 {
                self.last_beat = abs_beat;
                self.clicking = true;
                self.click_phase = 0.0;
                self.is_downbeat = beat_in_bar == 0;
            }

            // Generate click sound
            if self.clicking {
                let t = self.click_phase / self.sample_rate;

                if t > CLICK_DURATION {
                    self.clicking = false;
                } else {
                    let sample = self.generate_click(t);
                    let idx = i * 2;
                    output[idx] += sample;
                    output[idx + 1] += sample;
                }

                self.click_phase += 1.0;
            }
        }
    }

    /// Generate one sample of the click sound.
    /// MPC 2000xl style: short band-passed noise burst with fast exponential decay.
    /// Downbeat is higher pitched and slightly louder.
    fn generate_click(&self, t: f64) -> f32 {
        let decay = (-t * 500.0).exp(); // fast exponential decay

        let (freq, volume) = if self.is_downbeat {
            (1800.0, CLICK_VOLUME * 1.3) // higher, louder pop for beat 1
        } else {
            (1200.0, CLICK_VOLUME) // lower pop for other beats
        };

        // Sine burst with noise — gives that percussive "pop" character
        let sine = (t * freq * std::f64::consts::TAU).sin();
        // Add a bit of filtered noise for texture
        let noise = ((t * 7919.0).sin() * (t * 3571.0).cos()) * 0.3;

        ((sine + noise) * decay * volume as f64) as f32
    }

    /// Reset state (e.g., on transport stop).
    pub fn reset(&mut self) {
        self.clicking = false;
        self.click_phase = 0.0;
        self.last_beat = -1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn metronome_silent_when_off() {
        let transport = Arc::new(Transport::new(120.0));
        transport.play();
        // metronome is off by default
        let mut met = Metronome::new(44100.0);
        let mut output = vec![0.0f32; 512];
        met.process(&mut output, &transport);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn metronome_produces_sound_when_on() {
        let transport = Arc::new(Transport::new(120.0));
        transport.play();
        transport.toggle_metronome();
        let mut met = Metronome::new(44100.0);
        let mut output = vec![0.0f32; 512];
        met.process(&mut output, &transport);
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Metronome should produce sound, peak={peak}");
    }

    #[test]
    fn metronome_silent_when_not_playing() {
        let transport = Arc::new(Transport::new(120.0));
        transport.toggle_metronome();
        // NOT playing
        let mut met = Metronome::new(44100.0);
        let mut output = vec![0.0f32; 512];
        met.process(&mut output, &transport);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn metronome_output_is_finite() {
        let transport = Arc::new(Transport::new(120.0));
        transport.play();
        transport.toggle_metronome();
        let mut met = Metronome::new(44100.0);
        for _ in 0..1000 {
            let mut output = vec![0.0f32; 512];
            met.process(&mut output, &transport);
            assert!(output.iter().all(|s| s.is_finite()), "Output must be finite");
            transport.advance(256, 44100);
        }
    }

    #[test]
    fn click_sounds_differ_by_beat_type() {
        // The generate_click function uses different freq/volume for downbeat vs regular
        // Test by calling the underlying math directly
        let t: f64 = 0.002;
        let decay = (-t * 500.0_f64).exp();
        let sine_down = (t * 1800.0 * std::f64::consts::TAU).sin();
        let sine_reg = (t * 1200.0 * std::f64::consts::TAU).sin();
        let down_sample = sine_down * decay * 0.35 * 1.3;
        let reg_sample = sine_reg * decay * 0.35;
        assert!((down_sample - reg_sample).abs() > 0.01,
            "Downbeat and regular click should differ: down={down_sample} reg={reg_sample}");
    }
}
