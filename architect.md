# Phosphor — Architecture Plan

A Rust-based DAW with dual TUI and GUI frontends, built-in synthesizers, MIDI controller auto-detection, and a modular plugin system.

---

## Core Principles

1. **Shared core, swappable frontend** — All audio, MIDI, sequencing, and plugin logic lives in a headless `phosphor-core` library. TUI and GUI are thin presentation layers.
2. **Modular from day one** — Plugin API defined early. Internal synths and effects are themselves plugins.
3. **Real-time safe** — Audio thread never allocates, never locks. Communication via lock-free ring buffers.
4. **Cross-platform** — Linux, macOS, Windows. Native builds per platform.
5. **Correct by construction** — Every module ships with unit tests and integration tests. No feature merges without passing tests. Bugs are cheaper to prevent than to fix.
6. **Latency is a feature** — Sub-millisecond MIDI-to-audio. Peripheral response must feel instant. We measure, enforce, and regress-test latency budgets.
7. **Performance is non-negotiable** — Zero-copy where possible. No allocations in hot paths. Profile before and after every feature lands.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────┐
│                  Frontends                       │
│  ┌──────────────┐       ┌──────────────────┐    │
│  │  TUI (ratatui)│       │  GUI (egui/wgpu) │    │
│  └──────┬───────┘       └────────┬─────────┘    │
│         └──────────┬─────────────┘              │
│              phosphor-ui (trait)                 │
├─────────────────────────────────────────────────┤
│              phosphor-core                       │
│  ┌─────────┐ ┌──────────┐ ┌──────────────────┐ │
│  │ Engine   │ │Sequencer │ │  Plugin Host     │ │
│  │ (audio   │ │(timeline,│ │  (custom + CLAP) │ │
│  │  graph)  │ │ tracks)  │ │                  │ │
│  └────┬─────┘ └──────────┘ └──────────────────┘ │
│  ┌────┴─────┐ ┌──────────┐ ┌──────────────────┐ │
│  │ cpal I/O │ │ MIDI     │ │  DSP Primitives  │ │
│  │          │ │ (midir)  │ │  (fundsp/dasp)   │ │
│  └──────────┘ └──────────┘ └──────────────────┘ │
└─────────────────────────────────────────────────┘
```

---

## Crate / Module Layout

```
phosphor/
├── Cargo.toml              # workspace root
├── crates/
│   ├── phosphor-core/      # audio engine, sequencer, mixer, plugin host
│   ├── phosphor-dsp/       # built-in synths, effects, filters (all implement plugin trait)
│   ├── phosphor-midi/      # MIDI I/O, controller detection, mapping
│   ├── phosphor-plugin/    # plugin API trait definitions + dynamic loading
│   ├── phosphor-tui/       # ratatui frontend
│   └── phosphor-gui/       # egui frontend
├── plugins/                # directory scanned at runtime for user/3rd-party plugins
├── src/
│   └── main.rs             # CLI entry: `phosphor --tui` or `phosphor --gui`
└── architect.md
```

---

## Technology Stack

| Layer | Crate | Why |
|---|---|---|
| Audio I/O | **cpal** | Cross-platform PCM, JACK support, real-time thread priority |
| Audio Graph | **knyst** | Dynamic real-time graph — add/remove/reconnect nodes without glitches |
| DSP | **fundsp** + **dasp** | fundsp for synth voices & effects, dasp for sample types & conversion |
| MIDI I/O | **midir** | Cross-platform, virtual ports, SysEx |
| MIDI Parse | **wmidi** | Zero-alloc, real-time safe message decoding |
| TUI | **ratatui** + **crossterm** | Active, flexible layout, canvas widget for waveforms |
| GUI | **egui** + **eframe** | Immediate mode, custom widgets (knobs, meters, waveforms), wgpu |
| Plugin Load | **libloading** | Dynamic `.so`/`.dylib`/`.dll` loading |
| CLAP Host | **clack** | Safe Rust wrapper for hosting CLAP plugins |
| Lock-free | **ringbuf** / **crossbeam** | Audio↔UI communication without locks |
| No-alloc enforcement | **assert_no_alloc** | Panic on any allocation inside audio callback during dev/test |
| Benchmarking | **criterion** | Statistical benchmarks for DSP, MIDI routing, graph traversal |
| Property testing | **proptest** | Fuzz inputs to DSP, sequencer, MIDI parser |
| Profiling | **Tracy** (via tracing-tracy) | Frame-level profiling with real-time viewer |
| Latency measurement | **quanta** | Sub-nanosecond timestamps for MIDI→audio pipeline |

---

## Module / Plugin System

### Plugin Trait (C ABI stable)

```rust
// phosphor-plugin/src/lib.rs — the contract every plugin implements

