<p align="center">
  <img src="https://i.imgur.com/6oA9IPf.png" alt="Phosphor" width="680"/>
</p>

<p align="center">
  <strong>A terminal-native DAW built in Rust</strong><br/>
  6 built-in synthesizers, 10 drum kits, 300+ patches, 9 color themes, animated splash screen, session save/load, undo/redo, and a plugin system designed for extensibility.
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
- [Controls](#controls)
- [Themes](#themes)
- [Architecture](#architecture)
- [Building from Source](#building-from-source)
- [Project Structure](#project-structure)
- [Configuration](#configuration)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

Phosphor is a digital audio workstation that runs entirely in your terminal. It pairs a themeable TUI with a real-time audio engine, giving you a DAW you can use over SSH, in a tiling window manager, or anywhere a terminal lives.

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
7. Press `Space` then `v` to change the color theme

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

**Session Management**
- Save/load projects as `.phos` files (human-readable JSON)
- `Ctrl+S` quick save, `Space+S` save as, `Space+O` open
- Saves all tracks, instruments, synth parameters, clips, MIDI notes, transport settings
- Atomic writes prevent file corruption
- Default save directory: `sessions/`

**Undo/Redo**
- `u` undoes the last action, `Ctrl+R` redoes
- Works for: note draw/remove, highlight delete, paste, clip delete, track delete
- Full track restoration on undo (instruments, params, clips, audio routing)
- 100-action undo stack

**Themes**
- 9 built-in color themes (see [Themes](#themes))
- `Space+V` cycles themes instantly
- Theme choice persists across sessions (`~/.phosphor/config.json`)

**MIDI**
- Auto-detection of MIDI controllers on startup
- Lock-free SPSC ring buffer for MIDI-to-audio routing
- Sample-accurate MIDI event processing
- Note-on/off, CC, pitch bend support
- Per-track MIDI routing — only the selected track receives input
- Overdub recording with loop-based merge

**TUI**
- Animated splash screen with shimmering aquamarine/violet dot-matrix art
- 9 color themes with full UI coverage
- Vim-style navigation (j/k/h/l, Enter, Esc)
- Space menu (spacevim-inspired leader key)
- Per-track color coding, VU meters, mute/solo/arm controls
- Synth parameter panel with real-time adjustment and patch selection
- Instrument config tab for deeper parameter access
- Piano roll with horizontal scroll, playhead, column/row highlighting
- Clip locking with move, stretch, trim, and collision detection
- Transport with BPM, loop region, metronome, recording
- Send A/B buses and master track
- Clean terminal restore on exit and panic

**Architecture**
- Workspace with 7 crates, clean dependency graph
- Modular file structure — app, UI, and state split into focused sub-modules
- Shared domain models via atomics (no locks between threads)
- Command channel pattern for UI-to-audio communication
- Plugin trait for instruments and effects — same interface for built-in and third-party
- 216+ tests covering DSP, MIDI, engine, mixer, and navigation

---

## Controls

### Global

| Key | Action |
|-----|--------|
| `Space` | Open command menu |
| `Ctrl+C` | Quit |
| `Ctrl+S` | Quick save session |
| `u` | Undo last action |
| `Ctrl+R` | Redo |
| `Tab` | Cycle between panes / tabs |
| `Esc` | Back / close menu / clear highlights |

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
| `Space` `s` | Save project |
| `Space` `o` | Open project |
| `Space` `d` | Delete selected track/clip (with confirmation) |
| `Space` `v` | Cycle color theme |

### Tracks Pane

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate between tracks |
| `Enter` | Select track (shows synth controls) |
| `h` / `l` | Navigate track elements (fx, vol, mute, solo, arm, clips) |
| `m` | Toggle mute |
| `s` | Toggle solo |
| `r` | Toggle record arm |
| `R` | Toggle loop record |
| `1-9` | Jump to clip by number |

### Clip Operations (navigate to a clip with `h/l`, then `Enter` to lock)

| Key | Action |
|-----|--------|
| `Enter` | Lock to clip (enables move/stretch controls) |
| `h` / `l` | Move clip left/right by one beat |
| `H` / `Shift+Left` | Shrink clip (right edge moves left) |
| `L` / `Shift+Right` | Extend clip (right edge moves right) |
| `Ctrl+H` / `Ctrl+Left` | Trim left edge (start moves right) |
| `Ctrl+L` / `Ctrl+Right` | Extend left edge (start moves left) |
| `y` | Yank (copy) clip |
| `p` | Paste clip after current clip |
| `P` | Paste clip to same position on another track |
| `d` | Duplicate clip (copy + paste next to it) |
| `Esc` | Unlock clip (back to element navigation) |

Clip operations include collision detection — clips cannot overlap. Moving, stretching, and trimming all respect adjacent clip boundaries. Note positions are automatically rescaled when stretching or trimming to preserve their absolute timeline positions. All changes sync to the audio thread in real time.

### Piano Roll — Navigation Mode

| Key | Action |
|-----|--------|
| `h` / `l` | Navigate between columns (beats) |
| `j` / `k` | Scroll up/down through notes |
| `1-9` | Jump to column by number |
| `Enter` | Select column (enter edit mode) |
| `n` | Toggle note at cursor (draw or remove) |
| `Esc` | Clear highlights or exit piano roll |

### Piano Roll — Column/Row Highlighting

| Key | Action |
|-----|--------|
| `Shift+H` / `Shift+Left` | Start/expand column highlight left |
| `Shift+L` / `Shift+Right` | Start/expand column highlight right |
| `Shift+J` / `Shift+Down` | Start/expand row highlight down |
| `Shift+K` / `Shift+Up` | Start/expand row highlight up |
| `d` | Delete notes in highlighted region |
| `y` | Yank (copy) notes in highlighted region |
| `p` | Paste yanked notes at cursor/highlight position |
| `j` / `k` (without shift) | Clear row highlight and move |

### Piano Roll — Column Selected (Right Left Trick)

| Key | Action |
|-----|--------|
| `h` / `l` | Adjust left edge of all notes in column |
| `H` / `L` | Adjust right edge of all notes in column |
| `j` / `k` | Enter row mode (select individual note) |
| `n` | Draw note at cursor position |
| `Esc` | Back to navigation mode |

### Piano Roll — Row Mode (Single Note)

| Key | Action |
|-----|--------|
| `h` / `l` | Adjust left edge of single note |
| `H` / `L` | Adjust right edge of single note |
| `j` / `k` | Move between notes in column |
| `n` | Draw note / toggle note |
| `Esc` | Back to column mode |

### Loop Editor (Space+L)

| Key | Action |
|-----|--------|
| `h` / `l` | Move loop start left/right |
| `H` / `L` | Move loop end left/right |
| `Enter` | Enable/disable loop |
| `Esc` | Exit loop editor |

### Transport (Space+1)

| Key | Action |
|-----|--------|
| `h` / `l` | Navigate transport elements |
| `Enter` | Select element (BPM editing, etc.) |
| `+` / `-` | Adjust BPM |

---

## Themes

9 built-in color themes, cycle with `Space+V`:

| Theme | Description |
|-------|-------------|
| **Phosphor** | Original solarized-dark blue-teal (default) |
| **SpaceVim** | Charcoal background with bright gold accents |
| **Gruvbox** | Warm retro browns and oranges |
| **Midnight** | Deep navy with cool blue and violet |
| **Dracula** | Classic purple/pink/cyan dark theme |
| **Nord** | Arctic polar night with frost blue/teal |
| **Jellybean** | True black with soft pastel accents |
| **Catppuccin** | Mocha variant with mauve/pink/sky pastels |
| **SpaceVim2** | Authentic SpaceVim colorscheme (from SpaceVim.vim) |

Theme choice is saved to `~/.phosphor/config.json` and persists across sessions.

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
cargo test --workspace  # 216+ tests
```

---

## Project Structure

```
phosphor/
├── Cargo.toml                 # Workspace root (phosphor-studio on crates.io)
├── src/main.rs                # CLI entry point
├── sessions/                  # Default save directory for .phos files
├── crates/
│   ├── phosphor-core/         # Audio engine, mixer, transport, metronome
│   ├── phosphor-dsp/          # Built-in instruments
│   │   └── src/
│   │       ├── synth.rs       # Phosphor Synth (subtractive)
│   │       ├── dx7.rs         # DX7 FM synthesizer (51 patches)
│   │       ├── jupiter.rs     # Jupiter-8 analog poly (42 patches)
│   │       ├── odyssey.rs     # ARP Odyssey duophonic (44 patches)
│   │       ├── juno.rs        # Juno-60 DCO + BBD chorus (18 patches)
│   │       ├── drum_rack/     # Drum machine (10 kits)
│   │       │   ├── mod.rs     # Shared types, voice, plugin impl
│   │       │   └── racks/     # Per-kit synthesis (808, 909, 707, 606, 777, tsty1-5)
│   │       └── oscillator.rs  # Waveform oscillators
│   ├── phosphor-midi/         # MIDI I/O, message parsing, ring buffer
│   ├── phosphor-plugin/       # Plugin trait definitions
│   ├── phosphor-tui/          # Terminal UI frontend
│   │   └── src/
│   │       ├── app/           # Application logic
│   │       │   ├── mod.rs     # App struct, main loop
│   │       │   ├── keys.rs    # Keyboard event handling
│   │       │   ├── piano_roll.rs  # Note editing, yank/paste
│   │       │   ├── clips.rs   # Clip manipulation (move, stretch, duplicate)
│   │       │   ├── tracks.rs  # Track creation, space actions
│   │       │   ├── transport.rs   # Playback, recording, loop sync
│   │       │   ├── delete.rs  # Delete with confirmation
│   │       │   ├── undo_redo.rs   # Undo/redo system
│   │       │   └── session_io.rs  # Save/load .phos files
│   │       ├── state/         # Navigation state
│   │       │   ├── mod.rs     # NavState struct, accessors
│   │       │   ├── navigation.rs  # Pane focus, movement, tabs
│   │       │   ├── params.rs  # Synth parameter adjustment
│   │       │   ├── track_ops.rs   # Track management, clip recording
│   │       │   ├── clip_view.rs   # Piano roll state, highlights
│   │       │   ├── menu.rs    # Menus, modals, instrument types
│   │       │   ├── undo.rs    # Undo action definitions
│   │       │   └── ...        # Loop editor, transport UI, etc.
│   │       ├── ui/            # Rendering
│   │       │   ├── mod.rs     # Layout orchestration
│   │       │   ├── top_bar.rs # Transport display
│   │       │   ├── tracks.rs  # Track rows, clip grid
│   │       │   ├── clip_view.rs   # Piano roll, FX panel, inst config
│   │       │   ├── overlays.rs    # Menus, modals, confirmations
│   │       │   └── bottom_bar.rs  # Key hints
│   │       ├── session.rs     # Session file format
│   │       ├── splash.rs      # Animated splash screen
│   │       └── theme.rs       # 9 color themes
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

### Theme Persistence

Theme selection is saved to `~/.phosphor/config.json` and automatically loaded on startup.

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
4. Wire it into `create_instrument_track()` in `app/tracks.rs`

---

## License

MIT
