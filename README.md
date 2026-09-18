<p align="center">
  <img src="assets/banner.svg" alt="Phosphor — a terminal-native DAW built in Rust" width="900"/>
</p>

<p align="center">
  <a href="https://github.com/joshjetson/phosphor/actions/workflows/ci.yml"><img src="https://github.com/joshjetson/phosphor/actions/workflows/ci.yml/badge.svg" alt="CI"/></a>
  <a href="https://crates.io/crates/phosphor-studio"><img src="https://img.shields.io/crates/v/phosphor-studio.svg?color=4fe3c0&label=crates.io" alt="crates.io"/></a>
</p>

<p align="center">
  <strong>A terminal-native DAW built in Rust</strong><br/>
  9 built-in synthesizers, 18 drum kits, a sampler, 1,531 patches, 9 color themes, animated splash screen, session save/load, undo/redo, and a plugin system designed for extensibility.
</p>

<p align="center">
  <img src="assets/tracks.svg" alt="The Phosphor track view, animated" width="900"/><br/>
  <sub><em>an animated impression of the track view — the real thing runs in your terminal:</em></sub>
</p>

<p align="center">
  <img src="https://i.imgur.com/1Ia9OH2.png" alt="Phosphor UI screenshot" width="680"/>
</p>

---

## Index

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Instruments](#instruments)
- [Features](#features)
- [Sampler — a sound on every key](#sampler--a-sound-on-every-key)
- [Fingers — the practice room](#fingers--the-practice-room)
- [Shortcuts, A to Z](#shortcuts-a-to-z)
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

Phosphor runs on Windows, macOS and Linux. Everything is installed with
one command — but that command needs the Rust toolchain, and each
platform has one thing to do first. Start with yours:

**Windows** — install Rust from [rustup.rs](https://rustup.rs). The
installer will say it needs the Visual Studio C++ Build Tools and offer
to set them up — say yes; Rust compiles through them on Windows and
nothing works without them. Then open **Windows Terminal** (built into
Windows 11, free in the Microsoft Store on 10) rather than the old
Command Prompt: Phosphor draws its keyboards, meters and braces in
Unicode, and the legacy console mangles them. Audio and MIDI work out of
the box. Your files land in `%APPDATA%\phosphor`.

**macOS** — if you have never compiled anything on this machine, run
`xcode-select --install` once and let it finish. Then install Rust from
[rustup.rs](https://rustup.rs). Your files land in `~/.phosphor`.

**Linux** — install Rust from [rustup.rs](https://rustup.rs), plus the
ALSA headers the audio layer compiles against — without them the install
command fails halfway with a wall of C errors. Debian/Ubuntu:
`sudo apt install build-essential pkg-config libasound2-dev`. Fedora:
`sudo dnf install alsa-lib-devel`. Arch: `sudo pacman -S alsa-lib`.
Your files land in `~/.phosphor`.

Then, on every platform, the same command — and after it finishes, the
program is simply `phosphor` in your terminal:

```bash
# Install from crates.io — the --locked flag is required. It builds with
# the exact dependency versions the release was tested with; without it
# cargo picks newer ones, which now demand a newer Rust than the crate's,
# and the build fails.
cargo install phosphor-studio --locked

# Then run it
phosphor

# Or clone and build — this is how to get the newest version, which may
# be ahead of the one published on crates.io
git clone https://github.com/joshjetson/phosphor.git
cd phosphor
cargo install --path . --locked

# Run (TUI is the default)
cargo run --release

# Run with debug logging (PowerShell on Windows:
#   $env:PHOSPHOR_DEBUG=1; cargo run --release)
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
5. Use `j/k` to navigate synth parameters, `h/l` to adjust values — the
   first one is the patch selector
6. Press `Tab` to reach `[inst]`, the instrument's full panel, laid out in
   columns with room for all of it; `Tab` again for the piano roll
7. Press `Space` then `v` to change the color theme

---

## Instruments

### Synthesizers

| Instrument | Type | Voices | Patches | Description |
|-----------|------|--------|---------|-------------|
| **Phosphor Synth** | Wavetable / vector | 8 | **229** | Four oscillators with vector mixing, 16 wavetables, per-oscillator wave sequencing, Moog-style ladder, 6-slot mod matrix, keymapped drum patches |
| **DX7** | FM | 16 | **256** | All 8 original factory cartridges, decoded from the ROM dumps |
| **Jupiter-8** | Analog poly | 8 | **64** | All 64 factory patch names, full 32-control panel, two envelopes |
| **ARP Odyssey** | Duophonic | 2 | 44 | Complete 59-control front panel, all three filter revisions (4023/4035/4075), ADSR *and* AR envelopes, sample and hold |
| **Rhodes** | Physical model | 16 | 26 | Tine and tonebar as a coupled fork, inharmonic cantilever modes, nonlinear magnetic pickup |
| **Juno-60** | DCO poly | 6 | **56** | All 56 factory patches, read off Roland's patch charts; complete 25-control front panel measured against the hardware |
| **Little Phatty** | Mono Moog | 1 | **100** | Continuously morphing oscillators (triangle→saw→square→pulse, band-limited at every position in between), hard sync, 1/2/3/4-pole ladder, pre- and post-filter overload, the one-bus mod matrix with its spare destination, glide, and the three keyboard priorities |
| **Prophet-6** | Analog poly | 6 | **500** | All 500 factory programs, decoded from Sequential's own SysEx. Morphing oscillators with a triangle sub, resonant high-pass *and* SSM2040-lineage low-pass in series, poly mod (filter envelope and oscillator 2 into oscillator 1's frequency, shape and width and into both filters, at audio rate), per-oscillator slop, unison with chord memory, aftertouch, analog distortion |
| **TEO-5** | Analog poly | 5 | **256** | All 256 factory programs, decoded from Oberheim's own SysEx. The SEM state-variable filter, whose state control morphs continuously from low pass through notch to high pass with band pass on a switch; through-zero FM between the oscillators; three independently mixable waveshapes each, hard sync, a square sub and white or pink noise; two OB-8-curve DADSR envelopes; a global LFO and a per-voice one; a 16-slot modulation matrix of 20 sources against 65 destinations; twelve effect algorithms; unison with stored chord memory |

### Sampler

| Instrument | Type | Voices | Sounds | Description |
|-----------|------|--------|--------|-------------|
| **Sampler** | Sample playback | 64 | yours | 88 pads, one per piano key, eight sounds stacked on each with their own tune, trim, reverse and mute; per-pad trigger, poly, choke group, round robin, pitch, ADSR, level, pan, root and keytracking; a trim strip with a zero-crossing snap; **keys mode**, where stretches of keys become chromatic zones playing one sound from a root; and resampling — record any instrument in the box onto a pad, free or cut to whole bars |

64 is the number of voices allowed to sound at once, across the whole
instrument rather than per pad; the pool behind it holds 80, so a voice that
gets cut has somewhere to finish its fade. The per-pad limit is `poly`, which
runs 1–8 and counts *hits*.

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
| **jazz** | Live acoustic drums, modelled — a small bebop kit: sealed 18" kick with the two heads coupled through the shell, thin heads left to ring, a dark 20" ride, and brushes |
| **funk** | The same physics, a different drummer — 22" ported kick with a felt strip, a cranked snare over twenty tight strands, gel on the toms, and a ride with a ping |
| **studio** | Very dry and gated — a pillow in the kick that all but closes the two-head coupling, a muffling ring on the snare, taped toms, and a downward expander across every voice |

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

**Little Phatty** (100 patches, ours rather than Moog's — the Stage II's factory
*names* are printed in its manual but no published source gives the parameter
values behind them, so the bank is original and named in our own voice, weighted
towards bass the way the real one is):

- **Moog bass** — Taurus Deep, Sub Anchor, Round Bass, Bass Pillar, Thumb Bass, Wood Bass, Rubber Bass, Octave Bass, Muted Bass, Fifth Bass, Tri Sub, Pulse Bass, Sine Floor, Dub Weight, Slide Bass, Sequence Lo, Fat Unison, Bass Bloom
- **Overload** — Growl Bass, Tarmac, Coal Face, Snarl, Fuzz Anchor, Grit Stack, Overdriven, Bark Bass, Diesel, Torn Paper, Anvil Bass, Bad Weather
- **Sync** — Sync Lead, Sync Scream, Sync Sweep, Hard Reset, Sync Bell, Sync Buzz, Sync Whistle, Sync Stab
- **Lead** — Solo Saw, Reed Lead, Glass Lead, Whistle Top, Portamento, Brass Lead, Ribbon Lead, Flute Solo, Nasal Lead, Octave Lead, Vox Lead, Cut Lead
- **Wave morph** — Morph Drift, Half Saw, Between, Wave Wash, Slow Morph, Pulse Width, PWM Strings, Thin Ice, Wave Chase, Shape Shift, Morph Bass, Tri To Saw
- **Sample and hold, and effects** — Random Steps, Sample Hold, Computer, Alarm, Siren, Radio Chirp, Wind Tunnel, Static, Bleep Bloop, Sonar
- **Slope** — 2 Pole Bass, 1 Pole Pad, Open Ladder, Leaky, Bright 12dB, 6dB Lead, Half Ladder, Slope Swap
- **Pluck and percussion** — Zap Pluck, Clav Pluck, Wood Block, Kick Drum, Tom Hit, Blip, Marimba, Snap Bass, Dry Tick, Bell Pluck
- **Drone** — Self Osc, Slow Swell, Held Tone, Deep Drone, Air Pad, Ghost Pad, Filter Wash, Long Fifth, Choir Mono, Night Hum

**Rhodes** (26 patches, by instrument rather than by factory number — a Rhodes
has no patch memory):

- **Mark I** — MK1 Stage, MK1 Bright, MK1 Mellow, MK1 Bark, MK1 Ballad, MK1 Funk, MK1 Bass
- **Suitcase** — SC Classic, SC Tremolo, SC SlowTrem, SC Deep, SC Warm, SC 88
- **Mark II** — MK2 Stage, MK2 Tight, MK2 Dark, MK2 Suitcase
- **Dyno** — Dyno, Dyno Bell, Dyno Ballad, Dyno Bright
- **Character** — Bell Tine, Hard Bark, Soft Silk, Woody, Growl Bass

**Prophet-6** (all 500 factory programs, decoded from Sequential's own SysEx
release rather than recreated, in the instrument's five banks of one hundred):
`Brassed Off`, `Thick Low Brass`, `Jupiter Bass`, `Old School House Org`,
`TwinCity Kitty`, `Slow S&H Pad`, `It's a Prophet...6!`, `Oberheim`,
`JarreHead`, `Saw Sync Lead`, `Vox Felicities`, `War of the Worlds`,
`Wub Acid`, `Circus Triangles`, `Prophet Six String`, `Mass Effect`,
`T8 Piano` and the rest — including the forty `P5` programs, Sequential's own
ports of the original Prophet-5 factory bank. Pick a bank with `bank`, a
program with `program`.

**TEO-5** (all 256 factory programs, decoded from Oberheim's own SysEx release
rather than recreated, in the instrument's sixteen banks of sixteen):
`It's an Oberheim`, `Sync Growl`, `Weeping Wah`, `Bandpass Arp`,
`Bouncy Min9`, `Quintuple Mono`, `OB-X  S & H`, `OB-8 SoftSweep`,
`Ring Laboratory`, `Super Notch`, `Carillon`, `Phat Boi`, `2001 Choir`,
`Lofi Pad`, `Fields Pad` and the rest, each filed under the category the
instrument files it under — pad, lead, bass, poly, keys, string, pluck, bell,
arp, brass, voice, organ, perc, tuned perc, sfx. Pick a bank with `bank`, a
program with `program`.

---

## Features

**Audio Engine**
- Real-time audio via cpal (CoreAudio, WASAPI, ALSA)
- Lock-free audio thread — zero allocations, zero mutexes in the hot path
- Per-track instrument instances with independent processing
- Per-track and master VU metering via atomic shared state, on a dB scale
- Follows the output device: renders natively at whatever sample rate and block
  size the device is already set to, so nothing is resampled on the way out.
  `--sample-rate` and `--buffer-size` override it on request, and a device that
  refuses says so rather than letting the engine drift out of tune with the
  stream
- Gain-staged for chords, not single notes — every instrument is sized so a
  two-handed voicing at full velocity still has headroom
- Soft saturation on each instrument, transparent below its knee, replacing the
  hard clip that used to turn loud chords into a square wave
- Stereo-linked master limiter at -1 dBFS with a non-finite guard, so nothing
  above full scale and no NaN can ever reach the audio device

**Synthesizers**
- **Phosphor Synth**: the house synth, and the one instrument here that models nothing — the best ideas from three machines instead. Four oscillators mixed on a vector square, each an analog shape or one of 16 generated wavetables; a four-pole Moog-style ladder that self-oscillates and keeps its bass loss; a driven mixer ahead of it; two LFOs, two envelopes and a six-slot modulation matrix of eleven sources against ten destinations. Each oscillator can also be handed a **wave sequence** — a step list of waveform, length, crossfade, pitch and level that it walks on its own clock, the Wavestation's defining trick — so the timbre evolves rhythmically with no envelope doing it. Patches can be keymapped, so a single patch holds a whole drum kit built from the same oscillators and filter as the pads
- **DX7**: all 256 original factory voices, decoded from the ROM cartridge dumps and played on a 6-operator engine modelled on the YM21280/YM21290 chipset — all 32 algorithms decoded from the hardware table (including the multi-operator feedback loops in algorithms 4 and 6), log-domain envelopes with the hardware rate curve and its distinct attack shape, coarse/fine/detune frequency on the real parameter grid, keyboard level and rate scaling, global LFO with six waveforms and two-stage delay, and a per-voice pitch envelope
- **Jupiter-8**: the full front panel — dual VCOs with sync and exponential cross-modulation, switchable 12/24 dB IR3109 filter with resonance to self-oscillation, non-resonant HPF, two independent ADSR envelopes, LFO with four waveforms and a two-stage delay, portamento, and 4 voice modes (Solo/Unison/Poly1/Poly2). Envelope times follow Roland's published 1 ms-10 s specification; filter corners, LFO taper and keyboard follow are measured rather than approximated
- **ARP Odyssey**: the full front panel — two VCOs with coarse and fine tuning over the panel's 20 Hz-2 kHz range, hard sync, per-oscillator pulse width and PWM, two frequency-mod inputs each, a keyboard switch that drops VCO-1 into the LFO range, the sample-and-hold mixer with its own sources, clock and lag, an XOR ring modulator sharing a fader with white or pink noise, all three filter revisions (12 dB 4023 SVF / 24 dB 4035 ladder / 24 dB 4075 Norton) on one 16 Hz-16 kHz sweep and each resonating to self-oscillation, a non-resonant HPF, three filter modulation slots, VCA gain and drive, and both envelope generators — the ADSR and the AR — with their own sliders and their own LFO-repeat gating. Envelope times follow ARP's published 5 ms-10 s specification and the pitch pads are mapped to pitch bend and the modulation wheel
- **Juno-60**: the full front panel — LFO rate/delay, DCO with PWM depth and a 3-position PWM mode (LFO/MANUAL/ENV), saw/pulse/sub/noise and a 16'/8'/4' range switch, 4-position HPF, IR3109-style 24 dB/oct resonant VCF with env polarity, LFO and keyboard follow, ENV/GATE VCA, shared ADSR, and BBD stereo chorus (I / II / I+II). Envelope taper, LFO rate taper, filter corner frequencies and chorus rates are calibrated against measurements of the hardware rather than approximated. All 56 factory patches are the instrument's own, transcribed from Roland's published patch charts
- **Rhodes**: a physical model rather than a sample set, because velocity on a Rhodes changes the *spectrum* and a layered sample set is three photographs of that. A hammer strikes a tine — a steel cantilever, so its overtones are inharmonic, at 6.27, 17.5, 34.4 and 56.8 times the fundamental — and those overtones die far faster than the fundamental does, which is why the attack is a bell and the sustain is almost a pure tone. The tine is paired with a tonebar in an asymmetric tuning fork, so the sustain undulates as energy crosses between them. Sustain per register comes from measured Q values on a 1974 Mark I, interpolated rather than fitted: E flat 2 at 3.88 s, E flat 3 at 1.50, E flat 4 at 1.56, E flat 5 at 1.11, E flat 6 at 0.45 — not monotonic, and left that way. The bark is the pickup: the coil senses the flux gradient where the tine happens to be, and that gradient is an odd function of the tine's offset from the pickup axis, so **voicing** — moving the tine's rest position, the adjustment a technician actually makes — takes the fundamental and every odd partial away and leaves the second partial dominant, exactly as the literature describes. Struck harder the tine swings further into that nonlinearity, so velocity changes the timbre through the pickup rather than through a brightness knob. Felt dampers on release, none above the sixth octave as on the real action, sustain pedal, and the Suitcase's stereo tremolo — which is a pan between two amp channels, not an amplitude modulation
- **Prophet-6**: six voices of the whole front panel, and **all 500 factory programs**, decoded from `P6_Programs_v1.01.syx` — Sequential's own SysEx release of July 2015 — and shipped as 63 KB of the instrument's own bytes with a documented byte map, the way the DX7's cartridges are. Neither manual publishes an offset table and no open-source Prophet-6 editor exists, so the map was established against the bank itself; three of its modulation-destination bits are corrections to the obvious reading, and the argument for each is at `raw_offset`. Per voice: two oscillators that morph continuously triangle→sawtooth→pulse with the pulse width symmetric about the square, a triangle sub-oscillator an octave under oscillator 1, white noise, hard sync (oscillator 1 is the slave), oscillator 2's low-frequency and keyboard switches, and an independent slow random walk per oscillator per voice for slop. Two resonant filters in series — a 2-pole high-pass and a 4-pole low-pass in the **SSM2040 lineage of the Rev 1 and Rev 2 Prophet-5, not a Moog ladder**: its resonance stage is compensated, so where the Little Phatty's ladder loses 15.5 dB of bass at full resonance this one gains 6.5, a 22 dB gap that is measured against the ladder in the rack rather than asserted. One filter envelope with an independent bipolar amount and velocity switch at each filter, an amplifier envelope, and an LFO whose five shapes carry the manual's own polarity (triangle and random bipolar, sawtooth and square positive only) and reach audio rate. **Poly mod** is the section this instrument exists for: the filter envelope and oscillator 2 as bipolar sources into oscillator 1's frequency, waveshape and pulse width and into both filter cutoffs, all at the sample rate, so oscillator 2 into the low-pass really is audio-rate filter modulation. Unison stacks one to six voices with chord memory; the six key-assign modes; aftertouch to six destinations; analog stereo distortion with rails rather than a makeup gain; and Effect A and Effect B in series carrying the OS 1.0 effect lists, of which the two delays and the chorus render and the phasers and reverbs are stored, selectable and passed through until the effects milestone connects them

- **Little Phatty**: the Stage II's whole front panel, plus the eight per-preset parameters Moog put in its Advanced Preset menus. The headline is the **wave control**: each oscillator morphs continuously from triangle through sawtooth through square to a skinny pulse, and the positions between the four labelled shapes are real waveforms rather than crossfades of two others — the oscillator is one trapezoid whose rise, top, fall and bottom move with the knob, band-limited by polyBLAMP at all four corners, and WAVE is a modulation destination because it is voltage-controlled on the hardware. Hard sync with a sub-sample-accurate reset; a transistor ladder whose slope switches between 6, 12, 18 and 24 dB/octave by tapping the ladder rather than shortening it, so a two-pole Phatty still resonates as players describe; pre- and post-filter asymmetric overload with the documented +6 dB at full; two ADSRs with the three gate modes (legato on, legato off, envelope reset); a pitch wheel whose two directions are ranged independently; the one-bus modulation matrix with its six sources, four destinations and the secondary destination the menu adds; constant-rate glide measured against the manual's own five-seconds-across-the-keyboard figure; low, high and last-note keyboard priority; and velocity on the filter and nowhere else, which is most of why an LP feels the way it does. Every range is the manual's — 20 Hz to 16 kHz cutoff (audibly darker than a vintage Moog's, as Sound On Sound notes), 1 ms to 10 s envelopes, 0.2 Hz to 500 Hz LFO, ±7 semitones on oscillator 2
- **Drum Rack**: 18 kits — circuit-accurate 808/909/707/606/727/CR-78, companded-PCM LinnDrum and DMX, the analog SDS-V, the creative 777, the warm tape-saturated tsty series, and three **live acoustic kits** that are physics rather than voicing. An acoustic kick is two membranes coupled through the air inside the shell, and that coupling — not a filter — is what puts two low modes a sixth apart where a drum machine has one; the front head's muffling is a knob that moves the interval between them. The snare's strands are a bouncing-contact model, so they choke on a hard backbeat and ring on after the drum instead of being a noise burst under an envelope. Cymbals are banks of forty complex resonators with frequency gating, so hitting one harder brings in modes that were not there at all, and with a modal cascade that carries energy from the low modes up into the high ones — bow, bell and edge are one plate struck in three places, and a hi-hat is two plates that clamp

**Sampler**
- 88 pads, eight sounds on each, loaded from WAV or recorded off any
  instrument in the box; `K` turns the same bed into chromatic zones, so one
  sample can play a whole keyboard — see
  [its section below](#sampler--a-sound-on-every-key)

**Fingers — the practice room**
- A generative jazz-piano technique trainer over your own instruments — see
  [its section below](#fingers--the-practice-room)

**Session Management**
- Save/load projects as `.phos` files (human-readable JSON)
- `Ctrl+S` quick save, `Space+S` save under a name, `Space+O` open — the open
  prompt is a **file picker**: a list of the projects folder, walked with
  `j`/`k` and `Enter`, so nothing has to be typed from memory
- Saves all tracks, instruments, synth parameters, clips, MIDI notes, transport settings
- A kit, a patch or a cartridge is stored by **which one it is**, not by where its
  knob sat: a knob position only names a patch while the bank is the size it was
  when the session was written, and reopening on a different instrument is the
  kind of wrong that looks perfectly reasonable
- Atomic writes prevent file corruption
- Default save directory: `sessions/` when you are running from a checkout,
  otherwise `<app dir>/sessions/`. A name saves into it and the picker opens on
  it — the save and the list are the same folder by construction, so what you
  saved is in what you are shown. See [Where files live](#where-files-live)

**User Presets**
- `Space+W` opens a preset browser for the selected instrument
- Every instrument has its own bank, the drum rack included — the whole parameter
  block, including the factory patch it was dialled in from
- One human-readable file per instrument (`<app dir>/presets/<instrument>.json`),
  atomic writes, so a DX7 preset can never be offered to a Juno
- Presets sit beside the factory tables rather than extending them, so adding one
  cannot move a patch index stored in a saved session
- A preset saved against a different panel — wrong instrument, wrong number of
  controls, or an older layout — is refused rather than loaded into the wrong holes
- A kit, a patch or a cartridge is stored by **which one it is**, the same as a
  session stores it, so a preset saved on the 909 opens on the 909 after the rack
  has grown a kit rather than on whatever now sits at that fraction of the knob
- 128 presets per instrument, 32 characters per name

**Undo/Redo**
- `u` undoes the last action, `Ctrl+R` redoes
- Works for everything that changes the session: notes and highlights, yanks
  and pastes, clips and whole tracks, knobs on any panel, the effect chain,
  recorded takes, loop-section cuts and stamps, sequencer patterns, and the
  sampler — a sound removed from a pad comes back with its audio, and a zone
  comes back with the keys it covered
- A sweep of one knob, or a whole run of trim nudges, folds into a single step
- Full track restoration on undo (instruments, params, clips, audio routing)
- 100-action undo stack

**Themes**
- 9 built-in color themes (see [Themes](#themes))
- `Space+V` cycles themes instantly
- Theme choice persists across sessions (`<app dir>/config.json`)

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
- Workspace of eight crates — seven libraries and the `phosphor-studio` binary — with a clean dependency graph
- Modular file structure — app, UI, and state split into focused sub-modules
- Shared domain models via atomics (no locks between threads)
- Command channel pattern for UI-to-audio communication
- Plugin trait for instruments and effects — same interface for built-in and third-party
- 2,047+ tests covering DSP, MIDI, engine, mixer, navigation, and persistence, run on Linux, macOS and Windows in CI

---

## Sampler — a sound on every key

An instrument like the others, except the sound is yours. **`Space+A`**,
choose **Sampler**, and the track opens on its **`[pads]`** tab with a
keyboard drawn across it: eighty-eight pads, one per piano key, each holding
up to eight sampled sounds stacked on top of one another, each with its own trigger,
polyphony, choke group, pitch, envelope, level, pan and root — in
milliseconds and semitones, not in percentages of a knob.

Sounds arrive three ways. You pick a WAV out of a list and it lands on the
pad — or type its path, if it lives somewhere no list would show. Or you
point the pad at one of the synths in this box — the Rhodes, the DX7, any of
them — play it, and keep what you played as audio; you can then trim it by
eye and by ear, and what plays is a region of the file rather than a new copy
of it. Or you keep the same performance as a **phrase** — the notes rather
than the audio, replayed live through one instrument, for a pad that costs a
few hundred events instead of a few megabytes.

The same eighty-eight keys read two ways, and `K` swaps between them: a **pad
map**, where every key is its own sound with its own settings, and **keys
mode**, where a stretch of keys is one zone playing one sound transposed from
a root. Drum kit or piano, on the same bed.

### Getting started

1. Press `Space+A`, `j`/`k` to **Sampler**, `Enter`. The track arrives
   selected, the pad map open, your controller routed to it.
2. Play a key. The caret `▼` on the keyboard band moves to that pad — on an
   88-key controller that is the fastest pad selector there is. Without one,
   `h`/`l` walk the bed a key at a time and `H`/`L` an octave. The caret
   starts at C3, under your hand.
3. Press `a`. A list of `<app dir>/samples/` opens: `j`/`k` to a sound and
   `Enter` puts it on the pad. Type letters to narrow the list, `h` walks up
   a folder and `Enter` on a folder walks into it. For a file that lives
   somewhere else entirely, `/` swaps the list for a typed path — where a
   bare name like `kick` still looks in `<app dir>/samples/` and tries `.wav`
   for you.
4. Play that key again. The pad sounds. Press `a` again to stack a second
   sound on the same pad, up to eight; they play together.
5. `j`/`k` picks a control on the panel, `Enter` holds it, `h`/`l` turns it,
   `H`/`L` strides, `Esc` lets go. Holding is what tells the two jobs of
   `h`/`l` apart: free they walk the keyboard, held they turn the control and
   nothing else sees the key.
6. `t` opens the trim strip over the sound under the cursor. `i` records the
   pad off another instrument. `K` turns the bed into zones.

### The bed, the list and the panel

The keyboard band across the top **is** the kit. A key with sounds on it
lights in the track's colour and carries the number of them on its face; a
key whose file has gone missing is red; the caret marks the pad you are
editing. On a narrow terminal the band scrolls to keep the caret in sight,
and below twelve rows it goes entirely — the panel is what your keys are
typing into, and a keyboard you can see while the control you are turning is
off-screen is the wrong half to keep.

Under it, two columns: every filled pad as a row — its key, what is on it,
how many, how it triggers — and the panel for the pad under the caret, with
the layer list beneath it. The head of the list says how much audio the kit
is holding (`12.3 MB held`), counting a buffer shared by several pads once —
a zone across the whole bed costs one sample, not eighty-eight. The mode tag
in the bottom bar reads `-- PADS --`,
`-- HOLD --` while a control is held, `-- TRIM --` in the strip,
`-- SOURCE --` and a blinking `-- TAKE --` while you are recording.

### Keys mode — zones across the keyboard — `K`

`K` switches the bed between **pads**, where every key is its own sound, and
**keys**, where a stretch of them is a **zone**: one sound, transposed from a
root, the loop brace's shape laid along the keyboard instead of along the
bar. The tag reads `-- KEYS --`. It is a capital on purpose — lowercase `k`
walks the control cursor, and a kit that changed shape under a key you press
all day would change it by accident.

Nothing is destroyed either way. The pad map is still underneath while the
zones play, the zones are still there when you press `K` again, and a session
stores both.

**A sampled piano across the whole keyboard, from a fresh app:**

1. `Space+A`, `j`/`k` to **Sampler**, `Enter`.
2. Press `K`. The bar reads `-- KEYS --` and the panel says there is no zone
   on this key yet, naming the three keys that make one.
3. Press `w`. A zone covers the whole bed. (`o` covers just the octave the
   caret is standing in.) It takes the sound from the pad under the caret if
   that pad has one; on a fresh track it has none, and the panel says
   `a loads a sound into it`.
4. Press `a`, type the path, `Enter`. Because a zone always tracks the
   keyboard, a file name ending in a note teaches the zone its root as the
   sound arrives — `Piano_C3.wav`, `kick_A#1.wav`, `Strings Eb2.wav` — and
   the flash names the root it learned. A name that is not a note changes
   nothing: `07_Kick`, `TR808` and `C3loop` all leave the root alone, because
   a wrong root retunes every key in the zone.
5. Play. Every key sounds that sample, transposed by its distance from the
   root.
6. If the root is wrong, or the file name taught nothing, press `R` and play
   the key the sound was actually recorded at. The bar blinks `-- ROOT? --`
   while it waits; the key you play becomes the root and does not move the
   caret. `Esc` disarms and changes nothing.

**Splitting the keyboard into ranges.** Walk the caret to the key where the
next sound should start and press `s`. The zone under the caret splits there:
the left half keeps its sound and the root it was tuned to, the right half
starts as the same audio — the same buffers, not a copy — rooted at its own
first key, ready to be retuned with `R` or reloaded with `a`. Split again for
as many ranges as you want. `s` on a zone's own first key is refused in
words, because there is nothing to its left.

**Resizing a zone — the span brace.** `span` is the first control on the
panel, and making or splitting a zone leaves the cursor on it. `Enter` holds it;
then `h`/`l` move the **low** edge and `H`/`L` the **high** one. That is the
loop brace's grammar, so `H`/`L` are the other end of the zone rather than a
bigger stride. An edge stops where its neighbour begins and never pushes it,
and stops at the ends of the bed — when it stops, the bar says which. The
caret rides the edge it is pushing, so the panel never loses the zone you are
moving. A whole run of presses is one press of `u`. `Esc` lets go. Inside a
zone, `w` and `o` throw that same brace across the whole bed or the current
octave rather than laying a second zone over it.

**What the screen adds.** A rule under the keyboard draws each zone as a
brace — `├──┼───┤` — with the tick where its root is, and the root key itself
wears `◆` on the band instead of a layer count. The zone under the caret is
amber; the others are the track's colour. The column that lists filled pads
in pads mode becomes the **zone list**: span, sound, how many layers, root,
and how many keys.

**Everything else is the same keys pointed at the zone instead of the pad.**
`j`/`k` walks its controls, `[`/`]` picks a sound inside it, `a` stacks
another, `t` trims, `n` normalizes, `i` records into it, `m` and `d` mute and
remove a layer. Only two controls differ: `span` is added at the top, and
`keytrk` is gone — a zone always tracks the keyboard, so the switch would be
a control with nothing on the other side of it.

**`D` takes the zone under the caret off the bed**, after a y/n. Capital,
because lowercase `d` still removes one sound from the zone. `u` brings the
whole zone back with its audio.

**Zones may overlap.** Where they do, the leftmost zone covering a key owns
that key's settings — trigger, poly, envelope, level — because a key cannot
have two envelopes, and the zones over it lend only their layers. A lent
layer is retuned by the distance between the two roots, so it still sounds
the pitch you pressed rather than the one the owning zone's root would have
transposed it to. Eight layers is still the ceiling on any one key; past it
the rest are turned away, and the edit that caused it says how many. An edge
that already overlaps its neighbour can move the way that mends the overlap,
never the way that deepens it.

`w`, `o` and `s` pressed in pads mode do not quietly do nothing — they say
`K puts the bed into zones`, which is the key you wanted.

### What a pad does

`j`/`k` walks these, pad controls first and then the selected layer's. In
keys mode the same list belongs to the zone, with `span` at the top of it and
`keytrk` absent.

| Control | What it does |
|---------|--------------|
| `span` | *(keys mode only)* the zone's two edges, low and high. `Enter` holds it and `h`/`l` / `H`/`L` move them |
| `trig` | **one-shot** plays the trimmed region to its end and ignores the key coming up; **gate** sounds while the key is held and runs the release when you let go |
| `poly` | 1–8 simultaneous hits on this pad. Past the limit the oldest is cut with a fade, never a truncation |
| `choke` | mute group, `off` or 1–8. A hit silences every sounding voice on *other* pads in the same group — the closed hat stopping the open one |
| `cycle` | round robin. Off, every sound on the pad that answers the hit plays and they stack. On, one of them answers and the next hit takes the next — eight snares on one key stop sounding like a machine gun |
| `pitch` / `fine` | ±48 semitones, ±50 cents |
| `attack` `decay` `sustain` `release` | up to 10 s a stage, sustain as a percentage. Release is floored at 5 ms by the engine, because a zero release is a click |
| `level` | the pad's gain, from silence through −40 dB up to +12 dB |
| `pan` | twenty detents from the centre to either end |
| `root` | the key that plays the sound untransposed. Defaults to the pad's own key |
| `keytrk` | *(pads mode only)* on, the pad transposes the sound by its own distance from `root` — set `root` to the note the sample actually is and the pad plays it in tune on whatever key it sits on; off, the sound plays untransposed |

A fresh pad is one-shot, poly 1, no choke, no cycle, no pitch offset, attack
0, decay 400 ms, sustain 100%, release 60 ms, unity, centred, rooted on its
own key with keytracking off.

### The sounds on a pad

The layer controls — `level`, `pan`, `tune` (±48 st), `fine` (±50 ct), `rev`,
`mute` — are the last six on the panel, and they appear only once the pad has
a sound: a gain knob for a sound that is not there is a control that answers
keys and changes nothing.

`[` and `]` pick which sound they address; `1`–`8` jumps straight to one.
Moving the layer cursor **auditions** that sound on its own, outside the pad's
poly and its choke — a stack of eight is eight names in a list until you can
hear which is which. A muted layer stays silent, and the audition stops the
moment you leave the pads tab.

`m` mutes the sound without taking it off the pad. `d` removes it and asks
first. `u` brings it back with its audio, `Ctrl+R` takes it off again.

Turn `cycle` on and the stack becomes a rotation instead: successive hits hand
out the sounds one at a time, so eight takes of the same snare are eight
different snares rather than one thick one. A muted sound and a sound outside
the velocity window lose their turn rather than spending it on silence, and
where the rotation has got to is the pad's own memory — stopping the transport
does not put it back to the first sound, and neither does editing the pad.
Phrases are never rotated: every one on the pad fires either way.

Sampler tracks are ordinary tracks in every other respect: draw notes in the
piano roll, record from your controller, put effects in front of them. A note
on the track fires the pad for its key.

### The trim strip — `t`

`t` on a sound draws the whole file as a waveform, the region that plays lit
and the cut ends dark, with `[` and `]` on a ruler beneath it sitting over the
two edges. The header names the layer, the nudge unit, the snap and where both
markers are.

- `h`/`l` move the **start**, `H`/`L` move the **end** — the loop brace's
  grammar, so `H`/`L` are the other end rather than a stride.
- `j`/`k` walk how far one press moves: **bar · beat · 1/16 · 10 ms · 1 ms ·
  1 sample** (`j` deeper, `k` wider; it opens on 10 ms). The musical units
  come from the transport's tempo measured against the layer's *own* sample
  rate, so a bar of a 48 kHz loop is a bar wherever it is played.
- Every start nudge plays the region from its new start. A marker is a
  position in a waveform nobody can hear by looking at it.
- `z` toggles the zero-crossing snap, on by default: an edge lands on the
  nearest sign change within 5 ms, and keeps the exact frame you asked for
  when there is none.
- `r` plays the region backwards. The region, not the file — a trim found
  forwards still means the same audio flipped.
- `t` loops the region while you work on it.
- `Esc` goes back to the pad map and stops the sound.

Trim is never a rewrite: the buffer keeps every sample and the markers say
what plays. The edges cannot cross and stop a millisecond apart, which the
bar says in words. A whole nudge run — however long you hold the key — is one
press of `u`. A sound whose file has gone missing is refused rather than
opened onto an empty pane.

### Phrases — the performance instead of the audio

A **phrase** is a take that was never rendered. The notes are kept exactly as
you played them and replayed, live, through one instrument. Four bars cost a
few hundred events instead of a few megabytes, and the sound comes from a
synth rather than a buffer — so it is still the synth, with its filter still
open, and not a photograph of one.

In source mode (see below), `p` swaps what `r` will land. The banner reads
`take: audio` or `take: phrase` the whole time, because it is the difference
between a pad that costs a buffer and one that costs an instrument, and
finding out afterwards is finding out too late. The pad remembers which,
along with the instrument it was recorded from, so coming back to it a week
later opens the way you left it.

`r`, play, `r` again: the notes land on the pad as a **`phr` row** in the
sound list, named `phrase 1`, `phrase 2`, in one undo step. Four phrases fit
on a pad; the fifth is refused before you play, in words, the same as a full
layer bed. The track's chord and arp devices are baked in exactly as they are
for audio — a phrase recorded over an arpeggiator holds the arpeggio — and a
one-finger phrase teaches the pad its root the way a take does.

**One child instrument per sampler.** Not one per pad: eighty-eight pads with
an instrument each is eighty-eight voice pools, and this one is rendered once
per block however many phrases are running. It is what makes the feature
affordable, and it is the thing to know about it. Landing a phrase points the
child at the instrument that phrase was played on, with the panel you played
it with. Record the next phrase from something else and the child is
*replaced* — every phrase on the kit now plays through the new one — and the
flash says `child is now Rhodes · every phrase plays through it`, because a
silent change to how a whole kit sounds is the kind of thing you discover a
week later.

A phrase's own controls are three, and the three are all there are:

| Control | What it does |
|---------|--------------|
| `vel` | 0–400%, what every recorded velocity is multiplied by. A percentage and not decibels, because it scales what is *played* rather than how loud the result is — there is one shared render for every phrase on the kit, so there is nowhere per-phrase to put a fader |
| `mute` | out of the pad's sound, still in the list |
| `keytrk` | *(pads mode only)* the notes shift by the played key's distance from the pad's `root`. A zone's phrases always transpose, so the switch is not offered there |

The pad's `level`, `pan` and envelope do **not** reach a phrase, and neither
do a layer's `pan`, `tune` or `rev`. They are not greyed out; they are not
there, the same way an empty pad has no layer knobs. What does reach a phrase
is everything about *when* it plays: `trig`, `poly`, `choke`, the transport
stopping and the panic key all treat it exactly as they treat a sample, and
every note a phrase put down is handed back the moment it is cut.

`phr` rows sit after the wav and rec rows in the same list, so `[`/`]`,
`1`–`8`, `m` and `d` are the keys they already were. A row reads its name,
its length in seconds, its `vel` percentage and `phr`; `d` asks before it
removes one, and `u` brings it back with its notes.

Two things a phrase does not do. Moving the cursor onto a `phr` row does not
**audition** it — there is nothing to sound on its own — so it says to press
the pad's key instead of going quietly silent, which would read as an
audition that had broken. And `t` on one says a phrase is notes rather than
opening a waveform of nothing.

**Tempo is baked.** Event offsets are frames, decided when you played, the
same as an audio take. A phrase does not follow a tempo change — it is a
recording, and the sibling it has to sound like is the recording on the pad
beside it. It does follow a change of *device*: the rate it was captured at
is stored with it, so a phrase played on a 48 kHz interface plays at the
speed you played it on a 44.1 kHz one, exactly as a sample does.

Phrases go into the session inline, notes and all, with the child instrument
and its panel. Nothing is written beside the file: a phrase has no audio, so
it has no sidecar.

### Recording the machine into itself — `i`

`i` on a pad asks which instrument to record it from — everything in the
house except the step sequencer and the sampler itself. `Enter` and the track
slips into **source mode**: the sampler steps out of the slot and that
instrument plays in its place, through the track's own MIDI effects, inserts,
fader and sends. What you hear is what will be recorded, because it *is* the
track. The pad cursor freezes while the mode is on — your keys are a
performance now — and the banner names the pad waiting underneath.

`r` arms. Play. `r` again ends the take and lands it on the pad as a new
sound, selected, named `take 1`, `take 2`, in one undo step, with its length
and its peak in the flash. `p` swaps what `r` will land — audio, or the
performance itself as a phrase (see above); the banner reads which the whole
time, and the pad remembers it.

**Stopped, the take is free.** Time zero is your first note-on, it ends when
you disarm, and the instrument's tail is rendered out to silence — two
seconds at most. The result is auto-trimmed: the start backed 8 ms off the
first sample above −48 dB so nothing clips a transient, the tail cut 20 ms
past the last sample above −60 dB. It stops itself at 60 seconds and says so.

**Rolling, the take is bars.** The window opens at the next bar line and
closes at the end of the bar you disarm in, so the file is a whole musical
length. Nothing past the loop point is kept and nothing is trimmed — the seam
is the point, and a take that loops has to be cut exactly where the next pass
begins. It stops itself at 64 bars.

- Stopping the transport ends a running take. So does `Esc`, which ends it
  before it leaves the mode — a performance is too expensive to throw away on
  a key that means "back". The second `Esc` is the one that leaves, and
  leaving puts the sampler back with the whole kit replayed onto it.
- A performance on **one** key teaches the pad its root. Keytracking is left
  alone: a root is a fact, keytracking is a decision. A chord leaves the root
  where it was.
- The pad remembers what it was recorded from *and* the panel you recorded it
  with, so `i` again reopens the picker standing on that instrument and the
  next take of the same sound costs one `Enter`.
- A full pad refuses the arm before you play, not after. Finding out that a
  pad was full once the playing is over is losing the take.
- Nothing is recorded from the audio thread. What is captured is MIDI with
  its arrival stamps; the audio is rendered afterwards, offline, through a
  fresh copy of the instrument and the track's own chord and arp devices.
  That is why a take can never glitch a performance, why it is bar-exact
  rather than however long the buffer happened to be, and why the same
  performance renders the same file every time.
- Source mode takes three keys on the pad map — `r`, `i`, `Esc` — and answers
  the rest in words. While it is on, every key that edits a pad would be
  editing something you cannot hear.

**Dialling the sound — `Tab`.** The instrument in the slot is a real
instrument with a real panel, and `Tab` is the road to it: from the pad map,
one `Tab` puts you on `[inst]`, and while the mode is on that panel is the
*source* instrument's rather than the sampler's two globals. It is headed
`Phosphor Synth · source for pad C3` so you can see whose panel you are
standing in. `j`/`k` pick a control and `h`/`l` turn it — the same keys as
any other track's panel, because it is the same panel — and you hear every
turn as you make it, since that instrument *is* the track's plugin slot. The
patch selector loads a whole patch here exactly as it does anywhere else.

What you dial is what `r` renders the take through, and the pad keeps it:
landing a take, leaving the mode, or pressing `i` again writes the panel onto
the pad as one undo step — and only if you actually moved something, so a
trip through the mode that changed nothing changes nothing. Come back with
`i` a week later and the sound is the one you recorded with.

Two things worth knowing. `Esc` on that panel means what it means on every
other panel in the box — back out to the track list — and leaves the mode
standing; the mode's own `Esc` is on the pads tab, where the banner offering
it is. And the sampler's own `level` and `vel` are simply not on the panel
while the mode is on, which is the point: they used to be, over a slot
holding a synth, so turning `level` sent an unlabelled patch change to an
instrument nobody was looking at.

One gap, honestly: `Space+W`'s preset browser cannot be opened while the mode
is on, because `Space` is refused from every tab while the slot is on loan.
The factory patch selector at the top of the panel is how a sound is chosen
here; a named user preset has to wait for the browser to grow a door that
does not go through the space menu.

Recording into a **zone** works the same way, and it is how a multisampled
instrument gets built: split the bed with `s`, stand in each range and record
it from the same synth. The one-key performance that teaches a pad its root
teaches a zone its root too, which is exactly the number that range needs.

### Normalize — `n`

`n` on a sound sets its level so the **trimmed** region peaks at −0.5 dB.
That is a gain value, never a rewrite of the audio: the buffer is shared with
the engine and with undo history, and rewriting it would change all of them
destructively for a decision you may want back. `n` again puts the level back
to unity, and each throw is its own undo step.

The level control's ceiling is the ceiling here, because this *is* that
control. Our instruments render with a lot of headroom, so a single quiet
note can want more than the twelve decibels there are — then it goes as far
as it goes and the flash says how far short that left it.

### Files, and what a session keeps

A session stores each sound's **path** — never the audio. A path chosen from
the picker is stored whole, so it keeps pointing at that file wherever the
session is opened; a bare name typed into the `/` prompt stays a bare name
and keeps resolving against `samples/` on any machine. Both beds are stored:
the pad map, the zones, and which of the two the track was in when you saved.

A recorded take has no file until you save. Saving writes it as a 32-bit
float WAV into **`<session>.samples/`**, beside the session file, and stores
the path relative to it — so a project folder can be moved, copied or handed
to somebody else whole. Takes already on disk are not rewritten, so a session
with forty recordings in it does not rewrite forty WAVs every save, and
*Save As* gives the new project its own copies rather than a reference into
the old one's folder. The audio is written before the JSON: the worst crash
leaves an orphan WAV nobody notices, rather than a session naming audio that
is not there.

The same save sweeps out the takes the kit no longer names, so a folder does
not fill up with recordings nothing points at. Only inside that folder, and
only files it wrote itself: a WAV you put in there by hand stays where you
put it, and so does anything outside it.

`Space+W` saves and loads a sampler preset, which is the two globals on the
flat panel — `level` and `vel` — and nothing else. The pads, the sounds on
them, the zones and the takes belong to the session; a preset leaves them
exactly where they were.

A file that has moved is not dropped. The pad keeps it with all its settings,
the key goes red on the bed, the layer list says `missing`, and the status bar
says so on open — the paths are in the debug log. Fix the path or drop the
file into `samples/` and reopen.

WAV in, 16-, 24- or 32-bit integer or float, mono or stereo (anything past two
channels is dropped), up to ten minutes a file.

### Keys on the pads

| Key | Action |
|-----|--------|
| `Space+A` → *Sampler* | Make a sampler track; it opens on `[pads]` |
| `h` / `l` | Walk the bed one key |
| `H` / `L` | Walk the bed an octave |
| *play a key* | Jump the caret to that pad |
| `j` / `k` | Pick a control |
| `Enter` | Hold the control — now `h`/`l` turn it, `H`/`L` stride |
| `Esc` (held) | Let go of the control — `Enter` releases it too |
| `[` / `]` | Pick which sound on the pad the row controls address — layers first, phrases after |
| `1`–`8` | Jump to that sound, and play it |
| `a` | Load a WAV onto this pad — a list of `samples/`; `/` types a path instead |
| `t` | Trim the sound under the cursor |
| `i` | Record this pad from an instrument |
| `n` | Normalize the sound, or put it back to unity |
| `m` | Mute the sound, keeping its seat on the pad |
| `d` | Remove the sound (y/n) |
| `K` | Switch the bed between pads and keys |
| `u` / `Ctrl+R` | Undo / redo — a removed sound comes back with its audio |
| `Esc` | Back to the track list |

### Keys in keys mode

Everything above still means what it means; these are the four keys a pad map
has no word for, plus the brace.

| Key | Action |
|-----|--------|
| `K` | Back to pads — nothing is lost either way |
| `w` | A zone over the whole bed; inside one, throw its brace that wide |
| `o` | The same for the octave the caret is standing in |
| `s` | Split the zone under the caret at the caret key |
| `D` | Take the zone off the bed (y/n) — `u` brings it back with its audio |
| `R` | Learn the root from the next key you play; `Esc` disarms |
| `Enter` on `span` | Hold the brace — then `h`/`l` move the low edge, `H`/`L` the high one |

### Keys in the trim strip

| Key | Action |
|-----|--------|
| `h` / `l` | Move the start |
| `H` / `L` | Move the end |
| `j` / `k` | Nudge unit: bar · beat · 1/16 · 10 ms · 1 ms · 1 sample |
| `z` | Zero-crossing snap on/off |
| `r` | Play the region backwards |
| `t` | Loop the region while you work |
| `Esc` | Back to the pad map, quiet |

The strip owns every key while it is open, including `Tab` — the same bargain
a held knob strikes, and for the same reason. `Esc` is the way out.

### Keys in source mode

| Key | Action |
|-----|--------|
| `r` | Arm the take; again to end it and land it |
| `p` | Swap what `r` lands: `take: audio` or `take: phrase` |
| `i` | Change the instrument |
| `Tab` | Off the map to `[inst]` — the source instrument's own panel |
| `Esc` | End a running take; again to put the sampler back |

While the mode is on, the sampler is out of the track's plugin slot. Every
other key on the map — `Space` included, from any tab — says so rather than
acting, and the sentence it says names `Tab` as the way to the panel.

---

## Fingers — the practice room

A jazz-piano technique trainer built into the DAW. Press **`Space+F`** on any
instrument track and the practice room opens over it: your MIDI controller
keeps sounding that track's synth — the Rhodes, the DX7, whatever you chose —
while the room listens to what you play and judges it, from the same
microsecond arrival timestamps the recorder uses. Close it and you're back in
the session; nothing you had going is touched.

Everything in the room is **generated, in any key** — no canned lesson
content. The fingerings are the published standards (verified against
conservatory charts), and they're drawn **on the keys**: every target key
lights up with the finger number that belongs on it, 1 = thumb through
5 = pinky, right hand amber, left hand blue.

### Getting started

1. Add or select an instrument track (`Space+A` if you don't have one).
2. Press `Space+F`. The drill list opens.
3. Pick **major scale · C** (the top row), leave it on **RH** and **wait**
   mode, and press `Enter`.
4. Play the scale on your controller. Time stops until you play the right
   note — wrong notes flash, right notes advance. The on-screen keyboard
   shows the next key and the finger to use.
5. When a rep is clean, it rolls straight into the next. Three clean reps in
   a row and the tempo climbs 5 BPM on its own. That's the whole game.
6. When wait mode feels easy, press `w` for **flow**: a metronome rolls and
   every note is judged against the beat — early, late, or on it.

### The drills

| Drill | What it teaches | Level |
|-------|-----------------|-------|
| Major scales (12 keys) | The standard fingerings; thumb-under technique | 1 |
| Harmonic minor scales (12 keys) | Same, with the raised-7th colour | 2 |
| Chromatic scale | The French fingering: 3 on every black key | 2 |
| Hanon no. 1 | Finger independence; the weak 4-5 pair | 2 |
| Major arpeggios | Arm-led hand shifts across octaves | 3 |
| Shell 2-5-1 (cycle of fourths) | Root-3-7 voicings; one voice moves per change | 3 |
| Charleston comp | The first jazz comping rhythm: beat 1 + and-of-2 | 4 |
| Bebop dominant scale | The 8-note scale; chord tones land on downbeats | 4 |
| Rootless 2-5-1 (cycle of fourths) | Bill Evans A/B voicings near middle C | 5 |
| 6th-diminished chords | Barry Harris' moving stairway, harmonized | 5 |
| Enclosures on 3rds | Surround the target, land it on the beat | 5 |

The jazz drills default the click to **beats 2 and 4** — the click is the
drummer's hi-hat, and beats 1 and 3 are yours to feel. `c` cycles the click
through every-beat, 2&4, and off.

### The two modes

- **wait** — time stops until you play the right note. Chords wait for every
  voice to be down. Learn the shapes and the fingerings here.
- **flow** — the metronome rolls (with a four-beat count-in) and every note
  you play is judged against the grid within a ±90 ms window: on it, early,
  or late, each hit marked as it lands.

### The numbers

After every rep the room reports:

- **bias** — your average signed timing error. Negative means you rush,
  positive means you drag.
- **spread** — how consistent you are around your own bias. A steady player
  with a lean beats a wobbly player centred on the beat.
- **evenness** — the variation of the gaps between your notes, as a
  percentage. Conservatory hands measure about 7%; below 8% is what
  "perfectly even" sounds like to a listener.

### Progress

- **Three clean reps in a row → the tempo climbs 5 BPM automatically.**
- Your **best clean tempo is saved per drill, per key, per hand** in
  `~/.phosphor/practice.json` — that number is your progress bar, and next
  session each drill starts at your record, not back at the floor.
- `<` / `>` walk the key through the **circle of fourths** (C → F → Bb …),
  the order jazz players practice all twelve keys in.

### Keys in the room

| Key | Action |
|-----|--------|
| `Space+F` | Open the room (on an instrument track) |
| `j` / `k` | Choose a drill |
| `<` / `>` | Walk the key, in fourths |
| `h` | Cycle hands: RH → LH → hands together |
| `w` | Toggle wait / flow |
| `c` | Click: every beat → 2&4 → off |
| `[` / `]` | Tempo down / up 5 BPM |
| `Enter` | Start / stop the drill |
| `Esc` | Stop the drill; again to leave the room |

---

## Shortcuts, A to Z

Every shortcut, organized by what you're trying to do — Loop under L, Record
under R. Each entry is the complete path from anywhere in the app, no steps
assumed. Four moves cover all the getting-around, so learn these first:

- **`Space+1` / `Space+2` / `Space+3`** jump to the three panes: transport,
  tracks, clip view. You can always start from one of these.
- **In the tracks pane, `j`/`k` choose a track and `Enter` selects it.**
  Selecting a track is the important act: it routes your MIDI keyboard to
  that instrument and opens its panels in the clip view. Once selected,
  `h`/`l` walk along its cells: label → fx → volume → mute → solo → arm →
  clips.
- **In the clip view, `Tab` cycles its tabs**: `[trk fx]` → `[synth]` →
  `[inst]` → `[piano roll]` → `[settings]`, then round again — with `[seq]`
  after `[synth]` on a sequencer track, and `[pads]` there on a sampler. Keep
  pressing `Tab` until the one you want is lit.
- **`Esc` always backs out one level** — releases a held knob, drops a
  selection, closes a panel, leaves a mode. Lost? Press `Esc` a few times
  and you're back on solid ground.

Below, "select the track" always means: `Space+2`, `j`/`k` to it, `Enter`.
"Open the piano roll" always means: select the track, then `Space+3` and
`Tab` until `[piano roll]` is lit. "Open the pad map" always means: select a
sampler track — it opens on `[pads]` by itself — or, if you have tabbed away,
`Space+3` then `Tab` until `[pads]` is lit.

**Add a track** — From anywhere: `Space+A`, choose an instrument with
`j`/`k`, `Enter`. The track arrives selected, panel open, MIDI routed to it.

**Add an effect** — Select the track, then `h`/`l` to its `fx` cell and
`Enter`: the effect menu opens. `j`/`k` to choose — audio effects first,
then `chord · midi` and `arp · midi`, which transform notes before the
instrument — and `Enter` adds it. (The same menu opens with `a` from inside
the `[trk fx]` tab.)

**Arm for recording** — Select the track (arming and selecting go together:
MIDI records onto the *selected* track), then press `r`. The dot glows
bright when the track will really receive notes, dim when it's armed but
not selected.

**Arpeggiator** — Add `arp · midi` (see Add an effect). To reach its knobs:
`Space+3`, `Tab` until `[trk fx]`, `j`/`k` onto the arp row, `Enter`. Then
`j`/`k` picks a knob, `h`/`l` turns it, `1`–`4` load instant feels (rhodes
8ths, dilla 16ths, wide updown, chord pulse), `Esc` backs out. The latch
knob keeps a chord running after your hands lift.

**Automation** — Open the piano roll, press `A`: a controller lane opens
under the notes and takes the keys. `k`/`j` draw the value up/down (`K`/`J`
bigger), `h`/`l` walk columns, `[`/`]` switch controllers (mod, bend,
aftertouch), `r` fills a straight ramp back to your last point, `d` clears
a point, `Esc` closes the lane. Wheels you *play* while recording are
captured automatically; `X` in the piano roll erases a clip's recorded
controllers.

**Chord device** — Add `chord · midi` (see Add an effect), then open its
panel exactly like the arpeggiator's: `Space+3`, `Tab` to `[trk fx]`,
`j`/`k` to the chord row, `Enter`. Knobs set the key, scale, color
(triad → 7th → 9th → lush) and voicing; below the split (C4) one key on
your controller plays that degree's whole chord, above it the keyboard
plays normally. Press `e` in this panel for your own progressions (see
Progressions).

**Clips (move, stretch, trim)** — Select the track, `h`/`l` along to its
clips, and on a clip press `Enter` to lock it. Now `h`/`l` moves it,
`Shift+H/L` stretches its right edge, `Ctrl+H/L` trims its left, `d`
duplicates it, `Esc` releases. Shrinking hides notes rather than deleting —
stretch back out and they return.

**Copy a clip** — Lock the clip as above (or just stand on it), `y` yanks
it, `p` pastes it after the current clip, `P` pastes onto *another* track
at the same bars. Pastes refuse to overlap existing clips.

**Copy an arrangement** — Select the track, `h`/`l` to its *label* cell,
`y` yanks every clip on the track. Then select the destination track,
`h`/`l` to its label, `P` lays the whole arrangement at the same bars —
the layering move.

**Copy the loop section** — See Loop: the brace lifts and stamps sections.

**Count-in** — `Space+1` to the transport, `h`/`l` to the `cnt` element,
`Enter` cycles off → 1 bar → 2 bars. With it set, arm recording
(`Space+R`) and press play (`Space+P`): the bars click down before the
take rolls. Any transport key cancels mid-count.

**Delete** — `Space+D` deletes whatever is selected — track or clip — with
a y/n confirmation. Inside modes, `d` deletes the thing under the cursor:
a note in edit mode, an effect in the chain, a chord row in the
progression editor, a sound on a sampler pad.

**Duplicate a track** — Select the track, press `D` (capital). Instrument,
panel, effects, mix and clips all copy to a fresh track directly below,
unarmed. One `u` removes it.

**Edit notes** — Open the piano roll, then `Space+E`. The cursor now hops
*between notes*: `h`/`l` nearest note left/right, `j`/`k` nearest above/
below. `Enter` selects the note under the cursor; `Shift+direction`
selects as you sweep. With notes selected, plain `h`/`l` moves them by a
grid step and `j`/`k` by a semitone; `Shift+H/L` stretches them. `,`/`.`
nudge velocity (`<`/`>` in strides), `m` mutes a note without deleting it,
`d` deletes, `Esc` steps back out, and `Esc` again leaves edit mode.

**Effect panels** — `Space+3`, `Tab` until `[trk fx]`: the chain lists
every effect on the selected track, MIDI effects on top. `j`/`k` chooses a
row, `Enter` opens its panel (`j`/`k` control, `h`/`l` adjust, `H`/`L`
strides, `Enter` holds a knob, `Esc` releases then closes), `m` mutes the
effect in place, `[`/`]` reorder audio effects, `d` removes with a
confirmation, `c` prints the MIDI effects into the clip as real notes.

**Fingers (practice room)** — Select an instrument track, then `Space+F`
from anywhere. `j`/`k` pick a drill, `<`/`>` walk the key through the
circle of fourths, `h` cycles RH → LH → hands together, `w` toggles wait
mode (time stops until you play the right note) and flow mode (the click
rolls and you're judged), `c` cycles the click (every beat / 2&4 / off),
`[`/`]` set the tempo, `Enter` starts, `Esc` stops and then leaves. Three
clean reps in a row raise the tempo 5 BPM; your best clean tempo is
remembered per drill, per key, per hand.

**Grid and snap** — Select the track, `Space+3`, `Tab` until `[settings]`:
`j`/`k` between grid, snap, default velocity and record quantize; `h`/`l`
change each. The loop brace's own grid is `g` inside the loop editor (see
Loop).

**Help** — `Space+H` from anywhere. `j`/`k` picks a topic, `Enter` opens
it, `Esc` closes.

**Jump** — In the piano roll, `g` jumps the cursor to the playhead's
column, landing on a sounding note. Digits `1`–`9` jump straight to a
numbered clip (selected track), column (piano roll), or step (sequencer).

**Layers (stacking sounds on a pad)** — Open the pad map, walk to a pad with
`h`/`l` (or play its key), and press `a` once for each sound you want on it —
up to eight, and they play together. Any phrases on the pad (see Phrases)
sit in the same list after them, four more at most. `[`/`]` pick which row
the panel's bottom controls address and `1`–`8` jump straight to one; a
sampled row you land on plays once on its own, so you can hear which is
which. `m` mutes one without taking it off the pad, `d` removes it after a
y/n, and `u` brings it back with its audio. The pad's `poly` control says how many hits
can overlap and `choke` puts pads in a mute group, the way a closed hat stops
an open one.

**Load a sound onto a pad** — Open the pad map, put the caret on a pad —
`h`/`l` walk a key, `H`/`L` an octave, or just play the key on your
controller — and press `a`. A list of `<app dir>/samples/` opens: `j`/`k` to
a sound, `Enter` lands it. Type letters to narrow the list, `h` goes up a
folder, `Enter` on a folder walks into it, `Esc` cancels. If this session has
recorded takes, its own `<session>.samples/` folder is the first row. For a
file somewhere no list would show it, `/` swaps to a typed path — where a
bare name like `kick` still looks in `<app dir>/samples/` and tries `.wav`.
The sound lands as a new layer and the panel points at it. WAV files only, up
to ten minutes. See [Sampler](#sampler--a-sound-on-every-key).

**Loop** — `Space+L` from anywhere focuses the loop brace; `Enter` toggles
looping on/off. `h`/`l` move the start marker, `H`/`L` the end, and `g`
cycles the marker grid — bar → beat → 1/8 → 1/16 — so the brace can close
down to a single sixteenth. `j`/`k` slide the whole brace along the song;
`J`/`K` leap it by its own length. Then the scissors: `y` lifts everything
between the markers on every track, `x` cuts it (one `u` restores every
track at once), `p` stamps the lifted section at the brace — replacing
what's under the stamp, then leapfrogging forward so `p p p` lays copies
back to back — and `P` layers it over what's there instead. `Esc` leaves
the brace focused where you put it.

**Metronome** — `Space+M` toggles the click while playing. The practice
room's click is its own (see Fingers).

**Mute** — `m` mutes whatever you're standing on: a selected track, a
sequencer lane, an effect in the chain or its open panel, a single note in
edit mode, a sound on a sampler pad. `s` solos tracks and lanes the same way.

**Normalize a sound** — Open the pad map, `[`/`]` to the sound you mean,
press `n`: its level is set so the trimmed region peaks at −0.5 dB. It is a
gain, never a rewrite — `n` again puts the level back to unity, and `u`
undoes either throw. A very quiet take can want more than the +12 dB the
level control has, and the flash says how far short it landed.

**Notes (writing)** — Open the piano roll. `h`/`l` walk columns, `j`/`k`
change pitch, and `n` writes a note at the cursor (`n` again erases it).
On an empty track the first note conjures its clip automatically. On the
sequencer grid, `n` toggles steps the same way.

**Octaves** — In the piano roll, `{`/`}` move the cursor a whole octave;
`[`/`]` snap it to the nearest pitch above/below that already has a note.

**Open / save** — `Space+S`, type a name like `myjam`, `Enter`: phosphor
adds `.phos` and saves into your projects folder, which the prompt names
above the field. `Ctrl+S` then saves that same file instantly. `Space+O`
opens a **list** of that folder — `j`/`k` to your project, `Enter` opens it;
type letters to narrow a long list, `h` walks up a folder, `Esc` closes. `/`
inside the list swaps it for a typed path, for a file kept somewhere else.
See [Where files live](#where-files-live).

**Panic** — `Space+!` from anywhere: all sound stops immediately.

**Phrases** — A take kept as notes instead of audio, replayed through one
instrument. Open the pad map, press `i` and pick an instrument, then `p`
until the banner reads `take: phrase`. `r` arms, play, `r` again: the notes
land as a `phr` row in the sound list, four to a pad, one `u` deep. The
sampler has **one** child instrument for all of them — landing a phrase
points it at what you just played, and a phrase from a different instrument
replaces it for every phrase on the kit, which the flash says out loud. A
phrase's controls are `vel`, `mute` and `keytrk` and nothing else; `[`/`]`,
`1`–`8`, `m` and `d` reach its row the same as any other. See
[Phrases](#phrases--the-performance-instead-of-the-audio).

**Play / stop** — `Space+P` toggles play; `Space+0` stops and returns the
playhead to bar 1.

**Presets** — Select an instrument track, `Space+W`: its preset browser
opens. `j`/`k` rows, `Enter` on a preset loads it, `Enter` on the save row
names and saves the current panel, `d` deletes (confirmed), `Esc` closes.

**Progressions** — Open the chord device's panel (see Chord device), press
`e`: the progression editor opens. `j`/`k` picks a chord row, `Tab` the
column (root / quality / bass), `h`/`l` turns it. `a` adds a chord, `d`
removes one, `r` arms learn — play any chord on your controller, lift, and
that exact voicing lands in the row. `[`/`]` browse your saved library,
`n` names the progression, `s` saves it, `Enter` loads it into the device,
`Esc` closes.

**Quantize** — Open the piano roll on the clip, then `Space+Q`: `j`/`k`
between grid, strength (25–100%) and apply, `h`/`l` adjusts, `Enter` on
apply commits. To quantize *as you record*, set record quantize in the
`[settings]` tab.

**Record** — Arm a track (see Arm), press `Space+R` to arm the transport,
then `Space+P` to roll — with a count-in first if you set one. `R` on a
selected track starts loop-recording; `R` while already playing punches in
right there, no rewind. `Space+1` → the `take` element chooses overdub
(each pass layers) or re-record (the loop range clears first). While
recording, `u` scraps the pass under your fingers and the loop keeps
rolling.

**Redo** — `Ctrl+R`, everywhere.

**Rename a track** — Select the track, `h`/`l` to its label cell, press
`n`, type the name (8 characters max), `Enter`.

**Resample (record an instrument onto a pad)** — Open the pad map, stand on
the pad you want, press `i`, `j`/`k` to an instrument, `Enter`. The track now
plays that instrument instead of the sampler, through its own effects and
fader, and your keys are a performance. `r` arms, you play, `r` lands the
take on the pad — named, selected, one `u` away. Stopped, the take starts on
your first note and rings out to silence; rolling (`Space+P`), it is cut to
whole bars so it loops. Playing one pitch teaches the pad its root. `Tab`
takes you off the map to `[inst]`, which while the mode is on is that
instrument's own panel — patch selector and all — so the sound you record is
the sound you dialled, and the pad keeps the panel you dialled it on. `i`
changes the instrument, `Esc` ends a running take and `Esc` again puts the
sampler back with its kit. A take becomes a file the next time you save —
into `<session>.samples/`, which is the first row of the `a` list from then
on, so a take is reachable from any other pad in one `Enter`.

**Sampler** — `Space+A` and choose *Sampler*: the track arrives with its pad
map open and the keyboard drawn across it, eighty-eight pads, one per key.
`h`/`l` walk the bed and `H`/`L` an octave — or play a key and the caret goes
there. `a` opens a list of your samples folder and puts the WAV you choose on
the pad under the caret (`/` types a path instead), `t` trims it, `i`
records it off another instrument, `n` normalizes it, `d` removes it. `j`/`k`
picks a control — trigger, polyphony, choke group, pitch, ADSR, level, pan,
root, keytracking, then the selected sound's own six — `Enter` holds it,
`h`/`l` turns it, `H`/`L` strides, `Esc` lets go. Holding is the only time
`h`/`l` stop walking the keyboard. `K` turns the same bed into zones (see
Zones). Full section: [Sampler](#sampler--a-sound-on-every-key).

**Sequencer** — `Space+A` and choose *Step Sequencer*: it arrives with its
grid open. On the grid `j`/`k`/`h`/`l` move, `n` writes a step, `a`
accents it, `x` clears it, `[`/`]` jump rows, and `y`/`p` copy one step
with its chord, gate and accent to another spot. `j` below the grid
reaches the panels: the step's pitch/chord/voicing/gate, the lane's sound,
the pattern's length/rate/swing — `h`/`l` between knobs, `Enter` holds
one. On the slots row, `h`/`l` picks one of eight patterns, `Enter` queues
it, `c` chains it (`c` again for ×2), `C` clears the chain, and `y`/`p`
copy a whole pattern — onto another sequencer track too. `t` runs/stops,
`r` step-records from your controller (`.` writes a rest, `_` ties), `b`
bounces the pattern to a clip, `X` clears the pattern.

**Solo** — `s`, on a selected track or a sequencer lane.

**Tempo** — `+`/`-` from anywhere. Or `Space+1`, `Enter` on the BPM
element, `h`/`l` to walk it, `Esc` to release.

**Theme** — `Space+V` cycles the color theme.

**Trim a sound (sampler)** — Open the pad map, `[`/`]` to the sound you mean,
press `t`. The file is drawn as a waveform with the part that plays lit:
`h`/`l` move the start, `H`/`L` move the end, and `j`/`k` walk how far one
press moves — bar, beat, 1/16, 10 ms, 1 ms, one sample. Every start nudge
plays the region from its new start. `z` toggles the zero-crossing snap, `r`
plays it backwards, `t` loops it while you work, `Esc` goes back. The audio
is never rewritten, and a whole run of nudges is one press of `u`.

**Undo** — `u`, for everything: notes, knobs, effects, takes, cuts,
stamps, pads and zones. A sweep of one knob folds into a single step. While recording,
`u` scraps the in-flight pass first, then peels committed takes
newest-first.

**Velocity** — Notes draw brighter the harder they were hit, so dynamics
are visible at a glance. To change them: edit mode (see Edit notes),
`,`/`.` nudges, `<`/`>` strides, and the header reads the value out. The
velocity new notes get is in the `[settings]` tab.

**Yank** — `y` lifts, everywhere: a clip, a whole arrangement (from the
label cell), highlighted piano-roll notes, a sequencer step or pattern,
the loop section. `p` puts it down; wherever two flavors exist, lowercase
`p` replaces and capital `P` layers.

**Zones (a sampler across the keyboard)** — Open the pad map and press `K`:
the bed stops being one sound per key and becomes zones, a stretch of keys
playing one sound transposed from a root. The bar reads `-- KEYS --`. `w`
throws a zone across the whole bed, `o` across the octave the caret is in,
and `a` then picks a sound for it out of the list — a file name ending in a note
(`Piano_C3.wav`, `kick_A#1.wav`) sets the root on the way in, and `R` learns
it instead from the next key you play. `s` splits the zone at the caret, so a
keyboard can be several samples wide; the left half keeps its root, the right
is rooted at its own first key. To resize one, `Enter` on the `span` control
at the top of the panel, then `h`/`l` for the low edge and `H`/`L` for the
high. `D` takes a zone off the bed after a y/n, and `u` brings it back with
its audio. `K` again returns to pads — neither bed destroys the other, and a
session keeps both. Full section:
[Sampler](#sampler--a-sound-on-every-key).

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
| `Space` `0` | Stop and return to bar 1 |
| `Space` `r` | Toggle recording |
| `Space` `l` | Edit loop region |
| `Space` `m` | Toggle metronome |
| `Space` `!` | Panic — kill all sound |
| `Space` `a` | Add instrument track |
| `Space` `s` | Save project — asks for a name; the prompt says which folder it writes into |
| `Space` `o` | Open project — a list of that same folder; `/` types a path instead |
| `Space` `d` | Delete selected track/clip (with confirmation) |
| `Space` `e` | Enter edit mode (note-level piano roll editing) |
| `Space` `q` | Quantize notes to grid |
| `Space` `w` | Instrument presets — save / load / delete |
| `Space` `v` | Cycle color theme |
| `Space` `h` | Open help — twelve topics, `Enter` opens one as a reference card, `j`/`k` scrolls it |

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

The kit, the patch and the cartridge are stored by which one they are rather than by
where the knob sat, for the same reason a session stores them that way: a knob
position only names a patch while the bank is the size it was when the preset was
written, and a preset that reopens on a different drum machine is the kind of wrong
that looks perfectly reasonable. Presets written before this still load — the knob
position is the only evidence they carry — and the status bar says to check the
patch when one does.

### File Picker — Space+O, and `a` on a sampler pad

| Key | Action |
|-----|--------|
| `j` / `k` | Move the cursor — arrows, `Ctrl+N`/`Ctrl+P` and `PgUp`/`PgDn` too |
| `Enter` | A folder: walk into it. A file: open the project, or land the sound |
| `h` | Up one folder (`←` too) — it stops at the top of the filesystem |
| `g` / `G` | Top / bottom of the list |
| *typing* | Narrow the list: letters, digits, space, `.`, `-`, `_` |
| `Backspace` | Widen it again — and on an empty filter, up one folder |
| `/` | Swap the list for a typed path |
| `Esc` | Close, choosing nothing |

One folder at a time, listed: folders first and then files, each half
alphabetical whatever case it was typed in, with dot-prefixed names skipped.
Files are filtered by what the picker is for — `.phos` for projects, `.wav`
for sounds, either case — and **folders are always shown**, so you can walk
from where it opened to wherever you actually keep things.

`j` and `k` are the way down a list and also letters in a filename, and it
cannot be both at once: while nothing has been typed they walk the list, and
from the first letter typed they are letters and the arrows move the cursor.
The footer at the bottom of the box says which of the two it is in.

On a sampler, this session's own takes folder — `<session>.samples/` — is the
first row of the list whenever it exists, so a recording made ten minutes ago
is one `Enter` away rather than a walk out of the samples folder.

It does not search, it does not recurse, and selecting a row plays nothing.
An empty folder says what to do about it rather than showing an empty box,
and a folder that cannot be read says that instead of looking empty.

### Step Sequencer (a track type — drives any instrument)

A pattern sequencer in the TR/Elektron lineage. It makes no sound of its own:
it drives a **child instrument** — the drum rack by default, or any other
instrument in the rack, the sampler included — and it is sample-locked to the
transport, so a pattern step and a
clip note on the same beat land on the same sample.

**First beat in thirty seconds:**

1. `Space` `a` → **Step Sequencer** → `Enter`. You land on the grid.
2. `n` writes a hit. `h`/`l` move along the steps. Write a few.
3. `j`/`k` move between the rows — the kit's sounds (`BD` `SD` `CH`…), or a
   synth's eight voices. Write a hat line against the kick.
4. `t` — it plays. A light chases across every row and wraps.
5. `Space` `0` stops and returns to bar 1.

**The screen:**

```
 ▶ step 7 of 16  slot A · Drum Rack        ← what the machine is doing
     1  2  3  4   5  6  7  8   9 10 …      ← step ruler, grouped by beat
 BD ▓▓ ░░ ░░ ░░  ▓▓ ░░ ░░ ░░  ▓▓ ░░       ← one row per sound; ▓▓ hit,
▸SD ░░ ░░ ░░ ░░  ██ ░░ ░░ ░░  ░░ ░░          ██ accent, ▸ = row being written
 CH ░░ ░░ ▓▓ ░░  ░░ ░░ ▓▓ ░░  ░░ ░░
lane SD   sound ◑ SD 38  mute ○ off …      ← the row's own controls
pattern   child ○ Drum Rack  steps ◑ 16 …  ← the pattern's controls
  slots  ▶A  B  C  D  E  F  G  H  chain —  ← eight patterns, queue, chain
```

`j`/`k` walk down the rows and keep going into the panels below — lane,
pattern, slots — and `k` walks back up. `h`/`l` move along whatever row you
are on. `n` writes a step; `Enter` **opens** whatever the cursor is standing
on — the row's panel on a kit, the step's panel on a synth — and `Enter`
again **holds** the knob under it (like the volume fader): while held,
`h`/`l` adjust it, `H`/`L` take bigger strides, `Esc` lets go.

**Changing which sounds the rows play.** The eight rows start as kick, snare,
hats, clap and toms, but every kit has more — rimshot, crash, ride, cowbell,
percussion. Walk `j` down to the **lane** panel, `Enter` on the `sound` knob,
then `h`/`l` step through every sound in the kit one at a time (`H`/`L` jump
an octave of notes at once). The row's name follows the sound — `BD` becomes
`RS`, `CR`, `RD`, `CB`… (a sound with no short name shows its note number).
`Esc` releases. The pattern keeps playing while you do this.

**Sequencing a synth instead of drums.** Walk `j` to the **pattern** panel.
Its first knob is `child` — the instrument this sequencer drives. `Enter`,
then `h`/`l` cycle through everything in the rack: the DX7, the Jupiter-8,
the Prophet-6, the Phatty, all of them. (The **Sampler** is in the list too,
but a kit belongs to a track that *is* a sampler: as a sequencer's child it
arrives with eighty-eight empty pads, no `[pads]` tab and only its two
globals, and both the flash and the `[inst]` panel say so. Give the sampler a
track of its own and play it from the piano roll or your controller.) The
rows become eight voices —
`L1` through `L8` — and the panel above the pattern row becomes the **step**
panel:

- `pitch` — what the step plays. `h`/`l` walk semitones, `H`/`L` jump
  octaves. With a **mode** active (see below) `h`/`l` walk scale degrees
  instead, and the readout shows both: `iii·E4`.
- `chord` — from a single note through maj/min/dim/sus/6ths/7ths/quartal
  4ths, plus **diatonic** (the chord quality follows the scale degree).
- `voicing` — close, drop-2, first or second inversion; `root↓` doubles the
  root an octave down.
- `gate` — how long the note holds, up to **TIE**, which holds it into the
  next step (the 303 slide feel).

The readout line under the step panel always names what the step will play:
`Cm7 · C4 D#4 G4 A#4`. The child's own panel (patch, cutoff, everything)
stays on the left side of the screen — pick the patch there as usual.

**Layering chords across the rows.** The eight rows are eight independent
voices on the same step, which is how you build a chord the chord table has
no name for: put a `maj7` on `L1`, walk `j` to `L2`, write the same step and
run its pitch up to the ninth — now the step plays a five-note ninth chord.
Each row keeps its own pitch, chord, voicing and gate, and `m`/`s` mute or
solo one of them while the rest keep playing.

**Mode and key** (pattern panel): choose Dorian, Phrygian, Lydian… and a
tonic, and the pitch knob snaps to the scale; chords set to *diatonic* pick
their own quality per degree, so a progression stays in key by itself.

**Accent and feel** (pattern panel): `a` on a step makes it hit at the
`accent` velocity instead of `base`. `swing` delays the off-beats,
MPC-style. `steps` masks rather than deletes — shorten 16 → 8 and the hidden
half comes back when you lengthen again. `rate` runs from quarters to
sixteenth triplets; 12 or 24 steps give 3/4 and shuffle feels.

**Patterns, slots, chains** (slots row): eight patterns per sequencer, `A`
through `H`. `h`/`l` choose one to look at, digits `1`–`8` jump. `Enter`
queues the slot to take over **at the end of the current pattern** — the
header counts it down. `c` chains the slot under the cursor (press again for
×2, ×3…), building an arrangement like `A×4 B×2 A×2`; the chain plays in
order and loops. `C` clears it. `y`/`p` copy a pattern from one slot to
another.

**Bounce** (`b`): compiles the pattern — or the whole chain — into a real
clip on the timeline at the next free bar, and stops the live pattern so
nothing plays twice. The clip is then ordinary: edit it in the piano roll,
undo it with `u`.

**Step record** (`r`): arm it and play your MIDI keyboard — each key writes
its pitch to the step under the cursor and moves on; hold several keys and
the step gets the chord, named in the readout. `.` writes a rest (skips a
step), `_` ties the previous step. `r` again to disarm.

**All keys, in one place:**

| Key | Where | Action |
|-----|-------|--------|
| `h` / `l` | everywhere | Move along steps / knobs / slots; adjust a held knob |
| `j` / `k` | everywhere | Down / up: the rows, then lane, pattern, slots panels |
| `n` | grid | Write / erase the step under the cursor |
| `a` | grid | Accent it |
| `Enter` | grid | Open the panel for what is under the cursor · panels: hold the knob · slots: queue |
| `H` / `L` | held knob | Big strides (octaves, ±5 swing, ±10 velocity…) |
| `[` / `]` | everywhere | Previous / next sound row, from any depth |
| `x` | everywhere | Clear the step under the cursor |
| `m` / `s` | everywhere | Mute / solo the row being written |
| `t` | everywhere | Play — while stopped it always starts everything; while playing it mutes/unmutes this pattern |
| `r`, `.`, `_` | everywhere | Step record: arm · rest · tie |
| `b` | everywhere | Bounce pattern or chain to a clip |
| `c` / `C` | everywhere | Chain the slot under the cursor (repeat to stack) / clear the chain |
| `y` / `p` | everywhere | Copy / paste a pattern between slots |
| `digits` | grid / slots | Jump to a step / jump to a slot |
| `X` | everywhere | Clear the whole pattern |
| `Esc` | everywhere | Release knob → leave panel → leave the sequencer |
| `Space` `p` | global | Play — a fresh pattern runs from birth, so this alone makes sound |
| `Space` `0` | global | Stop and return to bar 1 |

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

### Effects (`[trk fx]` tab and the panel behind a slot)

Six insert slots on every track, every bus and the master. The `[trk fx]` tab
is the chain; `Enter` on a slot opens that effect's panel in the wide pane
beside it. Adding an effect drops it at its canonical place in the chain
(`EQ → comp → tape → delay → reverb`) and never moves what is already there.

| Key | Where | Action |
|-----|-------|--------|
| `j` / `k` | chain | Move between slots |
| `a` | chain | Add an effect · `d` removes one (it asks) |
| `Enter` | chain | Open the slot's panel |
| `b` | chain, panel | Bypass — the slot stays, the effect steps aside |
| `[` / `]` | chain | Move the slot earlier / later |

**The EQ panel.** Eight bands and an output trim. At 120 columns the bands are
columns with the response curve drawn over them, from the filter's own
closed-form response at the engine's sample rate; at 80 the bands become rows
and the curve is dropped — the numbers are what you mix with.

| Key | Action |
|-----|--------|
| `h` / `l` | The bands (the band's controls, when they are rows) |
| `j` / `k` | The band's controls: type, freq, gain, Q, slope, on |
| `1`–`8` | Jump to a band · `n` switches it on or off |
| `Enter` | Hold the control · `h`/`l` turns it, `H`/`L` in strides |
| `Esc` | Let go, then leave the panel for the chain |

Frequencies walk the ISO sixth-octave centres, so a band reads `2.5k` and never
`2487`; a stride is an octave of them. Gain moves 0.5 dB a press and 3 dB with
a stride. A control the band type does not use — a bell has no slope, a matched
shelf no Q — is greyed and will not move.

**The reverb and delay panels** are columns of knobs rather than grids, so
`j`/`k` picks one and `h`/`l` turns it straight away; `Enter` still holds, and
holding is what stops `j`/`k` walking off the control being turned.

**The delay** keeps two axes apart that most delays conflate. `mode` is what
the repeats sound like — `digital`, `bbd`, `tape` — and `route` is where they
go — `stereo`, `ping-pong`, `mono`. Any combination: a tape ping-pong is a
real setting here.

| Control | What it does |
|---------|--------------|
| `sync` / `div` / `time` | Follow the tempo on a musical division, or run free in milliseconds. Switching between them **carries the current time over** rather than jumping to a hidden value, and a division too long for the five-second line is halved until it fits and says `clamped` |
| `tmode` | `auto` resolves per mode — a digital delay crossfades, a bucket brigade and a tape repitch, because their clock rate *is* their delay time. `repitch`, `fade` and `jump` override it |
| `fb` | 0–200%. Past 100% the loop sings rather than running away: the in-loop saturator bounds it at `|in| + fb/g` by arithmetic rather than by a clamp. The readout shows the repeat count while there is one, and `sings` when there is not |
| `locut` / `hicut` | Two one-poles **inside the loop**, on at 200 Hz and 6 kHz, so each repeat darkens a little more than the last and the echoes recede rather than repeating a static copy |
| `freeze` | Input off, loop gain exactly one, filters and saturator out of the path — the buffer cycles unchanged rather than darkening away |
| `duck` | One knob, no threshold. The dry input keys an envelope follower that pulls the wet down, to 24 dB at the top of the knob, and never touches the feedback — so the repeat count does not vary with the performance |
| `heads` | Three tape heads at 1 : 2 : 3, seven combinations. Tape mode only; greyed elsewhere |
| `wander` | How far the bucket brigade's clock drifts. BBD only |

In `bbd` the loop's low-pass corner follows the clock — `4096 / 2τ`, a third of
the way up — so a longer delay is a darker one and it compounds: 5 kHz falls
2.4 dB a repeat at 120 ms and 11 dB a repeat at 600 ms. In `tape` the wow is
*multiplied* into the delay time the way a real echo's capstan does, so a long
setting warbles harder than a short one; the flutter and the scrape are added,
because a head's stick-slip does not stretch the tape between two heads.

**Send B ships with the delay** on a new session, synced to a dotted eighth at
100% wet, and the track strip calls that bus `dly`.

**Pan and sends** are cells on the track row, after the record-arm switch:
`h`/`l` reaches them, `Enter` holds one, `h`/`l` moves it. Sends are post-fader
and open from silence. The top bar shows the safety limiter's gain reduction
whenever the mix is loud enough to need it, and nothing when it is not.

### Instrument Panel (`[inst]` tab)

The instrument's own controls, laid out in as many columns as the pane has
room for — three at 120 columns, which is what makes an 84-control panel like
the Prophet-6 readable. The first control is always the patch selector.

| Key | Action |
|-----|--------|
| `j` / `k` | Move between controls (down each column, then over) |
| `h` / `l` | Turn the control under the cursor — a knob by a step, a selector to the next position |
| `Tab` | Next tab (piano roll) |
| `Esc` | Back to the tracks pane |

It is the same panel as the narrow `[synth]` strip on the left and the same
cursor, so moving in either moves in both; the tab simply has the room. A
patch selector reloads the whole panel, and every value of it reaches the
audio thread.

### Piano Roll — Navigation Mode

| Key | Action |
|-----|--------|
| `h` / `l` | Navigate between columns (beats) |
| `j` / `k` | Scroll up/down through notes |
| `1-9` | Jump to column by number |
| `Enter` | Select column (enter edit mode) |
| `n` | Toggle note at cursor (draw or remove) — on an empty track this makes the clip, and the status bar says so |
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

Theme choice is saved to `<app dir>/config.json` and persists across sessions.

---

## Where files live

Everything phosphor owns — presets, the theme preference, and sessions you have
not given a path of your own — sits in one directory:

| Platform | Application directory |
|---|---|
| macOS, Linux, BSD | `$HOME/.phosphor` |
| Windows | `%APPDATA%\phosphor`, falling back to `%USERPROFILE%\AppData\Roaming\phosphor` |

Set `PHOSPHOR_HOME` to put it somewhere else — a portable install on a USB
stick, or a scratch directory while you are experimenting. It names the
directory itself, not a parent.

Inside it:

```
<app dir>/config.json                    theme preference
<app dir>/presets/<instrument>.json      one user preset bank per instrument
<app dir>/samples/                       the folder `a` lists on a sampler pad
<app dir>/sessions/                      the folder `Space+O` lists, and a name saves into
<app dir>/progressions.json              your chord-progression library
<app dir>/practice.json                  practice-room records (clean BPM per drill)
```

A sampler path *typed* into the `/` prompt is looked for as you typed it
first, then in the working directory, then in `<app dir>/samples/`, then in
the app directory itself, with `.wav` tried at each step — so `kick` finds
`samples/kick.wav`. A sound chosen from the list needs none of that: the row
carries the whole path.

Recorded takes are the one thing that does not live here. They are written
beside the session that holds them, in `<session>.samples/` — so `myjam.phos`
keeps its recordings in `myjam.samples/`, and the two move together. The
sampler's `a` list puts that folder at the top of itself, so a take is one
`Enter` away from any other pad.

Both of these folders are lists rather than paths to remember: `Space+O`
shows the sessions, `a` on a sampler pad shows the samples. See
[File Picker](#file-picker--spaceo-and-a-on-a-sampler-pad).

### How to save and open — the short version

**To save:** press `Space+S`, type a name — just the name, like `myjam` —
and press `Enter`. Phosphor adds **`.phos`** to the end for you and puts
the file in the `sessions/` folder. From then on `Ctrl+S` saves that same
file instantly.

**To open:** press `Space+O`. A list of that same folder opens — your
projects, by name. `j`/`k` to the one you want and `Enter` opens it. Nothing
to type and nothing to remember.

### The details

- A session is one file with the **`.phos`** extension — human-readable JSON.
- You never have to type the extension — saving appends `.phos` to whatever
  you enter (a wrong extension is corrected: `mysong.txt` saves as
  `mysong.phos`), and the picker only lists `.phos` files anyway.
- The save prompt asks for a **name**, not a path. The field starts empty
  with a dim suggestion — `untitled.phos` — where the name goes, and the
  line under it says which folder the file is going into. The suggestion is
  a fallback, not pre-typed text: type and it is yours, or press `Enter` on
  an untouched prompt to take it.
- After the first save, `Ctrl+S` saves straight back to the same file, no
  prompt. `Space+S` always prompts, for saving a copy under a new name.
- Names have no other rules — anything your filesystem accepts works. Paths
  are still allowed: anything with a `/` in it is written exactly there, so
  `ideas/jam.phos` saves into an `ideas` folder.
- The open picker lists one folder at a time and walks: `Enter` on a folder
  goes into it, `h` comes back out, typing narrows a long list. For a
  project kept somewhere no list would show it, `/` swaps to a typed path —
  the prompt this replaced, and it still resolves the way it always did.
  Full keys: [File Picker](#file-picker--spaceo-and-a-on-a-sampler-pad).
- A sampler stores each sound's **path**, never its audio, and both beds —
  the pad map and the zones. A path that has moved keeps its pad rather than
  being dropped: the key goes red and the layer list says `missing`.

The folder a name is saved into and the folder the picker opens on are the
same folder: `sessions/` when the working directory has one — running from a
checkout, which is where the sessions in this repository already are — and
the absolute `<app dir>/sessions/` otherwise. It is made if it is not there
yet. A relative path typed into the `/` prompt is looked for in the working
directory first and then under the application directory, so
`sessions/take3.phos` keeps working from anywhere.

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
  - **Linux**: ALSA for the DAW itself (`sudo apt install libasound2-dev pkg-config`);
    building the whole workspace also wants the GUI stub's windowing headers
    (`libudev-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev`) — the exact
    list CI uses is in `.github/workflows/ci.yml`
  - **Windows**: WASAPI (included)
- MIDI support requires a connected MIDI device (optional)

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test --workspace  # 2,047+ tests
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
│   │       ├── synth.rs       # Phosphor Synth, wavetable/vector (229 patches)
│   │       ├── dx7.rs         # DX7 FM synthesizer (256 factory voices from the ROMs)
│   │       ├── jupiter.rs     # Jupiter-8 analog poly (64 factory patches)
│   │       ├── odyssey.rs     # ARP Odyssey duophonic (44 patches)
│   │       ├── juno.rs        # Juno-60 DCO + BBD chorus (56 factory patches)
│   │       ├── rhodes.rs      # Rhodes tine piano, modal physical model (26 patches)
│   │       ├── phatty.rs      # Little Phatty mono Moog, morphing oscillators (100 patches)
│   │       ├── prophet6.rs    # Prophet-6 analog poly with poly mod (500 factory programs)
│   │       ├── p6_programs.bin # The factory programs, from Sequential's SysEx
│   │       ├── teo5.rs        # Oberheim TEO-5, SEM morphing filter + TZFM (256 factory programs)
│   │       ├── teo5_programs.bin # The factory programs, from Oberheim's SysEx
│   │       ├── sampler/       # Sampler: pads, voices, preview playback
│   │       ├── drum_rack/     # Drum machine (18 kits)
│   │       │   ├── mod.rs     # Shared types, voice, plugin impl
│   │       │   └── racks/     # Per-kit synthesis: 808, 909, 707, 606, 727, CR-78,
│   │       │                  #   LinnDrum, DMX, SDS-V, 777, tsty1-5, and the three
│   │       │                  #   acoustic kits (jazz, funk, studio)
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
│   │       │   ├── sampler_*.rs   # Pad map, keys mode, trim strip, resampling
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
│   │       │   ├── clip_view.rs   # Piano roll, FX panel, instrument panel
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
    --buffer-size <N>     Request an audio block size in samples
    --sample-rate <N>     Request a sample rate in Hz
    --no-audio            Disable audio output
    --no-midi             Disable MIDI input
    -h, --help            Print help
    -V, --version         Print version
```

`--buffer-size` and `--sample-rate` have no defaults on purpose. Left off,
phosphor renders natively at whatever the output device is already set to. Pin
a rate the hardware is not at and the platform quietly inserts a sample-rate
converter in the output path — on macOS the HAL resamples between the audio
unit and the device — so you get conversion artifacts and added latency with
nothing on screen to say so. Ask for a rate and it is used if the device offers
it; if not, the device's own is adopted and the difference is reported on the
status bar rather than left to be discovered by ear. `--no-audio` has no device
to follow and runs at 44100 / 64.

### Debug Logging

```bash
PHOSPHOR_DEBUG=1 cargo run --release
```

Creates `phosphor_debug.log` with timestamped user actions and system responses. Includes a panic handler that captures full backtraces to the log.

It is written to the working directory when that is writable, otherwise to the
application directory, otherwise to the system temp directory; if none of those
will take it, phosphor says so on stderr and carries on without a log. The file
is capped at 8 MB and starts over rather than growing without bound.

### Theme Persistence

Theme selection is saved to `<app dir>/config.json` and automatically loaded on startup.

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