#[repr(C)]
pub struct PluginDescriptor {
    pub name: *const c_char,
    pub version: *const c_char,
    pub category: PluginCategory,  // Synth, Effect, Analyzer, Utility
}

#[repr(C)]
pub enum PluginCategory {
    Instrument,
    Effect,
    Analyzer,
    Utility,
}

/// Trait exposed via C ABI so plugins can be any language (Rust, C, C++)
pub trait PhosphorPlugin {
    fn descriptor(&self) -> PluginDescriptor;
    fn init(&mut self, sample_rate: f64, max_buffer_size: usize);
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi: &[MidiEvent]);
    fn parameter_count(&self) -> usize;
    fn get_parameter(&self, index: usize) -> f32;
    fn set_parameter(&mut self, index: usize, value: f32);
    fn reset(&mut self);
}
```

### Plugin Discovery

- On startup, scan `~/.phosphor/plugins/` and `./plugins/` for shared libraries
- Each library exports `phosphor_create_plugin() -> *mut dyn PhosphorPlugin`
- Plugins are loaded via `libloading`, instantiated, and registered in the plugin host
- Built-in synths/effects in `phosphor-dsp` implement the same trait — no special casing

### Future: CLAP Hosting

- Use **clack** to host industry-standard CLAP plugins alongside native Phosphor plugins
- CLAP chosen over VST3 because it's open-source, simpler, and has better Rust support

---

## MIDI Controller Auto-Detection

```
┌──────────────┐
│  midir poll   │──▶ enumerate ports every 2s
│  (background) │
└──────┬───────┘
       │ new port detected
       ▼
