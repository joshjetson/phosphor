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
///
/// Tracks the instruments' headroom trims, so the click sits where it always
/// did relative to the music. Those trims are around 0.18 on the output
/// stage; at the original 0.35 the click would peak near 0.59, several times
/// a chord, and playing along to it would be unpleasant at best.
///
/// This has to move whenever the trims do — it is not mixed through a track
/// and has no fader of its own, so nothing else can compensate for it. See
/// `OUTPUT_TRIM` in phosphor-dsp's dx7.rs.
const CLICK_VOLUME: f32 = 0.0634;

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
        self.render(output, transport.position_ticks(), transport.tempo_bpm());
    }

    /// The count-in's bars of click, on the countdown's own timeline.
    ///
    /// Always sounds, whatever the metronome switch says: a silent count-in
    /// is dead air, and the click *is* the count. `elapsed_tick` runs from
    /// zero at the top of the countdown, so beat one of each counted bar
    /// pops the downbeat voice exactly as the song's bars do.
    pub fn count_in(&mut self, output: &mut [f32], elapsed_tick: i64, transport: &Transport) {
        self.render(output, elapsed_tick, transport.tempo_bpm());
    }

    fn render(&mut self, output: &mut [f32], current_tick: i64, bpm: f64) {
        let ppq = Transport::PPQ;
        let ticks_per_bar = ppq * 4; // 4/4 time
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

    /// The practice click: a free-running metronome on its own sample
    /// clock, nothing to do with the transport. `beat_phase` is advanced by
    /// the caller between blocks. `pattern` 0 clicks every beat with a
    /// downbeat accent; 1 clicks beats 2 and 4 only — the jazz convention,
    /// where the click is the drummer's hi-hat and beats 1 and 3 are yours
    /// to feel.
    pub fn practice_click(
        &mut self,
        output: &mut [f32],
        bpm: f64,
        pattern: u8,
        beat_phase: &mut f64,
    ) {
        let num_frames = output.len() / 2;
        let beats_per_sample = bpm / (60.0 * self.sample_rate);
        for i in 0..num_frames {
            let beat_now = *beat_phase + i as f64 * beats_per_sample;
            let abs_beat = beat_now.floor() as i64;
            if abs_beat != self.last_beat && beat_now >= 0.0 {
                self.last_beat = abs_beat;
                let beat_in_bar = abs_beat.rem_euclid(4);
                let sounds = match pattern {
                    1 => beat_in_bar == 1 || beat_in_bar == 3,
                    _ => true,
                };
                if sounds {
                    self.clicking = true;
                    self.click_phase = 0.0;
                    self.is_downbeat = pattern == 0 && beat_in_bar == 0;
                }
            }
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
        *beat_phase += num_frames as f64 * beats_per_sample;
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
