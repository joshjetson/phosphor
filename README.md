<p align="center">
  <img src="https://i.imgur.com/6oA9IPf.png" alt="Phosphor" width="680"/>
</p>

<p align="center">
  <strong>A terminal-native DAW built in Rust</strong><br/>
  6 built-in synthesizers, 10 drum kits, 300+ patches, MIDI controller auto-detection, and a plugin system designed for extensibility.
</p>

<p align="center">
  <img src="https://i.imgur.com/1Ia9OH2.png" alt="Phosphor UI" width="680"/>
</p>

---

## Index

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Instruments](#instruments)
- [Features](#features)
- [Architecture](#architecture)
- [Controls](#controls)
- [Building from Source](#building-from-source)
- [Project Structure](#project-structure)
- [Configuration](#configuration)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

Phosphor is a digital audio workstation that runs entirely in your terminal. It pairs a solarized-dark TUI with a real-time audio engine, giving you a DAW you can use over SSH, in a tiling window manager, or anywhere a terminal lives.

Each instrument track gets its own synthesizer instance with independent parameters. MIDI controllers are detected automatically on startup. The audio engine runs on a dedicated real-time thread with lock-free communication — no mutexes in the audio path, ever.

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

# Run with debug logging
PHOSPHOR_DEBUG=1 cargo run --release

# Run without audio (UI development)
cargo run --release -- --no-audio

# Run without MIDI
cargo run --release -- --no-midi
```

**First steps once running:**

1. Press `Space` to open the command menu
2. Press `a` to add an instrument track
3. Select an instrument and press `Enter`
4. Play your MIDI controller — sound comes out
5. Use `j/k` to navigate synth parameters, `h/l` to adjust values
6. Press `Tab` to cycle between Track FX, Synth params, Inst Config, Piano Roll

---

## Instruments

### Synthesizers

| Instrument | Type | Voices | Patches | Description |
|-----------|------|--------|---------|-------------|
| **Phosphor Synth** | Subtractive | 16 | 4 waveforms | Dual oscillators, SVF filter, drive, ADSR |
| **DX7** | FM | 16 | 51 | 6-operator FM, 32 algorithms, classic ROM patches |
| **Jupiter-8** | Analog poly | 8 | 42 | Dual polyBLEP VCOs, IR3109 OTA ladder filter, 4 voice modes |
| **ARP Odyssey** | Duophonic | 2 | 44 | 3 selectable filter types (4023/4035/4075), hard sync, ring mod, S&H |
| **Juno-60** | DCO poly | 6 | 18 | Single DCO, BBD stereo chorus (I/II/I+II), sub-oscillator |

### Drum Rack

| Kit | Character |
|-----|-----------|
| **808** | Circuit-accurate analog — sine kicks, 6-osc metallic hats |
| **909** | Triangle snares, bit-crushed hats, longer pitch sweeps |
| **707** | Hybrid 808/909 character |
| **606** | Thinner, clickier, higher frequencies |
| **777** | 808/909 bass + creative FM/ring-mod/wavefolder sounds |
| **tsty-1** | Warm vintage, tape-saturated, reel-to-reel character |
| **tsty-2** | Acoustic modal — Bessel membrane modes, multi-phase envelopes |
| **tsty-3** | 88 unique sounds — every note a distinct synthesis |
| **tsty-4** | Extended hats/snares with long decays, varied synthesis methods |
| **tsty-5** | Resonator-based — impulse exciter into tuned bandpass filters, wire-coupled snares |

### Patch Highlights

**DX7** (51 patches): E.Piano, Bass, Brass, Bells, Organ, Strings, Flute, Harpsichord, Marimba, Clavinet, Tubular Bells, Vibraphone, Koto, Synth Lead, Choir, Harmonica, Kalimba, Sitar, Oboe, Clarinet, Trumpet, Glockenspiel, Xylophone, Steel Pan, Slap Bass, Fretless Bass, Crystal, Ice Rain, Synth Pad, Digital Pad, Cello, Pizzicato, Log Drum, Tinkle Bell, Shakuhachi, Synth Brass, Voices, E.Piano 2, Accordion, Harp, Clav 2, Banjo, Guitar, Piano, Celeste, Cowbell, Synth Bass, Timpani, Pan Flute, Horns, Toy Piano

**Jupiter-8** (42 patches): Pad, Brass, Bass, Sync Lead, Strings, Electric Piano, Pluck, Bell, Organ, PWM Pad, Unison Lead, Key Bass, Ambient, Sweep, Stab, Harp, Sync Bass, Sub Bass, Acid, Choir, Vox, Whistle, PWM Lead, XM Bell, Sequence, Resonant, Detune, Clav, Hollow Pad, Power Pluck, Lo Strings, Flute, Tuba, Saw Pad, Clarinet, Cello, Xylo, Funk Bass, Warm Lead, Noise, Cars Sync, and more

**Odyssey** (44 patches): Bass, Funk, Sync Lead, Bells, Pad, S&H, Zap, Hawkshaw Funk, Bennett Atmos, Numan Cars, Sci-Fi Wobble, Percussive Pluck, Thick Lead, Filter Sweep, Noise Hit, Duo Split, Snare Drum, Kick, Resonance, Squelch, Growl, Wind, Wah Bass, Stab, Buzz, Flute, Tremolo, Siren, Brass, Organ, Conga, Tom, Clap, PWM Bass, Violin, Oboe, Choir, Trombone, Marimba, Alarm, Robot, Whistler, Sitar, Theremin

**Juno-60** (18 patches): Classic Pad, PWM Pad, Bass, Brass, Strings, Hoover, Acid, Warm Lead, Choir, Pluck, Organ, Synth Bass, Glass Bells, Resonant Pad, Wind, Clav, Sub Bass, Saw Pad

---

## Features

**Audio Engine**
- Real-time audio via cpal (CoreAudio, WASAPI, ALSA)
- Lock-free audio thread — zero allocations, zero mutexes in the hot path
- Per-track instrument instances with independent processing
- Per-track and master VU metering via atomic shared state
- Configurable buffer size (default 64 samples, ~1.5ms latency at 44.1kHz)

**Synthesizers**
- **Phosphor Synth**: 16-voice polyphonic subtractive — dual oscillators, SVF filter, drive, ADSR
- **DX7**: 6-operator FM synthesis, all 32 algorithms, 4-rate/4-level envelopes, operator feedback
- **Jupiter-8**: Dual polyBLEP VCOs, IR3109 4-pole OTA ladder filter with tanh saturation, per-voice analog drift, 4 voice modes (Solo/Unison/Poly1/Poly2)
- **ARP Odyssey**: Duophonic split, 3 selectable filters (12dB SVF / 24dB Moog ladder / 24dB Norton), XOR ring mod, hard sync, Sample & Hold
- **Juno-60**: Single DCO per voice, BBD stereo chorus (Chorus I / II / I+II), sub-oscillator, 4-position HPF, single ADSR shared VCF+VCA
- **Drum Rack**: 10 kits including circuit-accurate 808/909/707/606, creative 777, warm tape-saturated tsty series, and resonator-based physical modeling

**MIDI**
- Auto-detection of MIDI controllers on startup
- Lock-free SPSC ring buffer for MIDI-to-audio routing
- Sample-accurate MIDI event processing
- Note-on/off, CC, pitch bend support
- Per-track MIDI routing — only the selected track receives input

**TUI**
- Solarized-dark theme with phosphor CRT aesthetic
- Vim-style navigation (j/k/h/l, Enter, Esc)
- Space menu (spacevim-inspired leader key)
- Per-track color coding, VU meters, mute/solo/arm controls
- Synth parameter panel with real-time adjustment and patch selection
- Instrument config tab for deeper parameter access
- Clip view with piano roll, automation, and FX panel
- Transport with BPM, loop region, metronome, recording
- Send A/B buses and master track

**Architecture**
- Workspace with 7 crates, clean dependency graph
- Shared domain models via atomics (no locks between threads)
- Command channel pattern for UI-to-audio communication
- Plugin trait for instruments and effects — same interface for built-in and third-party
- 214+ tests covering DSP, MIDI, engine, mixer, and navigation

---

## Architecture

```
                    UI Thread                              Audio Thread
                    ---------                              ------------
                    NavState                               Mixer
                      |                                      |
                      +-- TrackState --Arc<TrackHandle>--> AudioTrack
                      |    muted ---> TrackConfig.muted      |
                      |    soloed --> TrackConfig.soloed      +-- instrument: Box<dyn Plugin>
                      |    volume --> TrackConfig.volume      +-- buf_l / buf_r
                      |    VU <----- TrackHandle.vu <------- +-- per-track VU
                      |
                      +-- MixerCommand --crossbeam--> Mixer.drain_commands()
                           AddTrack                     -> tracks.push()
                           SetInstrument                -> track.instrument = Some(plugin)
                           SetParameter                 -> plugin.set_parameter()

MIDI Controller --midir--> MidiRingSender --SPSC--> MidiRingReceiver
                                                        |
                                                   EngineAudio.process()
                                                        |
                                                   Mixer.process()
                                                        |
                                                   cpal audio callback --> speakers
```

---

## Controls

### Global

| Key | Action |
|-----|--------|
| `Space` | Open command menu |
| `Ctrl+C` | Quit |
| `Tab` | Cycle between panes / tabs |
| `Esc` | Back / close menu |

### Space Menu

| Key | Action |
|-----|--------|
| `Space` `1` | Focus transport |
| `Space` `2` | Focus tracks |
| `Space` `3` | Focus clip view |
| `Space` `p` | Play / pause |
| `Space` `r` | Toggle recording |
| `Space` `l` | Edit loop region |
| `Space` `m` | Toggle metronome |
| `Space` `!` | Panic — kill all sound |
| `Space` `a` | Add instrument track |

### Tracks Pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate between tracks |
| `Enter` | Select track |
| `h` / `l` | Navigate track elements |
| `m` | Toggle mute |
| `s` | Toggle solo |
| `r` | Toggle record arm |

### Clip View

| Key | Action |
|-----|--------|
| `Tab` | Cycle tabs: Track FX / Synth / Inst Config / Piano / Auto |
| `j` / `k` | Navigate parameters or piano roll |
| `h` / `l` | Adjust values or navigate columns |
| `Enter` | Select column / note |
| `n` | Draw note in piano roll |

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
cargo test --workspace  # 214+ tests
```

---

## Project Structure

```
phosphor/
├── Cargo.toml                 # Workspace root (phosphor-studio on crates.io)
├── src/main.rs                # CLI entry point
├── crates/
│   ├── phosphor-core/         # Audio engine, mixer, transport, metronome
│   ├── phosphor-dsp/          # Built-in instruments
│   │   └── src/
│   │       ├── synth.rs       # Phosphor Synth (subtractive)
│   │       ├── dx7.rs         # DX7 FM synthesizer (51 patches)
│   │       ├── jupiter.rs     # Jupiter-8 analog poly (42 patches)
│   │       ├── odyssey.rs     # ARP Odyssey duophonic (44 patches)
│   │       ├── juno.rs        # Juno-60 DCO + BBD chorus (18 patches)
│   │       ├── drum_rack.rs   # Drum machine (10 kits, 88 sounds each)
│   │       └── oscillator.rs  # Waveform oscillators
│   ├── phosphor-midi/         # MIDI I/O, message parsing, ring buffer
│   ├── phosphor-plugin/       # Plugin trait definitions
│   ├── phosphor-tui/          # Terminal UI frontend
│   │   └── src/
│   │       ├── app.rs         # Application loop, audio/MIDI wiring
│   │       ├── state/         # Navigation, track state, clip view, modals
│   │       ├── ui.rs          # Rendering
│   │       └── theme.rs       # Solarized-dark color palette
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

### Debug Logging

```bash
PHOSPHOR_DEBUG=1 cargo run --release
```

Creates `phosphor_debug.log` with timestamped user actions and system responses. Includes a panic handler that captures full backtraces to the log.

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

To add a new instrument:

1. Create a struct that implements `Plugin`
2. Add it to `phosphor-dsp` (or your own crate)
3. Add the variant to `InstrumentType` in `phosphor-tui/src/state/menu.rs`
4. Wire it into `create_instrument_track()` in `app.rs`

---

## License

MIT