┌──────────────┐
│ Device ID    │──▶ send SysEx identity request
│ Query        │    parse manufacturer/model from reply
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Profile DB   │──▶ match against known controller profiles (JSON)
│ Lookup       │    load default CC/note mappings
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ MIDI Router  │──▶ route messages to tracks/plugins
│              │    user can remap in UI
└──────────────┘
```

- Controller profiles stored as JSON in `~/.phosphor/controllers/`
- Unknown controllers get a generic mapping (CCs → parameters, notes → notes)
- Users can create/share profiles

---

## Sequencer & Timeline

- **Timeline** measured in ticks (PPQ 960) with a tempo map for BPM changes
- **Tracks**: each track has a type (Audio, MIDI, Bus), solo/mute/arm state, volume/pan, plugin chain
- **Clips**: regions on a track with start tick, length, and content reference
- **Transport**: play, pause, stop, record, loop — communicated to audio thread via atomic state + ring buffer
- Audio thread does look-ahead reads from the timeline to fill buffers

---

## Frontend Abstraction

```rust
/// Both TUI and GUI implement this trait
pub trait PhosphorUI {
    fn run(&mut self, engine: Arc<Engine>) -> Result<()>;
    fn render_tracks(&self, tracks: &[Track]);
    fn render_mixer(&self, mixer: &Mixer);
    fn render_transport(&self, transport: &TransportState);
    fn handle_input(&mut self) -> Vec<UIAction>;
}
```

### TUI Layout (ratatui)

```
┌─ Transport ──────────────────────────────┐
│  ▶ Play  ⏸ 120 BPM  4/4  Bar 1.1.0     │
├─ Tracks ─────────────────────────────────┤
│ 1 [M][S] Kick     ████░░░░████░░░░████  │
│ 2 [M][S] Bass     ░░██████░░░░████████  │
│ 3 [M][S] Lead     ██████░░░░░░██████░░  │
├─ Mixer ──────────────────────────────────┤
│  ▌1▐  ▌2▐  ▌3▐  ▌M▐   (vertical bars)  │
├─ Status ─────────────────────────────────┤
│ MIDI: Akai MPK Mini | CPU: 12% | 44.1k  │
└──────────────────────────────────────────┘
```

### GUI Layout (egui)

- Same logical layout but with proper waveform rendering, rotary knobs, drag-and-drop clips, piano roll, spectrum analyzers
- Custom egui widgets for: VU meters, knobs, waveform display, piano roll grid

---

## Implementation Phases

### Phase 1 — Foundation (current target)
- [ ] Workspace setup with crate structure, CI pipeline (fmt + clippy + test + bench + audit)
- [ ] `phosphor-core`: cpal audio output, basic callback that outputs silence/test tone
- [ ] `phosphor-core` tests: verify callback fires, output buffer is correct length, no panics
- [ ] `phosphor-midi`: midir port enumeration, receive MIDI messages, push to ring buffer
- [ ] `phosphor-midi` tests: message parsing round-trip, ring buffer ordering, port enumeration with mock
- [ ] `phosphor-tui`: ratatui skeleton with transport bar and track list (static data)
- [ ] CLI entry point: `phosphor --tui`
- [ ] Benchmark baseline: empty audio callback latency, MIDI parse throughput
- [ ] `TestBackend` for audio: in-memory capture so all future tests run without a sound card

### Phase 2 — Sound
- [ ] `phosphor-dsp`: basic oscillator (sine, saw, square, tri) implementing plugin trait
- [ ] DSP tests: frequency accuracy (FFT), amplitude, no NaN/Inf, no denormals, 10-second stability
- [ ] DSP proptest: arbitrary freq × sample rate combos never produce NaN
- [ ] DSP benchmarks: oscillator throughput (samples/sec), establish baseline
- [ ] Wire MIDI input → oscillator → audio output (monophonic synth)
- [ ] Integration test: MIDI note-on → non-silent output, note-off → decay to silence
- [ ] Latency test: MIDI→audio path ≤ 2 buffer periods
- [ ] `phosphor-plugin`: define the plugin trait, load built-in synths through it
- [ ] Plugin tests: trait contract verification, parameter clamping
- [ ] TUI: real-time MIDI status display, synth parameter controls

### Phase 3 — Sequencer
- [ ] Timeline data structures (tracks, clips, tempo map)
- [ ] Sequencer unit tests: tick↔sample conversion round-trip, tempo map interpolation, clip boundary math
- [ ] Sequencer proptest: tick↔sample round-trip for arbitrary BPM/sample-rate
- [ ] MIDI clip playback (read MIDI events from timeline, send to synth)
- [ ] Integration test: sequencer playback is sample-accurate (note at tick N → correct sample offset)
- [ ] Transport controls (play/pause/stop/loop)
- [ ] Transport test: start→stop→start produces bit-identical output
- [ ] TUI: interactive track view with pattern display, transport controls

### Phase 4 — Mixer & Effects
- [ ] Audio graph with mixer (per-track volume, pan, solo, mute)
- [ ] Mixer tests: solo/mute logic, volume/pan math, graph add/remove without glitches
- [ ] Glitch detection test: add/remove nodes while processing → no discontinuities above threshold
- [ ] Built-in effects: filter (LP/HP/BP), delay, reverb (all as plugins)
- [ ] Effect tests: impulse response verification, no denormals after silence, bypass is bit-transparent
- [ ] Effect benchmarks: filter chain throughput (must be < 5% CPU for 16 tracks)
- [ ] Per-track plugin chain (insert effects)
- [ ] TUI: mixer view with level meters

### Phase 5 — Plugin System
- [ ] Dynamic plugin loading from `~/.phosphor/plugins/`
- [ ] Plugin safety tests: panic recovery, ABI version mismatch rejection, allocation detection
- [ ] Plugin API stabilization, documentation, example plugin template
- [ ] CLAP host integration via clack
- [ ] Hot-reload support for development

### Phase 6 — GUI Frontend
- [ ] `phosphor-gui`: egui/eframe app shell
- [ ] Reimplement all TUI views as egui panels (transport, tracks, mixer)
- [ ] GUI render budget test: frame time < 16ms with 32 tracks visible
- [ ] Custom widgets: knobs, VU meters, waveform display
- [ ] Piano roll editor for MIDI clips
- [ ] CLI: `phosphor --gui` flag

### Phase 7 — Polish & Distribution
- [ ] MIDI controller auto-detection with profile database
- [ ] Controller detection tests: known device → correct profile, unknown → generic mapping
- [ ] Audio recording (input → clip)
- [ ] Recording test: captured audio matches input within floating-point epsilon
- [ ] Project save/load (serde → JSON or binary)
- [ ] Save/load round-trip test: save → load → save produces identical output
- [ ] Cross-platform CI (Linux, macOS, Windows)
- [ ] Packaging: AppImage (Linux), .dmg (macOS), .exe installer (Windows)

---

## Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Audio graph | knyst (dynamic) over fundsp (static) | DAW needs runtime graph changes (add/remove tracks, reorder plugins) |
| GUI framework | egui over iced | Faster iteration, easier custom widgets, good enough for DAW |
| Plugin ABI | C ABI with Rust trait | Cross-language compatibility, no Rust ABI instability |
| Plugin format | Custom + CLAP hosting | Custom for simplicity, CLAP for ecosystem access |
| Sequencer resolution | 960 PPQ ticks | Industry standard, sufficient for all quantization levels |
| Lock-free comms | ringbuf for audio↔UI | No mutex on audio thread, ever |
| Project format | JSON (serde) | Human-readable, diffable, easy to debug |

---

## Platform Notes

- **Linux**: ALSA required (`libasound2-dev`), JACK optional. AppImage for distribution.
- **macOS**: CoreAudio works out of the box. Code signing needed for distribution.
- **Windows**: WASAPI default. ASIO support via cpal feature flag for low-latency pro use. `.exe` via cargo-dist.
- All GUI rendering via wgpu — works on all platforms with Vulkan/Metal/DX12.

---

## Risk Mitigations

- **"No mature Rust DAW exists"** — True, but the audio crate ecosystem (cpal, fundsp, midir) is solid. We only need to build the glue and UI.
- **GUI complexity** — Start TUI-first to prove the core. GUI is a frontend swap, not a rewrite.
- **Plugin ABI stability** — Use C ABI from the start. Version the plugin API. Don't expose Rust internals.
- **Real-time safety** — Enforce no-alloc in audio thread via `assert_no_alloc` crate during development.

---

## Performance & Latency Architecture

This is the section that separates Phosphor from every other DAW. Most DAWs treat performance as an afterthought — we treat it as a first-class architectural constraint.

### Thread Model

```
┌─────────────────────────────────────────────────────────────┐
│                        Thread Map                            │
├──────────────┬──────────────────────────────────────────────┤
│ Audio Thread │ HIGHEST priority. Real-time scheduled.       │
│ (1 thread)   │ Runs the audio graph, reads MIDI, writes PCM│
│              │ RULES: no alloc, no lock, no syscall,        │
│              │ no I/O, no logging. Violation = panic in dev │
├──────────────┼──────────────────────────────────────────────┤
│ MIDI Thread  │ High priority. Dedicated per-port receive.   │
│ (1 per port) │ Timestamps messages with quanta::Instant,    │
│              │ pushes to lock-free SPSC ring → audio thread │
├──────────────┼──────────────────────────────────────────────┤
│ UI Thread    │ Normal priority. Renders at 30-60 FPS.       │
│ (1 thread)   │ Reads state snapshots from audio thread via  │
│              │ triple buffer. Never blocks audio.           │
├──────────────┼──────────────────────────────────────────────┤
│ Worker Pool  │ Normal priority. File I/O, plugin scanning,  │
│ (N threads)  │ waveform rendering, project save/load.       │
│              │ Communicates with UI via channels.           │
└──────────────┴──────────────────────────────────────────────┘
```

### Latency Budgets

| Path | Target | How |
|---|---|---|
| MIDI input → audio output | **< 3ms** (ideally < 1ms) | Dedicated MIDI thread, SPSC ring to audio thread, no queuing. At 44.1kHz/64 samples = 1.45ms buffer. We target 64-sample buffers. |
| Key press (TUI/GUI) → UI update | **< 16ms** (60fps) | UI thread reads atomic state, no round-trip through audio |
| Transport start → first audible sample | **< 5ms** | Pre-computed look-ahead buffer, audio graph always hot |
| Plugin parameter change → audible | **< 1 buffer** | Parameters are atomics, read every buffer cycle |
| MIDI controller knob → parameter change | **< 1 buffer** | Direct MIDI CC → atomic parameter write, no message queue |

### MIDI Latency — The Critical Path

Most DAWs add unnecessary latency to MIDI because they batch messages, route through a central dispatcher, or process MIDI on the audio thread's schedule. We don't.

```
Controller ──┐
             │ USB/hardware (fixed, ~1ms)
             ▼
