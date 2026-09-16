//! The audition: one layer, sounded on its own for the UI to listen to.
//!
//! Every voice in the pool belongs to a *hit* — a key was pressed, a pad
//! answered, poly and choke had their say. A preview is none of that. It is
//! the player moving a trim marker and wanting to hear where they put it, so
//! it gets voices of its own, outside the pool, outside poly, outside choke
//! and outside the global cap. It can neither steal from the kit nor be
//! stolen from, which is what makes an audition during a busy pattern
//! actually arrive.
//!
//! # The envelope is left behind on purpose
//!
//! The pad's pitch, level and pan travel with the audition because they are
//! part of what the sound *is*. Its ADSR does not. A pad set to a 50 ms
//! decay would answer every trim nudge with a tick, which tells a player
//! nothing about where the marker landed — and the marker is the only thing
//! they are listening for. So the envelope is flattened to a wire, the
//! trigger forced to one-shot, and keytracking turned off: what comes back
//! is the audio between the two markers and nothing else.
//!
//! # The seam
//!
//! A loop does not rewind a voice; it starts the next lap in another slot
//! while the one finishing still has its closing edge fade to run. The two
//! ramps cross, so the join is a lap rather than a fade to silence and back.
//! The voice already knows how to fade both its edges — this only has to
//! decide when to light the next one.

use phosphor_plugin::sample::{PadConfig, PreviewLayer, PreviewMode, TrigMode};

use super::pad::LayerSlot;
use super::voice::SamplerVoice;

/// Audition slots: the one playing, the one lapping it across a loop seam,
/// and one more so that a retrigger always has somewhere to start while the
/// pair it replaced is still fading out.
pub(super) const PREVIEW_VOICES: usize = 3;

/// The note a preview fires on. Keytracking is off for an audition, so this
/// only ever names the pad's own root and never transposes anything — but a
/// voice needs a note, and passing the root makes the distance zero whatever
/// a future caller does with `keytrack`.
fn preview_note(config: &PadConfig) -> u8 {
    config.root
}

/// The pad's sound with the pad's envelope taken out. See the module note.
fn flattened(config: &PadConfig) -> PadConfig {
    PadConfig {
        trig: TrigMode::OneShot,
        attack_ms: 0.0,
        decay_ms: 0.0,
        sustain: 1.0,
        keytrack: false,
        ..*config
    }
}

/// Everything the sampler holds on behalf of the audition.
pub(super) struct Preview {
    voices: [SamplerVoice; PREVIEW_VOICES],
    /// The layer being auditioned, clamped the way the pad table clamps it.
    /// Kept rather than re-sent, so a loop can light its next lap at every
    /// seam without the UI thread being involved in the timing.
    layer: Option<LayerSlot>,
    config: PadConfig,
    looping: bool,
    /// The slot that is leading. The lap starts in another one, and takes
    /// the lead when it does.
    head: usize,
}

impl Preview {
    pub(super) fn new() -> Self {
        Self {
            voices: std::array::from_fn(|_| SamplerVoice::new()),
            layer: None,
            // Any pad will do for a preview that is not running; it is
            // replaced whole before a voice ever reads it.
            config: PadConfig::for_key(60),
            looping: false,
            head: 0,
        }
    }

    /// What the UI asked for. `None` stops the audition with the kill fade.
    ///
    /// Re-sending a layer retriggers: the voices in flight are cut with the
    /// same fade a stolen voice gets, and the new pass starts underneath
    /// them. That is the engine's existing answer to "the same thing again,
    /// now" and an audition is the case it was written for — a player
    /// walking a trim marker sends one of these per press.
    pub(super) fn set(&mut self, preview: Option<&PreviewLayer>, engine_sr: f64) {
        let Some(request) = preview else {
            self.stop();
            return;
        };
        let Some(slot) = LayerSlot::from_layer(&request.layer) else {
            // Nothing playable behind it: silence is the honest answer, and
            // it is the same one the pad table gives an empty buffer.
            self.stop();
            return;
        };
        self.layer = Some(slot);
        self.config = flattened(&request.config);
        self.looping = request.mode == PreviewMode::Loop;
        // The pass already in flight fades out under the new one instead of
        // being truncated, and instead of stacking with it.
        self.cut();
        self.fire(engine_sr);
    }

    /// Cut the audition and forget what it was. Idempotent.
    pub(super) fn stop(&mut self) {
        self.cut();
        self.layer = None;
        self.looping = false;
    }

    /// Fade out everything sounding, leaving the audition's settings alone.
    fn cut(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
    }

    /// Stop instantly, no fade — for `reset`, where the whole output is
    /// being silenced anyway.
    pub(super) fn silence(&mut self) {
        for v in &mut self.voices {
            v.silence();
        }
        self.layer = None;
        self.looping = false;
    }

    /// One sample of the audition, summed into the mix.
    pub(super) fn tick(&mut self, engine_sr: f64) -> (f32, f32) {
        if self.looping && self.lap_is_due() {
            self.fire(engine_sr);
        }
        let mut sum = (0.0f32, 0.0f32);
        for v in &mut self.voices {
            if v.is_sounding() {
                let (l, r) = v.tick();
                sum.0 += l;
                sum.1 += r;
            }
        }
        sum
    }

    /// Whether the loop is due for its next lap.
    ///
    /// Two ways in, and the second one is the degenerate case rather than an
    /// afterthought. The ordinary way is the overlap: the voice in the lead
    /// is inside its own closing edge fade and has already played at least
    /// one fade's worth, so the next lap starts under it and the seam is two
    /// ramps crossing. The other is a region too short to hold two fades —
    /// under about four milliseconds — where there is nothing to overlap;
    /// there the lap simply follows the voice that ended. Without that
    /// second branch a one-millisecond loop would either stop after a single
    /// pass or relight itself every sample.
    fn lap_is_due(&self) -> bool {
        let lead = &self.voices[self.head];
        if !lead.is_sounding() {
            return true;
        }
        let fade = lead.fade_len();
        lead.elapsed() >= fade && lead.samples_to_exit() <= fade
    }

    /// Start a lap in a free slot, and give it the lead.
    fn fire(&mut self, engine_sr: f64) {
        let next = self.free_slot();
        // Copied out, so the only field still borrowed below is the layer and
        // the voice array is free to be written.
        let config = self.config;
        let Some(slot) = self.layer.as_ref() else {
            // Nothing to fire is nothing to loop. The flag must not outlive
            // the layer, or `tick` would ask for a lap every sample for as
            // long as the plugin is loaded.
            self.looping = false;
            return;
        };
        let note = preview_note(&config);
        self.voices[next].start(note, 0, 0, 0, &config, &slot.trigger(), 1.0, engine_sr);
        self.head = next;
    }

    /// A slot that is not sounding, else the one furthest through its fade.
    ///
    /// The fallback only comes up under a retrigger storm — three presses
    /// inside the three-millisecond kill fade — and it takes the quietest
    /// voice rather than a fixed slot so that the cut it makes is the
    /// smallest one available.
    fn free_slot(&self) -> usize {
        let mut quietest = 0usize;
        let mut best = f64::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.is_sounding() {
                return i;
            }
            let left = v.samples_to_exit();
            if left < best {
                best = left;
                quietest = i;
            }
        }
        quietest
    }

    #[cfg(test)]
    pub(super) fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.is_sounding()).count()
    }
}
