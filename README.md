<p align="center">
  <img src="https://i.imgur.com/6oA9IPf.png" alt="Phosphor" width="680"/>
</p>

<p align="center">
  <strong>A terminal-native DAW built in Rust</strong><br/>
  Built-in synthesizers, MIDI controller auto-detection, per-track instruments, and a plugin system designed for extensibility.
</p>

<p align="center">
  <img src="https://i.imgur.com/1Ia9OH2.png" alt="Phosphor UI" width="680"/>
</p>

---

## Index

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Features](#features)
- [Architecture](#architecture)
- [Controls](#controls)
- [Synth Parameters](#synth-parameters)
- [Building from Source](#building-from-source)
- [Project Structure](#project-structure)
- [Configuration](#configuration)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

Phosphor is a digital audio workstation that runs entirely in your terminal. It pairs a solarized-dark TUI with a real-time audio engine, giving you a DAW you can use over SSH, in a tiling window manager, or anywhere a terminal lives.

Each instrument track gets its own polyphonic synthesizer instance with independent parameters. MIDI controllers are detected automatically on startup. The audio engine runs on a dedicated real-time thread with lock-free communication — no mutexes in the audio path, ever.

Phosphor is in active early development. The current beta includes a fully playable polyphonic synth with per-track isolation, real-time VU metering, and vim-style navigation.

---

## Quick Start

```bash
# Install from crates.io
cargo install phosphor-studio

# Or clone and build
git clone https://github.com/joshjetson/phosphor.git
cd phosphor
cargo build --release

# Run (TUI is the default)
cargo run --release

# Run without audio (UI development)
cargo run --release -- --no-audio

# Run without MIDI
cargo run --release -- --no-midi
```

**First steps once running:**

1. Press `Space` to open the command menu
2. Press `a` to add an instrument track
3. Select **Phosphor Synth** and press `Enter`
4. Play your MIDI controller — sound comes out
5. Use `j/k` to navigate synth parameters, `h/l` to adjust values
6. Press `Space` then `a` again to add a second instrument with different settings

---

## Features

**Audio Engine**
- Real-time audio via cpal (CoreAudio, WASAPI, ALSA)
- Lock-free audio thread — no allocations, no mutexes, no I/O
- Per-track instrument instances with independent processing
- Per-track and master VU metering via atomic shared state
- Configurable buffer size (default 64 samples, ~1.5ms latency at 44.1kHz)

**Synthesizer**
- 16-voice polyphonic subtractive synth
- Dual oscillators with adjustable detune (0-50 cents)
- 4 waveforms: sine, saw, square, triangle
- Sub oscillator (one octave down)
- White noise generator
- Resonant state-variable low-pass filter with envelope modulation
- Soft-clip drive/saturation
- Full ADSR envelope
- 12 real-time adjustable parameters

**MIDI**
- Auto-detection of MIDI controllers on startup
- Lock-free SPSC ring buffer for MIDI-to-audio routing
- Sample-accurate MIDI event processing
- Note-on/off, CC, pitch bend support
- CC 120 (All Sound Off) and CC 123 (All Notes Off) handling
- Per-track MIDI routing — only the selected track receives input

**TUI**
- Solarized-dark theme with phosphor CRT aesthetic
- Vim-style navigation (j/k/h/l, Enter, Esc)
- Space menu (spacevim-inspired leader key)
- Per-track color coding, VU meters, mute/solo/arm controls
- Synth parameter panel with real-time adjustment
- FX chain display with tabs (Track FX, Synth, Clip FX)
- Clip view with piano roll and FX panel
- Send A/B buses and master track
- Scroll support for tracks and parameters

**Architecture**
- Workspace with 6 crates, clean dependency graph
- Shared domain models via atomics (no locks between threads)
- Command channel pattern for UI-to-audio communication
- Plugin trait for instruments and effects
- 113+ tests covering DSP, MIDI, engine, mixer, and navigation

---

## Architecture

```
                    UI Thread                              Audio Thread
                    ─────────                              ────────────
                    NavState                               Mixer
                      │                                      │
                      ├─ TrackState ──Arc<TrackHandle>──→ AudioTrack
                      │    muted ───→ TrackConfig.muted      │
                      │    soloed ──→ TrackConfig.soloed      ├─ instrument: Box<dyn Plugin>
                      │    volume ──→ TrackConfig.volume      ├─ buf_l / buf_r
                      │    VU ←──── TrackHandle.vu ←──────── └─ per-track VU
                      │
                      └─ MixerCommand ──crossbeam──→ Mixer.drain_commands()
                           AddTrack                     → tracks.push()
                           SetInstrument                → track.instrument = Some(plugin)
                           SetParameter                 → plugin.set_parameter()

MIDI Controller ──midir──→ MidiRingSender ──SPSC──→ MidiRingReceiver
                                                        │
                                                   EngineAudio.process()
                                                        │
                                                   Mixer.process()
                                                        │
                                                   cpal audio callback ──→ speakers
```

---

## Controls

### Global

| Key | Action |
|-----|--------|
| `Space` | Open command menu |
| `Space` `Space` | Close command menu |
| `Ctrl+C` | Quit |
| `Tab` | Cycle between panes |
| `Esc` | Back / close menu |

### Space Menu

| Key | Action |
|-----|--------|
| `Space` `1` | Focus tracks pane |
| `Space` `2` | Focus clip view pane |
| `Space` `p` | Play / pause |
| `Space` `r` | Toggle recording |
| `Space` `l` | Toggle loop |
| `Space` `!` | Panic — kill all sound |
| `Space` `a` | Add instrument track |
| `Space` `h` | Help topics |

### Tracks Pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate between tracks |
| `Enter` | Select track (shows synth controls) |
| `h` / `l` | Navigate track elements (fx, vol, mute, solo, arm, clips) |
| `m` | Toggle mute |
| `s` | Toggle solo |
| `r` | Toggle record arm |
| `+` / `-` | Adjust BPM |
| `q` | Quit (when no track selected) |

### Clip View / Synth Controls

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate parameters |
| `h` / `l` | Decrease / increase parameter value (5% steps) |
| `Tab` | Cycle tabs (Track FX / Synth / Clip FX) |
| `Esc` | Back to tracks |

---

## Synth Parameters

The Phosphor Synth exposes 12 parameters, all adjustable in real-time:

| Parameter | Range | Description |
|-----------|-------|-------------|
| waveform | sine / saw / square / tri | Oscillator waveform shape |
| detune | 0–50 cents | Dual oscillator detuning for analog fatness |
| sub | 0–100% | Sub oscillator level (one octave down, sine) |
| noise | 0–100% | White noise mix for breath and texture |
| cutoff | 20Hz–20kHz | Low-pass filter cutoff frequency |
| reso | 0–95% | Filter resonance |
| drive | 0–100% | Soft-clip saturation / overdrive |
| attack | 0–2000ms | Envelope attack time |
| decay | 0–2000ms | Envelope decay time |
| sustain | 0–100% | Envelope sustain level |
| release | 0–2000ms | Envelope release time |
| gain | 0–100% | Output level |

**Vintage sound tips:**
- Saw wave + detune 15-25 cents + sub 30% = classic analog pad
- Square wave + cutoff 40% + reso 60% + drive 30% = acid bass
- Triangle + slow attack + long release + noise 10% = ambient texture
- Saw + cutoff 20% + reso 80% = filtered sweep (automate cutoff)

---

## Building from Source

### Requirements

- Rust 1.75+ (install via [rustup](https://rustup.rs))
- System audio libraries:
  - **macOS**: CoreAudio (included with Xcode)
  - **Linux**: ALSA (`sudo apt install libasound2-dev`) and optionally JACK
  - **Windows**: WASAPI (included)
- MIDI support requires a connected MIDI device (optional)

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test --workspace
```

### Benchmarks

```bash
cargo bench --workspace
```

---

## Project Structure

```
phosphor/
├── Cargo.toml                 # Workspace root
├── src/main.rs                # CLI entry point
├── crates/
│   ├── phosphor-core/         # Audio engine, mixer, transport, project models
│   │   ├── src/
│   │   │   ├── engine.rs      # Audio callback, VU levels
│   │   │   ├── mixer.rs       # Per-track processing, MIDI routing
│   │   │   ├── project.rs     # Shared domain models (TrackConfig, TrackHandle)
│   │   │   ├── transport.rs   # Play/pause/loop with atomics
│   │   │   ├── cpal_backend.rs# Real audio I/O
│   │   │   └── audio.rs       # Test audio backend
│   │   └── benches/
│   ├── phosphor-dsp/          # Built-in DSP and instruments
│   │   └── src/
│   │       ├── synth.rs       # Polyphonic subtractive synth (Plugin impl)
│   │       └── oscillator.rs  # Waveform oscillators
│   ├── phosphor-midi/         # MIDI I/O and message handling
│   │   └── src/
│   │       ├── message.rs     # MIDI message parsing
│   │       ├── ring.rs        # Lock-free SPSC ring buffer
│   │       └── ports.rs       # Port enumeration and hot-plug detection
│   ├── phosphor-plugin/       # Plugin trait definitions
│   │   └── src/lib.rs         # Plugin, MidiEvent, ParameterInfo traits
│   ├── phosphor-tui/          # Terminal UI frontend
│   │   └── src/
│   │       ├── app.rs         # Application loop, audio/MIDI wiring
│   │       ├── state.rs       # Navigation, track state, modals
│   │       ├── ui.rs          # Rendering
│   │       └── theme.rs       # Color palette
│   └── phosphor-gui/          # GUI frontend (planned)
└── architect.md               # Architecture plan and roadmap
```

---

## Configuration

### CLI Options

```
phosphor [OPTIONS]

Options:
    --tui                 Launch TUI frontend (default)
    --gui                 Launch GUI frontend (not yet implemented)
    --buffer-size <N>     Audio buffer size in samples [default: 64]
    --sample-rate <N>     Sample rate in Hz [default: 44100]
    --no-audio            Disable audio output
    --no-midi             Disable MIDI input
    -h, --help            Print help
    -V, --version         Print version
```

### Latency Tuning

Lower buffer sizes reduce latency but increase CPU load:

| Buffer Size | Latency @ 44.1kHz | Use Case |
|------------|-------------------|----------|
| 32 | 0.7ms | Low-latency monitoring |
| 64 | 1.5ms | Default, good balance |
| 128 | 2.9ms | Lighter CPU load |
| 256 | 5.8ms | Complex projects |

---

## Contributing

Phosphor uses a modular plugin architecture. The `Plugin` trait in `phosphor-plugin` is the contract for all instruments and effects:

```rust
pub trait Plugin: Send {
    fn info(&self) -> PluginInfo;
    fn init(&mut self, sample_rate: f64, max_buffer_size: usize);
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], midi_events: &[MidiEvent]);
    fn parameter_count(&self) -> usize;
    fn parameter_info(&self, index: usize) -> Option<ParameterInfo>;
    fn get_parameter(&self, index: usize) -> f32;
    fn set_parameter(&mut self, index: usize, value: f32);
    fn reset(&mut self);
}
```

To add a new instrument or effect:

1. Create a struct that implements `Plugin`
2. Add it to `phosphor-dsp` (or your own crate)
3. Register it in the instrument selection modal

---

## License

MIT