midir callback ──┐
                 │ timestamp with quanta::Instant (< 1μs)
                 │ push to SPSC ring buffer (lock-free, < 100ns)
                 ▼
Audio thread ────┐
                 │ drain ring buffer at top of each callback
                 │ apply messages with sample-accurate offset
                 │ (timestamp delta / sample period = sample offset)
                 ▼
Audio output ────▶ speaker/headphones
```

**Sample-accurate MIDI**: We don't just process MIDI "once per buffer." Each MIDI message carries a sub-buffer timestamp. The audio callback splits processing at each MIDI event boundary, so a note-on at sample 17 of a 64-sample buffer is rendered starting exactly at sample 17. This eliminates the ±1 buffer jitter that plagues most DAWs.

### Audio Thread Rules (Enforced)

These aren't guidelines — they're enforced at compile time where possible and at runtime during development:

```rust
// In dev/test builds, wraps the audio callback:
#[cfg(debug_assertions)]
assert_no_alloc::assert_no_alloc(|| {
    engine.process_audio(buffer);
});
```

| Rule | Enforcement |
|---|---|
| No heap allocation | `assert_no_alloc` crate — panics on any alloc in debug builds |
| No mutex/rwlock | Code review + `#[deny(clippy::mutex_in_audio)]` custom lint |
| No I/O (file, network, logging) | Audio module has no `std::fs`, `std::net`, `log` in deps |
| No unbounded loops | All loops iterate over fixed-size buffers |
| No system calls | Preallocate everything during `init()`, not `process()` |
| Fixed-size data structures | `ArrayVec`, `heapless::Vec` instead of `Vec` in audio path |

