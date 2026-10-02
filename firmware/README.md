# Phosphor Deck firmware

The program for the Phosphor Deck controller's Raspberry Pi Pico 2. It reads
every knob, button, fader and pad, and sends them to Phosphor as a USB MIDI
device named **Phosphor Deck** — the name Phosphor recognises it by.

You only need this if you are building a deck. Phosphor works the same
without one.

## Load it onto a Pico 2

1. Install the tools once:
   ```
   rustup target add thumbv8m.main-none-eabihf
   brew install picotool        # or your Linux distribution's picotool package
   ```
2. Hold the Pico's **BOOTSEL** button while plugging it in.
3. From this folder:
   ```
   cd deck-pico
   cargo run --release
   ```
   It builds, loads and starts. Plug the deck into a computer running
   Phosphor 0.3.91 or newer and the bottom bar says
   `MIDI: Phosphor Deck connected`.

Without picotool's `load`, `picotool uf2 convert` turns the built file into a
`.uf2` you can drag onto the drive a Pico in BOOTSEL mode shows up as.

## What is where

- `deck-logic/` — everything the deck decides, as plain Rust that tests on
  any computer: when a bouncing switch counts as pressed, how a knob's two
  pins become clicks, how hard a pad was hit, when a fader has really moved,
  and what the lights show. `cargo test` here runs it.
- `deck-pico/` — the Pico 2 program around it: the pins, the scan every
  500 µs, USB MIDI and the LED chain.
- `deck-layout.json` — Phosphor's own control table, written by
  `phosphor --deck-layout`. The firmware's table of which input is which
  control is generated from it at build time, and a test in Phosphor fails if
  the two drift apart.

`cargo run --example scenario` plays a scripted performance — a button with
contact bounce, a knob turned, a fader slid, a pad struck — through the
logic and prints the MIDI it sends, so the firmware's decisions can be piped
into Phosphor with no board at all.

## Tuning

At the top of `deck-pico/src/main.rs`: `SETTLE` (how long a switch must hold,
5 ms) and `FLIP_ENCODERS` (set it if every knob counts backwards). The pads'
thresholds are `deck-logic/src/pad.rs`'s `DEFAULT`: raise `on` if pads trigger
by themselves, lower `full` if the hardest hit never reaches 127.
