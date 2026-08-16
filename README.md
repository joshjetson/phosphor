<p align="center">
  <img src="https://i.imgur.com/6oA9IPf.png" alt="Phosphor" width="680"/>
</p>

<p align="center">
  <strong>A terminal-native DAW built in Rust</strong><br/>
  6 built-in synthesizers, 15 drum kits, 300+ patches, 9 color themes, animated splash screen, session save/load, undo/redo, and a plugin system designed for extensibility.
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
| **DX7** | FM | 16 | **256** | All 8 original factory cartridges, decoded from the ROM dumps |
| **Jupiter-8** | Analog poly | 8 | **64** | All 64 factory patch names, full 32-control panel, two envelopes |
| **ARP Odyssey** | Duophonic | 2 | 44 | 3 selectable filter types (4023/4035/4075), hard sync, ring mod, S&H |
| **Juno-60** | DCO poly | 6 | **56** | All 56 factory patches, read off Roland's patch charts; complete 25-control front panel measured against the hardware |

### Drum Rack

| Kit | Character |
|-----|-----------|
| **808** | Rebuilt on the service-notes circuit values — 49.4 Hz bridged-T kick, 238/476 Hz snare, one shared free-running six-oscillator metal bank |
| **909** | The hybrid it really is — analog kick/snare/toms/rim/clap, and hi-hat, ride and crash as 6-bit 18 kHz samples |
| **707** | A PCM machine, not an analog one — sampled character through post-converter analog envelopes |
| **606** | Its own seven analog voices, built from the service-notes component values |
| **777** | 808/909 bass + creative FM/ring-mod/wavefolder sounds |
| **tsty-1** | Warm vintage, tape-saturated, reel-to-reel character |
| **tsty-2** | Acoustic modal — Bessel membrane modes, multi-phase envelopes |
| **tsty-3** | 88 unique sounds — every note a distinct synthesis |
| **tsty-4** | Extended hats/snares with long decays, varied synthesis methods |
| **tsty-5** | Resonator-based — impulse exciter into tuned bandpass filters, wire-coupled snares |
| **LinnDrum** | 15 recordings through a mu-255 companded 8-bit converter; tuning is the read clock, so pitch and length move together |
| **DMX** | 11 recordings making 15 sounds, companded 8-bit, per-card pitch trimmer at half an octave |
| **SDS-V** | Five analog modules — triangle VCO, noise and click through a 4-pole SSM2044, ramp VCAs that stop rather than fade |
| **727** | The 707's converter with the Latin voice set: congas, bongos, timbales, agogo, cabasa, maracas, whistles, quijada, star chime |
| **CR-78** | Pre-808 Roland analog — a snare with no oscillator in it at all, one LC band-pass for every metal voice, and the metallic beat |

### Patch Highlights

**DX7** (256 voices): every voice from the eight original factory cartridges —
ROM1A/1B, ROM2A/2B, ROM3A/3B, ROM4A/4B — decoded from the ROM sysex dumps rather
than recreated. That includes the ones that defined the instrument: `E.PIANO 1`,
`BASS    1`, `TUB BELLS`, `BRASS   1`, `STRINGS 1`, `HARPSICH 1`, and the novelty
voices Yamaha shipped alongside them (`TAKE OFF`, `WASP STING`, `..GOTCHA..`).
Pick a cartridge with the `bank` parameter, then a voice with `patch`.


**Jupiter-8** (64 patches): the factory bank in Roland's own 8x8 numbering — `11 NEG SYNC`,
`13 JUICY FUNK`, `15 CARS SYNC`, `17 HAMMER LEAD`, `24 MELLOW RHODES`, `31 LO STRINGS`,
`45 PIPE ORGAN`, `51 TRAIN CHUG`, `57 TOMITA CHIME`, `63 KLINGONS`, `64 MUSIC OF THE SPHERES`,
`66 SOLAR WINDS`, `71 FAT FIFTHS`, `87 UPRIGHT BASS` and the rest. Names, numbers, voice modes
and character follow the original factory patch sheets; the parameter values are voiced to match
Roland's published description of each patch.


**Odyssey** (44 patches): Bass, Funk, Sync Lead, Bells, Pad, S&H, Zap, Hawkshaw Funk, Bennett Atmos, Numan Cars, Sci-Fi Wobble, Percussive Pluck, Thick Lead, Filter Sweep, Noise Hit, Duo Split, Snare Drum, Kick, Resonance, Squelch, Growl, Wind, Wah Bass, Stab, Buzz, Flute, Tremolo, Siren, Brass, Organ, Conga, Tom, Clap, PWM Bass, Violin, Oboe, Choir, Trombone, Marimba, Alarm, Robot, Whistler, Sitar, Theremin