### Memory & Allocation Strategy

```
STARTUP (allocate freely):
  ├── Audio graph node pool (pre-sized arena)
  ├── MIDI ring buffers (fixed capacity: 4096 events)
  ├── Plugin instance scratch buffers
  ├── Waveform display caches
  └── String interning for parameter names

RUNTIME (audio thread — zero alloc):
  ├── Process graph in pre-allocated traversal order
  ├── Intermediate buffers from pre-sized pool
  ├── MIDI events from fixed ring buffer
  └── Parameter smoothing in stack-allocated arrays

RUNTIME (UI thread — alloc OK but budgeted):
  ├── Frame allocator: bulk alloc at frame start, drop at frame end
  ├── Waveform/meter rendering into pre-allocated textures
  └── String formatting for display values only
```

### CPU Efficiency

| Technique | Where | Impact |
|---|---|---|
| SIMD via `std::simd` / `packed_simd` | DSP: mixing, filtering, gain | 4-8x throughput on f32 operations |
| Graph traversal order caching | Audio engine | Avoid topological sort every buffer cycle |
| Branch-free DSP | Oscillators, filters | Eliminate branch misprediction in inner loops |
| Buffer reuse pool | Audio graph | Zero alloc for intermediate audio buffers |
| Denormal flushing | All DSP | Set FTZ/DAZ flags to prevent 100x slowdown on near-zero signals |
| Lazy UI updates | TUI/GUI | Only re-render changed regions, not full screen |

