//! The audio engine: owns the transport, synth, MIDI routing,
//! and drives the audio callback.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crossbeam_channel::Sender;
use phosphor_midi::message::{MidiMessage, MidiMessageType};
use phosphor_midi::ring::MidiRingReceiver;
use phosphor_plugin::{MidiEvent, Plugin};

use crate::mixer::{Mixer, MixerCommand, mixer_command_channel};
use crate::project::TrackHandle;
use crate::transport::Transport;
use crate::EngineConfig;

/// Shared VU meter levels — written by audio thread, read by UI thread.
/// Stored as f32 bits in AtomicU32 (lock-free, no allocation).
#[derive(Debug)]
pub struct VuLevels {
    /// Peak level (0.0..1.0) — decays over time.
    pub peak_l: AtomicU32,
    pub peak_r: AtomicU32,
}

impl VuLevels {
    pub fn new() -> Self {
        Self {
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
        }
    }

    pub fn set(&self, l: f32, r: f32) {
        self.peak_l.store(l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(r.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> (f32, f32) {
        (
            f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
        )
    }
}

/// The core audio engine. Lives on the audio thread.
///
/// The engine is split into two parts:
/// - `EngineShared`: read from any thread (transport, config)
/// - `EngineAudio`: owned by the audio callback (synth, MIDI receiver)
pub struct EngineShared {
    pub config: EngineConfig,
    pub transport: Arc<Transport>,
    pub panic_flag: Arc<AtomicBool>,
    /// Real-time VU meter levels from the audio thread (master bus).
    pub vu_levels: Arc<VuLevels>,
    /// Send commands to the mixer (add/remove tracks, set instruments).
    pub mixer_command_tx: Sender<MixerCommand>,
    /// Per-track handles for UI to read VU / write mute/solo/arm/volume.
    pub track_handles: Vec<Arc<TrackHandle>>,
}

impl EngineShared {
    pub fn new(config: EngineConfig) -> Self {
        let (tx, _rx) = mixer_command_channel();
        Self {
            config,
            transport: Arc::new(Transport::new(120.0)),
            panic_flag: Arc::new(AtomicBool::new(false)),
            vu_levels: Arc::new(VuLevels::new()),
            mixer_command_tx: tx,
            track_handles: Vec::new(),
        }
    }

    /// Create shared state with a specific command sender (for wiring to a mixer).
    pub fn with_command_tx(config: EngineConfig, tx: Sender<MixerCommand>) -> Self {
        Self {
            config,
            transport: Arc::new(Transport::new(120.0)),
            panic_flag: Arc::new(AtomicBool::new(false)),
            vu_levels: Arc::new(VuLevels::new()),
            mixer_command_tx: tx,
            track_handles: Vec::new(),
        }
    }

    /// Trigger a panic — kills all sound on next audio callback.
    pub fn panic(&self) {
        self.panic_flag.store(true, Ordering::Relaxed);
    }
}

/// Audio-thread-only state. NOT Send — lives inside the audio callback closure.
pub struct EngineAudio {
    channels: u16,
    sample_rate: u32,
    synth: Box<dyn Plugin>,
    midi_rx: Option<MidiRingReceiver>,
    panic_flag: Arc<AtomicBool>,
    vu_levels: Arc<VuLevels>,
    /// Scratch buffer for MIDI events (reused to avoid allocation).
    midi_scratch: Vec<MidiMessage>,
    /// Plugin-format MIDI events for the current buffer.
    plugin_events: Vec<MidiEvent>,
    /// Scratch audio buffers for plugin output.
    plugin_buf_l: Vec<f32>,
    plugin_buf_r: Vec<f32>,
    /// Per-track mixer. Processes all tracks and mixes to master.
    mixer: Option<Mixer>,
}

impl EngineAudio {
    pub fn new(
        config: &EngineConfig,
        synth: Box<dyn Plugin>,
        midi_rx: Option<MidiRingReceiver>,
        panic_flag: Arc<AtomicBool>,
        vu_levels: Arc<VuLevels>,
    ) -> Self {
        let buf_size = config.buffer_size as usize;
        let mut s = Self {
            channels: 2,
            sample_rate: config.sample_rate,
            synth,
            midi_rx,
            panic_flag,
            vu_levels,
            midi_scratch: Vec::with_capacity(256),
            plugin_events: Vec::with_capacity(256),
            plugin_buf_l: vec![0.0; buf_size],
            plugin_buf_r: vec![0.0; buf_size],
            mixer: None,
        };
        s.synth.init(config.sample_rate as f64, buf_size);
        s
    }

    /// Create an EngineAudio with a mixer instead of a single synth.
    pub fn with_mixer(
        config: &EngineConfig,
        mixer: Mixer,
        midi_rx: Option<MidiRingReceiver>,
        panic_flag: Arc<AtomicBool>,
        vu_levels: Arc<VuLevels>,
    ) -> Self {
        let buf_size = config.buffer_size as usize;
        // We still need a dummy synth for the legacy code path;
        // when the mixer is present, the synth is not used.
        use phosphor_plugin::{ParameterInfo, PluginCategory, PluginInfo};
        struct NullPlugin;
        impl Plugin for NullPlugin {
            fn info(&self) -> PluginInfo {
                PluginInfo {
                    name: "null".into(),
                    version: "0".into(),
                    author: "".into(),
                    category: PluginCategory::Utility,
                }
            }
            fn init(&mut self, _sr: f64, _bs: usize) {}
            fn process(
                &mut self,
                _i: &[&[f32]],
                _o: &mut [&mut [f32]],
                _m: &[MidiEvent],
            ) {
            }
            fn parameter_count(&self) -> usize { 0 }
            fn parameter_info(&self, _: usize) -> Option<ParameterInfo> { None }
            fn get_parameter(&self, _: usize) -> f32 { 0.0 }
            fn set_parameter(&mut self, _: usize, _: f32) {}
            fn reset(&mut self) {}
        }

        Self {
            channels: 2,
            sample_rate: config.sample_rate,
            synth: Box::new(NullPlugin),
            midi_rx,
            panic_flag,
            vu_levels,
            midi_scratch: Vec::with_capacity(256),
            plugin_events: Vec::with_capacity(256),
            plugin_buf_l: vec![0.0; buf_size],
            plugin_buf_r: vec![0.0; buf_size],
            mixer: Some(mixer),
        }
    }

    /// Drain and discard any pending MIDI events. Call before starting
    /// the audio stream to flush controller init bursts.
    pub fn flush_midi(&mut self) {
        if let Some(rx) = &mut self.midi_rx {
            self.midi_scratch.clear();
            rx.drain_into(&mut self.midi_scratch);
            if !self.midi_scratch.is_empty() {
                tracing::info!("Flushed {} stale MIDI events", self.midi_scratch.len());
            }
            self.midi_scratch.clear();
        }
    }

    /// Process one interleaved audio buffer. Called from the audio thread.
    ///
    /// `output` is interleaved: [L0, R0, L1, R1, ...]
    pub fn process(&mut self, output: &mut [f32], transport: &Transport) {
        // Check panic flag — kill all sound immediately
        if self.panic_flag.swap(false, Ordering::Relaxed) {
            self.synth.reset();
            if let Some(ref mut mixer) = self.mixer {
                mixer.reset_all();
            }
            output.fill(0.0);
            return;
        }

        let num_frames = output.len() / self.channels as usize;

        // Drain MIDI from ring buffer
        self.midi_scratch.clear();
        if let Some(rx) = &mut self.midi_rx {
            rx.drain_into(&mut self.midi_scratch);
        }

        // If we have a mixer, delegate to it
        if let Some(ref mut mixer) = self.mixer {
            mixer.process(output, &self.midi_scratch, transport);
            // Advance transport after processing (so playback reads the pre-advance position)
            transport.advance(num_frames as u32, self.sample_rate);
            return;
        }

        // Legacy single-synth path (for tests and backward compat)
        // Ensure scratch buffers are big enough
        if self.plugin_buf_l.len() < num_frames {
            self.plugin_buf_l.resize(num_frames, 0.0);
            self.plugin_buf_r.resize(num_frames, 0.0);
        }

        // Convert MIDI to plugin events
        self.plugin_events.clear();
        for msg in &self.midi_scratch {
            if let Some(ev) = midi_to_plugin_event(msg) {
                self.plugin_events.push(ev);
            }
        }

        // Clear plugin output buffers
        self.plugin_buf_l[..num_frames].fill(0.0);
        self.plugin_buf_r[..num_frames].fill(0.0);

        // Process synth
        {
            let mut outputs: [&mut [f32]; 2] = [
                &mut self.plugin_buf_l[..num_frames],
                &mut self.plugin_buf_r[..num_frames],
            ];
            let mut out_slices: Vec<&mut [f32]> = outputs.iter_mut().map(|s| &mut **s).collect();
            self.synth.process(&[], &mut out_slices, &self.plugin_events);
        }

        // Interleave into output and compute peak levels
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        for i in 0..num_frames {
            let idx = i * self.channels as usize;
            let l = self.plugin_buf_l[i];
            output[idx] = l;
            peak_l = peak_l.max(l.abs());
            if self.channels >= 2 {
                let r = self.plugin_buf_r[i];
                output[idx + 1] = r;
                peak_r = peak_r.max(r.abs());
            }
        }

        // Smooth VU: fast attack, slow decay
        let (old_l, old_r) = self.vu_levels.get();
        let decay = 0.85f32; // ~60ms decay at 44.1k/64
        let new_l = if peak_l > old_l { peak_l } else { old_l * decay };
        let new_r = if peak_r > old_r { peak_r } else { old_r * decay };
        self.vu_levels.set(new_l, new_r);

        // Advance transport
        transport.advance(num_frames as u32, self.sample_rate);
    }
}

/// Convert a phosphor-midi MidiMessage to a phosphor-plugin MidiEvent.
fn midi_to_plugin_event(msg: &MidiMessage) -> Option<MidiEvent> {
    // For now, all events get sample_offset 0 (within the current buffer).
    // Future: use msg.timestamp for sample-accurate positioning.
    match msg.message_type {
        MidiMessageType::NoteOn { .. }
        | MidiMessageType::NoteOff { .. }
        | MidiMessageType::ControlChange { .. }
        | MidiMessageType::PitchBend { .. } => Some(MidiEvent {
            sample_offset: 0,
            status: msg.raw[0],
            data1: msg.raw[1],
            data2: msg.raw[2],
        }),
        _ => None,
    }
}

// ── Legacy Engine (for tests and simple use) ──

/// Simple combined engine for tests and the TUI.
pub struct Engine {
    pub shared: EngineShared,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        let (tx, _rx) = mixer_command_channel();
        Self {
            shared: EngineShared::with_command_tx(config, tx),
        }
    }

    /// Create an engine wired to a mixer command channel.
    pub fn with_command_tx(config: EngineConfig, tx: Sender<MixerCommand>) -> Self {
        Self {
            shared: EngineShared::with_command_tx(config, tx),
        }
    }

    /// Access the transport.
    pub fn transport(&self) -> &Transport {
        &self.shared.transport
    }
}

// Re-export for backward compat with TUI code
impl std::ops::Deref for Engine {
    type Target = EngineShared;
    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_dsp::synth::PhosphorSynth;
    use phosphor_midi::ring::midi_ring_buffer;
    use phosphor_midi::message::MidiMessage;

    fn test_engine(midi_rx: Option<MidiRingReceiver>) -> (EngineAudio, Arc<Transport>) {
        let config = EngineConfig { buffer_size: 64, sample_rate: 44100 };
        let transport = Arc::new(Transport::new(120.0));
        let panic_flag = Arc::new(AtomicBool::new(false));
        let vu_levels = Arc::new(VuLevels::new());
        let synth = Box::new(PhosphorSynth::new());
        let engine = EngineAudio::new(&config, synth, midi_rx, panic_flag, vu_levels);
        (engine, transport)
    }

    #[test]
    fn engine_produces_silence_with_no_midi() {
        let (mut engine, transport) = test_engine(None);
        let mut output = vec![0.0f32; 128]; // 64 frames stereo
        engine.process(&mut output, &transport);
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn engine_produces_sound_from_midi() {
        let (mut tx, rx) = midi_ring_buffer();
        let (mut engine, transport) = test_engine(Some(rx));

        // Send note on
        let msg = MidiMessage::from_bytes(&[0x90, 60, 100], 0).unwrap();
        tx.push(msg);

        let mut output = vec![0.0f32; 512]; // 256 frames stereo
        engine.process(&mut output, &transport);

        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.01, "Should produce sound from MIDI, peak={peak}");
    }

    #[test]
    fn engine_output_is_stereo() {
        let (mut tx, rx) = midi_ring_buffer();
        let (mut engine, transport) = test_engine(Some(rx));

        let msg = MidiMessage::from_bytes(&[0x90, 60, 100], 0).unwrap();
        tx.push(msg);

        let mut output = vec![0.0f32; 128];
        engine.process(&mut output, &transport);

        // Left and right channels should be identical (mono synth)
        for i in 0..64 {
            assert_eq!(output[i * 2], output[i * 2 + 1], "L/R mismatch at frame {i}");
        }
    }

    #[test]
    fn engine_output_always_finite() {
        let (mut tx, rx) = midi_ring_buffer();
        let (mut engine, transport) = test_engine(Some(rx));

        let msg = MidiMessage::from_bytes(&[0x90, 60, 127], 0).unwrap();
        tx.push(msg);

        for _ in 0..1000 {
            let mut output = vec![0.0f32; 128];
            engine.process(&mut output, &transport);
            assert!(output.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn engine_note_off_leads_to_silence() {
        let (mut tx, rx) = midi_ring_buffer();
        let (mut engine, transport) = test_engine(Some(rx));

        // Note on
        tx.push(MidiMessage::from_bytes(&[0x90, 60, 100], 0).unwrap());
        let mut output = vec![0.0f32; 128];
        engine.process(&mut output, &transport);

        // Note off
        tx.push(MidiMessage::from_bytes(&[0x80, 60, 0], 0).unwrap());

        // Process enough for release to finish (exponential decay needs more buffers)
        for _ in 0..500 {
            output.fill(0.0);
            engine.process(&mut output, &transport);
        }

        // Should be silent now
        let peak = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak < 0.001, "Should be silent after release, peak={peak}");
    }

    #[test]
    fn engine_advances_transport() {
        let (mut engine, transport) = test_engine(None);
        transport.play();
        let mut output = vec![0.0f32; 128];
        engine.process(&mut output, &transport);
        assert!(transport.position_ticks() > 0);
    }
}