**Juno-60** (56 factory patches, in the instrument's own seven banks of eight):

- **Bank 1** — 11 Strings 1, 12 Strings 2, 13 Strings 3, 14 Organ 1, 15 Organ 2, 16 Organ 3, 17 Brass, 18 Phase Brass
- **Bank 2** — 21 Piano 1, 22 Piano 2, 23 Celesta, 24 Mellow Piano, 25 Harpsichord 1, 26 Harpsichord 2, 27 Guitar, 28 Synthesizer Harp
- **Bank 3** — 31 Bass 1, 32 Bass 2, 33 Clavichord 1, 34 Clavichord 2, 35 Pizzicato Sound 1, 36 Pizzicato Sound 2, 37 Xylophone, 38 Glockenspeil
- **Bank 4** — 41 Violine, 42 Trumpet, 43 Horn, 44 Tuba, 45 Flute, 46 Clarinet, 47 Oboe, 48 English Horn
- **Bank 5** — 51 Funny Cat, 52 Wah Brass, 53 Phase Combination, 54 Reed 1, 55 Popcorn, 56 Reed 2, 57 Reed 3, 58 PWM Chorus
- **Bank 6** — 61 Synthesizer Organ, 62 Effect Sound 1, 63 Effect Sound 2, 64 Space Harp, 65 Funk, 66 Space Sound 1, 67 Mysterious Invention, 68 Space Sound 2
- **Bank 7** — 71 Percussive Sound 1, 72 Percussive Sound 2, 73 Whistle, 74 Effect Sound 3, 75 UFO, 76 Space Sound 3, 77 Surf, 78 Synthesizer Drum — the bank whose sound source is the VCF oscillating on its own

Names and spellings are Roland's, Glockenspeil included.

---

## Features

**Audio Engine**
- Real-time audio via cpal (CoreAudio, WASAPI, ALSA)
- Lock-free audio thread — zero allocations, zero mutexes in the hot path
- Per-track instrument instances with independent processing
- Per-track and master VU metering via atomic shared state, on a dB scale
- Configurable buffer size (default 64 samples, ~1.5ms latency at 44.1kHz)
- Gain-staged for chords, not single notes — every instrument is sized so a
  two-handed voicing at full velocity still has headroom
- Soft saturation on each instrument, transparent below its knee, replacing the
  hard clip that used to turn loud chords into a square wave
- Stereo-linked master limiter at -1 dBFS with a non-finite guard, so nothing
  above full scale and no NaN can ever reach the audio device

**Synthesizers**
- **Phosphor Synth**: 16-voice polyphonic subtractive — dual oscillators, SVF filter, drive, ADSR
- **DX7**: all 256 original factory voices, decoded from the ROM cartridge dumps and played on a 6-operator engine modelled on the YM21280/YM21290 chipset — all 32 algorithms decoded from the hardware table (including the multi-operator feedback loops in algorithms 4 and 6), log-domain envelopes with the hardware rate curve and its distinct attack shape, coarse/fine/detune frequency on the real parameter grid, keyboard level and rate scaling, global LFO with six waveforms and two-stage delay, and a per-voice pitch envelope
- **Jupiter-8**: the full front panel — dual VCOs with sync and exponential cross-modulation, switchable 12/24 dB IR3109 filter with resonance to self-oscillation, non-resonant HPF, two independent ADSR envelopes, LFO with four waveforms and a two-stage delay, portamento, and 4 voice modes (Solo/Unison/Poly1/Poly2). Envelope times follow Roland's published 1 ms-10 s specification; filter corners, LFO taper and keyboard follow are measured rather than approximated
- **ARP Odyssey**: Duophonic split, 3 selectable filters (12dB SVF / 24dB Moog ladder / 24dB Norton), XOR ring mod, hard sync, Sample & Hold
- **Juno-60**: the full front panel — LFO rate/delay, DCO with PWM depth and a 3-position PWM mode (LFO/MANUAL/ENV), saw/pulse/sub/noise and a 16'/8'/4' range switch, 4-position HPF, IR3109-style 24 dB/oct resonant VCF with env polarity, LFO and keyboard follow, ENV/GATE VCA, shared ADSR, and BBD stereo chorus (I / II / I+II). Envelope taper, LFO rate taper, filter corner frequencies and chorus rates are calibrated against measurements of the hardware rather than approximated. All 56 factory patches are the instrument's own, transcribed from Roland's published patch charts
- **Drum Rack**: 15 kits including circuit-accurate 808/909/707/606, creative 777, warm tape-saturated tsty series, and resonator-based physical modeling

**Session Management**
- Save/load projects as `.phos` files (human-readable JSON)
- `Ctrl+S` quick save, `Space+S` save as, `Space+O` open
- Saves all tracks, instruments, synth parameters, clips, MIDI notes, transport settings
- Atomic writes prevent file corruption
- Default save directory: `sessions/`

**User Presets**
- `Space+W` opens a preset browser for the selected instrument
- Every instrument has its own bank, the drum rack included — the whole parameter
  block, including the factory patch it was dialled in from
- One human-readable file per instrument (`~/.phosphor/presets/<instrument>.json`),
  atomic writes, so a DX7 preset can never be offered to a Juno
- Presets sit beside the factory tables rather than extending them, so adding one
  cannot move a patch index stored in a saved session
- A preset saved against a different panel — wrong instrument, wrong number of
  controls, or an older layout — is refused rather than loaded into the wrong holes
- 128 presets per instrument, 32 characters per name

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
- Note-level edit mode with per-note select, move, transpose, and stretch
- Variable-strength quantize (25–100%) with grid resolution selection
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
- 496 tests covering DSP, MIDI, engine, mixer, navigation, and persistence

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
| `Space` `e` | Enter edit mode (note-level piano roll editing) |
| `Space` `q` | Quantize notes to grid |
| `Space` `w` | Instrument presets — save / load / delete |
| `Space` `v` | Cycle color theme |
| `Space` `h` | Open help topics |

### Preset Browser (Space+W, on an instrument track)

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate rows |
| `Enter` on the top row | Name and save the current panel |
| `Enter` on a preset | Load it into the track |
| `d` | Delete the selected preset (`y`/`n`) |
| `Esc` | Close the browser |

Every instrument has its own bank, including the drum rack — the 35 controls behind
a kit are exactly what a factory table cannot hold. A preset is the whole parameter
block as it stands, including the factory patch it was dialled in from, so loading
one puts the panel back exactly where it was.

Names are slots: saving under a name the bank already holds rewrites that preset,
after a confirmation, rather than adding a second row you cannot tell from the first.
128 presets per instrument, 32 characters per name.

A preset saved for a different instrument, with a different number of controls, or
against an older panel layout is **refused** rather than loaded — a block that does
not fit the panel would produce a plausible sound that is not the one that was saved.
The reason appears in the status bar.

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

### Volume Fader (navigate to `vol` with `h/l`, then `Enter` to lock)

| Key | Action |
|-----|--------|
| `Enter` | Lock the fader |
| `h` / `l` | Down / up by 1 dB |
| `Esc` / `Enter` | Release the fader |

The fader reads out in dB relative to unity — `0` at unity, `+6` at the top, `-oo` at
the bottom. New tracks start at `-2`. Unity is not the maximum: there is 6 dB of
makeup gain above it, which is where to reach when a quiet patch needs to sit forward
in a mix.

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

### Piano Roll — Edit Mode (Space+E)

Note-level editing. Where the Right Left Trick operates on whole columns, edit mode moves a cursor between individual notes.

**Navigate**

| Key | Action |
|-----|--------|
| `j` / `k` | Move to next note up/down within the same column |
| `h` / `l` | Jump to nearest note in previous/next column |
| `Enter` | Select cursor note for moving |
| `d` | Delete cursor note |
| `u` | Undo |
| `Esc` / `e` | Exit edit mode |

**Select** (triggered by `Shift`+direction from navigate)

| Key | Action |
|-----|--------|
| `Shift+J` / `Shift+K` | Extend selection up/down within the column |
| `Shift+H` / `Shift+L` | Extend selection to previous/next column |
| `d` | Delete all selected notes |
| `h` / `j` / `k` / `l` | Begin moving the selection |
| `Esc` | Clear selection, back to navigate |

**Move** (after selecting, or `Enter` on a single note)

| Key | Action |
|-----|--------|
| `h` / `l` | Move selected notes left/right by one grid step |
| `j` / `k` | Transpose selected notes down/up by a semitone |
| `Shift+H` / `Shift+L` | Stretch the right edge (duration) |
| `Shift+J` / `Shift+K` | Stretch the left edge (start position) |
| `d` | Delete all selected notes |
| `Esc` | Lock notes in place, clear selection |

### Quantize (Space+Q)

Opens a modal that snaps the selected clip's notes to the grid. Requires a selected clip.

| Key | Action |
|-----|--------|
| `j` / `k` | Move between rows (grid, strength, apply) |
| `h` / `l` | Adjust the selected value |
| `Enter` | Apply quantize (when on the apply button) |
| `Esc` | Close without applying |

Strength runs from 25% to 100%. At 100% notes land exactly on the grid; below that they move proportionally toward it, so you can tighten a performance without flattening its feel. Quantize is a single undoable action — `u` restores the original positions.

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
cargo test --workspace  # 496 tests
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
│   │       ├── juno.rs        # Juno-60 DCO + BBD chorus (56 factory patches)
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