### Denormal Protection

Denormalized floating-point numbers (very small values near zero) cause catastrophic CPU spikes in audio — a single filter can go from 1% to 90% CPU. Every DSP node must:

```rust
/// Flush denormals to zero. Called once at top of audio callback.
fn setup_denormal_flushing() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // Set FTZ (Flush To Zero) and DAZ (Denormals Are Zero) bits in MXCSR
        let mut mxcsr = std::arch::x86_64::_mm_getcsr();
        mxcsr |= 0x8040; // FTZ | DAZ
        std::arch::x86_64::_mm_setcsr(mxcsr);
    }
}
```

---

## Testing Strategy

Every crate has its own `tests/` directory. No feature ships without tests. No PR merges with a failing test. Tests are the specification.

### Test Pyramid

```
        ╱  ╲          End-to-End (few, slow)
       ╱    ╲         Full pipeline: MIDI in → audio out, project save/load round-trip
      ╱──────╲
     ╱        ╲       Integration (moderate)
    ╱          ╲      Cross-crate: engine + sequencer, MIDI + plugin host
   ╱────────────╲
  ╱              ╲    Unit (many, fast)
 ╱                ╲   Per-function: oscillator output, filter coefficients, tick math
╱──────────────────╲
```

### Test Categories by Crate

#### `phosphor-dsp` — Unit Tests

```rust
#[cfg(test)]
mod tests {
    // Every oscillator tested for:
    // 1. Correct frequency (FFT peak within ±1 Hz)
    // 2. Correct amplitude (peak within ±0.01)
    // 3. No NaN or infinity in output
    // 4. No denormals in output
    // 5. Deterministic output (same input = same output, always)

    #[test]
    fn sine_oscillator_correct_frequency() {
        let mut osc = SineOscillator::new(440.0, 44100.0);
        let mut buffer = [0.0f32; 4096];
        osc.process(&mut buffer);
        let peak_freq = fft_peak_frequency(&buffer, 44100.0);
        assert!((peak_freq - 440.0).abs() < 1.0, "Expected 440Hz, got {peak_freq}Hz");
    }

    #[test]
    fn oscillator_no_nan_or_inf() {
        // Run for 10 seconds of audio — catches long-term instability
        let mut osc = SawOscillator::new(440.0, 44100.0);
        for _ in 0..(44100 * 10 / 64) {
            let mut buf = [0.0f32; 64];
            osc.process(&mut buf);
            assert!(buf.iter().all(|s| s.is_finite()), "NaN/Inf in oscillator output");
        }
    }

    #[test]
    fn filter_no_denormals_after_silence() {
        // Feed signal then silence — filter must not produce denormals
        let mut filter = LowPassFilter::new(1000.0, 44100.0);
        // ... process signal, then 1000 buffers of silence ...
        assert!(buf.iter().all(|s| s == &0.0 || s.abs() > f32::MIN_POSITIVE));
    }
}
```

#### `phosphor-midi` — Unit + Integration Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn midi_message_parsing_round_trip() {
        // Every valid MIDI message type: parse → serialize → parse = identical
    }

    #[test]
    fn midi_timestamp_ordering_preserved() {
        // Messages pushed to ring buffer come out in timestamp order
    }

    #[test]
    fn controller_detection_identifies_known_device() {
        // Mock SysEx identity reply → correct profile loaded
    }

    #[test]
    fn unknown_controller_gets_generic_mapping() {
        // Unrecognized device → default CC mapping, no crash
    }
}
```

#### `phosphor-core` — Integration Tests

```rust
#[cfg(test)]
mod integration {
    #[test]
    fn midi_note_on_produces_audio_output() {
        // Wire MIDI source → synth → audio sink (in-memory)
        // Send note-on, process N buffers, assert output is non-silent
        // Send note-off, process N buffers, assert output decays to silence
    }

    #[test]
    fn audio_graph_add_remove_node_no_glitch() {
        // While audio is processing, add a node, then remove it
        // Assert: no discontinuities > threshold in output
        // (glitch = sample-to-sample delta > 0.1 without note event)
    }

    #[test]
    fn solo_mute_behavior_correct() {
        // 3 tracks playing. Mute track 2 → only 1+3 audible.
        // Solo track 1 → only 1 audible. Un-solo → back to 1+3.
    }

    #[test]
    fn transport_start_stop_is_deterministic() {
        // Start playback from bar 1, stop at bar 2, start again
        // Output must be bit-identical both times
    }

    #[test]
    fn sequencer_playback_sample_accurate() {
        // Place a MIDI note at tick 960 (beat 2 at 120bpm = sample 22050)
        // Assert the note appears at exactly sample 22050 in the output buffer
    }
}
```

#### `phosphor-plugin` — Safety Tests

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn plugin_that_panics_does_not_crash_host() {
        // Load a test plugin that panics in process()
        // Assert: host catches it, disables plugin, audio continues
    }

    #[test]
    fn plugin_that_allocates_is_detected() {
        // In debug builds, a plugin that calls Vec::push in process()
        // triggers assert_no_alloc → test proves we catch it
    }

    #[test]
    fn plugin_with_wrong_abi_version_rejected() {
        // Load a .so with mismatched ABI version → graceful error, not UB
    }

    #[test]
    fn plugin_parameter_out_of_range_clamped() {
        // set_parameter(0, 999.0) where range is 0.0..1.0 → clamped to 1.0
    }
}
```

### Performance / Regression Tests (criterion)

```rust
// benches/dsp_bench.rs — run with `cargo bench`
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_sine_oscillator(c: &mut Criterion) {
    c.bench_function("sine_osc_64_samples", |b| {
        let mut osc = SineOscillator::new(440.0, 44100.0);
        let mut buf = [0.0f32; 64];
        b.iter(|| osc.process(&mut buf));
    });
}

fn bench_mixer_16_tracks(c: &mut Criterion) {
    c.bench_function("mix_16_tracks_64_samples", |b| {
        // 16 tracks of 64 samples each → stereo master
        b.iter(|| mixer.process(&inputs, &mut output));
    });
}

fn bench_midi_routing_1000_events(c: &mut Criterion) {
    c.bench_function("route_1000_midi_events", |b| {
        b.iter(|| router.process(&events));
    });
}

criterion_group!(benches, bench_sine_oscillator, bench_mixer_16_tracks, bench_midi_routing_1000_events);
criterion_main!(benches);
```

**Benchmark CI rule**: If any benchmark regresses by more than 10%, the CI build fails. We use `criterion`'s built-in comparison against the baseline.

### Property-Based Tests (proptest)

```rust
use proptest::prelude::*;

proptest! {
    // Any valid sample rate + frequency combo must not produce NaN
    #[test]
    fn oscillator_never_nan(
        freq in 20.0f64..20000.0,
        sample_rate in prop::sample::subsequence(vec![22050.0, 44100.0, 48000.0, 96000.0], 1..2)
    ) {
        let mut osc = SineOscillator::new(freq, sample_rate[0]);
        let mut buf = [0.0f32; 512];
        osc.process(&mut buf);
        prop_assert!(buf.iter().all(|s| s.is_finite()));
    }

    // Sequencer tick↔sample conversion round-trips cleanly
    #[test]
    fn tick_sample_round_trip(
        tick in 0i64..1_000_000,
        bpm in 30.0f64..300.0
    ) {
        let sample = tick_to_sample(tick, bpm, 44100.0);
        let back = sample_to_tick(sample, bpm, 44100.0);
        prop_assert!((back - tick).abs() <= 1, "Tick round-trip error: {tick} → {sample} → {back}");
    }

    // Any sequence of MIDI bytes can be parsed without panic
    #[test]
    fn midi_parser_never_panics(data: Vec<u8>) {
        let _ = parse_midi_message(&data); // may return Err, must not panic
    }
}
```

### Latency Tests

```rust
#[cfg(test)]
mod latency {
    #[test]
    fn midi_to_audio_latency_under_budget() {
        // Simulated test (no real hardware required):
        // 1. Create engine with 64-sample buffer at 44100 Hz
        // 2. Inject MIDI note-on at known timestamp
        // 3. Measure which output sample first goes non-zero
        // 4. Assert: latency ≤ 2 buffer periods (2.9ms at 64/44100)
        let engine = TestEngine::new(44100, 64);
        let t0 = engine.inject_midi(NoteOn { note: 60, vel: 100 });
        let first_output_sample = engine.find_first_nonzero_output();
        let latency_samples = first_output_sample - t0;
        assert!(latency_samples <= 128, "MIDI→audio latency: {latency_samples} samples ({:.2}ms)",
            latency_samples as f64 / 44100.0 * 1000.0);
    }

    #[test]
    fn parameter_change_audible_within_one_buffer() {
        // Change filter cutoff mid-stream
        // Assert: output changes within the next buffer
    }
}
```

### CI Pipeline

```yaml
# Every push / PR:
steps:
  - cargo fmt --check          # formatting
  - cargo clippy -- -D warnings # lints (warnings = errors)
  - cargo test --workspace     # all unit + integration tests
  - cargo test --workspace -- --ignored  # slow tests (latency, long-running)
  - cargo bench --workspace    # benchmarks — fail on regression > 10%
  - cargo audit                # check dependencies for known vulnerabilities
  - cargo deny check           # license + duplicate dep checks
```

### Test Rules (Enforced for Every PR)

1. **No feature without a test.** If it doesn't have a test, it doesn't exist.
2. **No test without an assertion.** `println!` debugging is not a test.
3. **Tests must be deterministic.** No reliance on wall clock, random seed, or system state. Use injectable clocks and fixed seeds.
4. **DSP tests use epsilon comparisons.** `assert!((a - b).abs() < 1e-6)` not `assert_eq!(a, b)` for floats.
5. **Integration tests use in-memory audio.** No real sound cards needed in CI. Pluggable I/O backend with a `TestBackend` that captures output to a `Vec<f32>`.
6. **Benchmarks are tests too.** A 10% regression is a bug. Criterion baselines stored in git.

---

## Bug Prevention Strategy

We don't fix bugs — we prevent them. The architecture is designed so entire categories of bugs cannot exist.

| Bug Category | Prevention | How |
|---|---|---|
| Use-after-free, double-free, buffer overflow | Rust ownership system | No `unsafe` outside FFI boundaries. All `unsafe` blocks documented and audited. |
| Data races | Rust borrow checker + Send/Sync | Audio thread owns its data. UI reads via atomic snapshots. No shared mutable state. |
| Deadlocks | No mutexes in audio path | Lock-free ring buffers and atomics only. Mutex allowed in worker threads with strict lock ordering. |
| Memory leaks | RAII + arena allocation | Plugins cleaned up on Drop. Audio buffers returned to pool. No manual memory management. |
| Denormal CPU spikes | FTZ/DAZ + output validation | Set CPU flags at thread start. Tests assert no denormals in DSP output. |
| Numeric overflow in tick math | `i64` ticks with bounds checking | 960 PPQ * i64::MAX = billions of years of music. Overflow is impossible in practice. |
| Plugin crashes taking down host | `catch_unwind` at FFI boundary | Plugin panics are caught, plugin is disabled, audio continues. |
| State desync (UI shows wrong value) | Single source of truth | Audio thread is authoritative. UI reads snapshots. No bidirectional sync. |
| Glitches on graph change | Double-buffered graph | New graph compiled on worker thread, swapped atomically. Audio never sees partial state. |
| Resource exhaustion | Bounded buffers + backpressure | Ring buffers have fixed capacity. If UI is slow, it drops frames — audio never waits. |

### Unsafe Code Policy

- `unsafe` is allowed ONLY for: FFI (plugin loading, SIMD intrinsics, denormal flags)
- Every `unsafe` block has a `// SAFETY:` comment explaining the invariant
- All `unsafe` is concentrated in dedicated modules (`ffi.rs`, `simd.rs`), never in business logic
- `#[deny(unsafe_op_in_unsafe_fn)]` enabled workspace-wide
- Miri runs on all non-FFI tests in CI to detect undefined behavior
