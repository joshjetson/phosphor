# Rebranding & Hardware — taking Phosphor to market

Phosphor ships today as free MIT software named after the hardware it models. Selling a physical
product changes the risk profile completely: it gets press, it substitutes directly for the
originals, it can be delisted by a single marketplace complaint or seized at customs, and a
cease-and-desist against software is a `git commit` while one against 500 units in a warehouse is
your capital.

This is the concise version of what has to change, and what to build it on.

*Not legal advice. Get IP counsel before committing to tooling.*

---

# Part I — Branding and legal

## 1. Three separate problems, three different answers

| | Exposure | Renaming fixes it? |
|---|---|---|
| **Sounding like the originals** | **None.** No IP right covers timbre. Chowning's FM patent expired ~1995, Moog's ladder patent mid-80s. u-he, Cherry Audio, GForce, TAL and Behringer all stand on this. | n/a — recognition *is* the marketing |
| **Using their names** | **Real.** Live marks held by Yamaha, Roland, Focusrite/Sequential, inMusic/Moog, Korg, Rhodes Music Group, Simmons, Oberheim | Yes |
| **Shipping their data** | **Real — and renaming does not touch it.** This is the one that gets missed. | No |

People recognising your Juno as a Juno is the goal, not the liability. What gets companies sued is
names, logos, panel trade dress, and copied data or code.

## 2. What ships that isn't ours

| Item | Path | Size | Action |
|---|---|---|---|
| Yamaha DX7 factory ROM — 256 voices + their names | `crates/phosphor-dsp/src/dx7_roms.bin` | 32,768 B | Delete → `.syx` import |
| Sequential P6 factory programs — 500 | `crates/phosphor-dsp/src/p6_programs.bin` | 63,500 B | Delete → `.syx` import |
| Korg microKORG Voice Name List — 128 names verbatim | `crates/phosphor-dsp/tests/data/microkorg_voices.json`, used by `synth/bank.rs` A.11–B.88 | 20,921 B | Rename all 128 |
| DX7 decoder oracle — 14 decoded factory voices | `crates/phosphor-dsp/tests/data/dx7_reference.json` | 15,476 B | Replace with self-authored fixtures |
| Roland Juno-60 factory names ×56, incl. their "Glockenspeil" typo | `juno.rs:401` `BANK` | — | Rename |
| Roland Jupiter-8 factory names + 8×8 numbering ×64 | `jupiter.rs:515` `BANK` | — | Rename |
| Bank labels `microKORG` / `MINIMOOG` / `WAVESTATN` | `synth/bank.rs:164` `BANK_NAMES` | — | Rename |

**Reproducing Roland's typo is the detail that reads as deliberate to a judge.** Short names aren't
individually copyrightable; the verbatim set, in their order, with their errors, is a compilation
copy *and* evidence of intent.

Note that even the flagship original instrument isn't clean: Phosphor Synth's bank selector reads
`PHOSPHOR / microKORG / MINIMOOG / WAVESTATN` — three trademarks — and 128 of its 229 patches carry
Korg's factory names.

**Already clean — keep as the template:** Little Phatty's 100 patches, Odyssey's 44, Rhodes' 26,
Phosphor Synth's own 11, the Minimoog and Wavestation sets (authored — neither machine has a
transcribable factory bank), all `tsty-N` kits, and every drum voice.

**No audio assets exist anywhere in the repository.** The sampled machines model the *converter*
— µ-255 companding, 6-bit at 18 kHz, 8-bit at 25 kHz, the read clock as tuning — not the content.
The kit files say so outright: *"These are not Roland's samples"*, *"not Linn's samples"*, *"not
Oberheim's samples."* That is exactly the right posture and it should be the whole project's posture.

## 3. The Dexed/msfa exposure

`crates/phosphor-dsp/src/dx7.rs:677` states the 32-algorithm routing table was *"decoded from the
Yamaha DX7 algorithm table as encoded in Dexed/msfa `FmCore::algorithms[32]` (fm_core.cc)"*.

- **msfa** is Apache-2.0 → attribution obligation, and there is no NOTICE file in the repo
- **Dexed** is **GPLv3** → incompatible with MIT and with a closed hardware product

The Rust is not a port — it's 32 hand-written struct literals — and the routing table is arguably an
uncopyrightable fact about the hardware. But the comment documents a *channel of copying*, which is
what a plaintiff needs. The same applies to `VELOCITY_DATA`, `EXP_SCALE_DATA`, `LFO_RATE_HZ`,
`PITCH_ENV_RATE`, `PITCH_MOD_SENS` and `AMP_MOD_SENS`, which msfa/Dexed also carry and which are
unattributed here.

**Fix:** re-derive from the DX7 owner's and service manuals, which print all 32 algorithm diagrams,
and re-cite those. Cheap now; removes GPL from the conversation before hardware exists.

## 4. Where the names live

Every user-visible name is in two tables:

- `crates/phosphor-app/src/state/menu.rs:113` — `InstrumentType::label()`, and `:129` `description()`
- `crates/phosphor-dsp/src/drum_rack/mod.rs:388` — `KIT_LABELS`

Three secondary surfaces:

- `crates/phosphor-app/src/state/track_ops.rs:122` — 5-char strip names (`jup8`, `phaty`, `p6`…)
- Nine `PluginInfo.name` literals, one per engine's `info()`
- `crates/phosphor-app/src/session.rs:95` `instrument_key()` — **this is the save format and the
  preset filename** (`preset.rs:239`). Needs a migration, not a rename.

Enum variants (`DX7`, `Jupiter8`, `LittlePhatty`…) appear ~250 times across 12 files — mechanical
and rustc-guided. Module and type names (`phatty.rs`, `Jupiter8Synth`, `RhodesPiano`) plus the
extensive doc comments in `phosphor-dsp` are a separate and much larger axis.

### Candidate names — starting points, not answers

| Current | Suggested | Rationale |
|---|---|---|
| Phosphor Synth | *keep* | already ours (but rename its banks) |
| DX7 | **Carillon** | bells are the instrument's signature |
| Jupiter-8 | **Meridian** | 8-voice flagship, no planet |
| Odyssey | **Wanderer** | Korg currently sells the ARP Odyssey; "Odyssey" is the distinctive word |
| Juno-60 | **Halo** | the BBD chorus *is* the instrument |
| Rhodes | **Tine** | descriptive, accurate, unowned |
| Little Phatty | **Lowboy** | mono, low, ladder |
| Prophet-6 | **Hexad** | six voices |
| Sampler | *remove or build* | see §7 |

| Kit | → | Kit | → |
|---|---|---|---|
| 808 | **Boom** | linn | **Session** |
| 909 | **Rave** | dmx | **Electro** |
| 707 | **Deck** | sds-v | **Hexpad** |
| 606 | **Pocket** | cr-78 | **Combo** |
| 727 | **Latin** | 777, tsty-1..5, jazz, funk, studio | *keep* |

**Avoid semantic clones.** Prophet → Oracle is *worse* than a neutral word: confusing similarity
includes similarity in meaning.

**"Phatty-like" is worse than a clean rename.** "Phatty" is the distinctive element of the Moog mark,
so a near-variant is still a use of the mark — and it proves you knew, which turns innocent
infringement into willful, where enhanced damages and fee-shifting live.

## 5. Language rules for code, docs and marketing

- **Describe by circuit and era, never by brand.** "Transistor ladder, switchable 1/2/3/4-pole."
  "Six-operator FM, 32 algorithms." "Single DCO with a BBD chorus." "SSM2040-lineage low-pass."
  More accurate, what a knowledgeable buyer actually searches for, and it names zero trademarks.
- **Keep the source citations.** The manuals, service notes, Sound On Sound reviews and JASA papers
  cited throughout `phosphor-dsp` are evidence of *independent development*. That's a defense, not a
  liability. Do not strip them.
- **Nothing comparative on packaging.** "Sounds exactly like a Prophet-6" is the loudest possible
  signal. Reviewers will say it for free, and them saying it costs you nothing.
- **Add a trademark disclaimer.** There is currently none anywhere in the repo.
- Product names must be entirely yours. Nominative reference in prose ("in the tradition of…") is
  legally defensible but is a defense you pay to assert — keep it off the box regardless.

## 6. Ship the importer, not the payload

`crates/phosphor-dsp/examples/p6_rom.rs` already decodes Sequential's SysEx format, and the DX7
32-voice bank format is publicly documented. **Keep the decoders, drop the data.**

"Imports standard 6-operator FM SysEx banks" and "imports Prophet-format program dumps" are
legitimate interoperability features. The customer brings their own file; you distribute nothing of
Yamaha's or Sequential's. Backfill the out-of-box patch count with original banks.

**Do not** ship clean and point users at a download. Inducing infringement is its own cause of
action and it proves knowledge — strictly worse than shipping the files.

## 7. Product-specific legal notes

- **Nothing silkscreened.** No model numbers, no brand references anywhere on the enclosure.
- **Don't reach for TR-808 orange/red/yellow**, or any recognisable panel layout. Trade dress and —
  in Germany — the unfair-competition doctrine against "slavish imitation" reach product
  *appearance* even absent an IP right. Verify with counsel; keep the enclosure distinctively yours.
- **Ship the dependency license texts.** All deps are permissive, but `epaint_default_fonts` carries
  OFL-1.1 + LicenseRef-UFL-1.0 attribution obligations. Putting software on a device is distribution.
- **The `Sampler` menu entry has no engine behind it** — `crates/phosphor-tui/src/app/tracks.rs:131`
  maps `Synth | Sampler => PhosphorSynth::new()`. Harmless in free software; on a spec sheet for a
  sold product it's a claim that isn't true. Build it or remove it.

## 8. Order of operations

1. **De-brand the OSS repo in one release, before any hardware announcement.** An afternoon now.
   Later it's exhibit A — a public repo with your name on it, holding `dx7_roms.bin`.
2. **Author replacement banks.** The real cost of going commercial. Little Phatty's 100 prove it's
   doable and establish the house voice.
3. **Clearance search on "Phosphor" itself** — it's a common word — then register the mark.
4. **IP counsel review before tooling.** A few thousand dollars against a seized container.

---

# Part II — Hardware platform

## 9. Measure before choosing

Before any platform decision: get real CPU numbers. `crates/phosphor-dsp/benches/dsp_bench.rs`
(criterion) and `crates/phosphor-dsp/examples/levels.rs` already exist. Profile a realistically
loaded session — Prophet-6 at 6 voices, the drum rack, two or three more tracks — as a percentage of
one core at the target buffer size.

The likely finding is that you are 10× over-provisioned on a Pi 5, and that single number decides
everything below. It's an afternoon of work and it's the highest-leverage step in the whole project.

## 10. Why the Pi 5 is the wrong member of the family

| Issue | Detail |
|---|---|
| **No analog audio out at all** | The 3.5 mm jack is gone and PWM audio isn't broken out. A DAC is mandatory, not a fallback |
| **I/O moved behind the RP1 southbridge** (over PCIe) | Audio-HAT support needed driver catch-up; Pi 4 HAT compatibility is not a given |
| **Thermals** | Sustained load wants active cooling. A fan in a musical instrument is a defect |
| **Power** | Wants 5 V / 5 A for full peripheral current — a 27 W supply for a device using part of one core |
| **Cost** | ~$80 for the board alone, before PSU, DAC, storage, display and controls |

Availability is the *good* part — Raspberry Pi has committed to Pi 5 production into the mid-2030s.
It's the form factor that's wrong, not the vendor.

Also worth knowing: **PREEMPT_RT is now merged into mainline Linux** (6.12), so proper real-time
scheduling on ARM is no longer an out-of-tree patch exercise. You still need `isolcpus`,
`threadirqs`, IRQ affinity and a well-behaved codec, but the kernel side is solved.

## 11. The options

### Prototype in weeks — Pi 4 or CM4 + Pisound + Patchbox OS

Blokas' **Pisound** is a Pi HAT with 24-bit stereo in/out and **MIDI DIN in and out**, and they ship
**Patchbox OS**, a Pi distribution already tuned for low-latency audio. Audio, MIDI and the RT kernel
are solved on day one, and Blokas supports OEM use. Fastest path to a unit you can put in front of
people and validate the product with.

### Ship v1 — Compute Module 4 or 5 on a custom carrier

This is the real product answer. You design one board carrying the codec, the ADCs, the display
connector, the MIDI jacks and the power regulation.

- No stacked HATs, no ribbon spaghetti
- Dramatically better EMC — and you **will** be paying for radiated-emissions testing
- **Onboard eMMC instead of an SD card that dies**
- Industrial lifecycle (CM4 committed into the mid-2030s)
- Lower per-unit BOM

Cost: roughly $5–15k for a good 4–6 layer carrier with clean analog, less if you work from a
reference design. CM4 is cheaper, cooler and fanless with a mature ecosystem; go CM5 only if the
benchmark says you need it.

**Useful:** Raspberry Pi acquired IQaudIO and now sells the **Codec Zero** and **DAC Pro** in-house —
same lifecycle guarantees as the CM, and **the schematics are published**, so you can drop that exact
codec circuit onto your carrier rather than reverse-engineering one.

### Worth benchmarking — Pi Zero 2 W

Quad A53 at 1 GHz, ~$15. Probably marginal for a full session. But if it runs, the BOM story changes
completely. Cheap to find out, and the benchmark from §9 answers it directly.

### Steal ideas from, don't build on — Bela

BeagleBone-based, Xenomai, sub-millisecond round-trip latency, and **8 channels of 16-bit analog
input built in** — designed for exactly the pots and sliders you want. Right architecture, wrong CPU
for a full DAW. Study their analog input design.

### If VST hosting is ever on the roadmap — Intel N100/N150

~$100–150, ~6 W, mature audio stack, standard PC compliance path, no ARM porting at all. Bigger,
hotter at idle, and no GPIO so everything goes over USB. Only worth it if plugin hosting matters.

### Avoid — Rockchip / Radxa

Better price/performance on paper, but audio driver support is the weak spot and the entire product
promise is "always works out of the box."

## 12. The control surface

Put **all** of it behind an **RP2040 (or RP2350) presenting as a USB-MIDI class-compliant device.**

- `phosphor-midi` already handles that path — zero new plumbing on the Linux side
- The MCU does ADC scanning, debouncing, filtering and hysteresis in firmware, where jitter is
  harmless
- ~$1 for the chip, and it goes on the carrier board
- Gives you the 5-pin DIN jacks cheaply
- The panel can be developed and tested independently of the Pi

**Encoders, not pots.** With 84 parameters on the Prophet-6 panel alone, an absolute pot's physical
position lies the moment you change patch. Detented endless encoders with push map cleanly onto the
existing lock/release navigation model.

**The real gap:** there is currently **no control-surface layer in Phosphor at all.**
`midi_to_plugin_event()` (`crates/phosphor-core/src/mixer.rs:677`) forwards CC straight to the
instrument — no CC→parameter mapping, no MIDI learn, no controller profile, no transport or mixer
control from MIDI. `ProgramChange` is parsed and deliberately dropped. This is a bigger piece of work
than the audio backend and it sits on the critical path.

## 13. Display and boot

- **Small DSI panel, framebuffer console.** No X, no Wayland, no compositor — a getty booting
  straight into `phosphor`. SPI panels (ST7789/ILI9341 via fbtft) are cheaper but too slow to refresh
  the per-track VU meters.
- **Read-only rootfs with overlayfs**, `~/.phosphor` on a separate writable partition, A/B updates.
  Users will yank power, and "always works out of the box" means surviving that.
- **eMMC or NVMe. Not SD.**

## 14. Software gaps on the critical path

| Gap | Location |
|---|---|
| No MIDI control-surface layer — no CC mapping, MIDI learn, or transport/mixer control | `phosphor-core/src/mixer.rs:677`, `phosphor-midi` |
| `CpalBackend` doesn't implement the `AudioBackend` trait — only `TestBackend` does, so the second-backend seam is drafted but not load-bearing | `phosphor-core/src/audio.rs:21`, `cpal_backend.rs:204` |
| No ARM target in CI (`ubuntu` / `macos` / `windows-latest` only) | `.github/workflows/ci.yml:41` |
| No appliance run mode — `--no-audio` is a test mode, not headless operation | `phosphor-tui/src/app/mod.rs` |
| `phosphor-gui` is a 669-byte stub | `crates/phosphor-gui/src/lib.rs` |
| `Sampler` menu entry has no engine behind it | `phosphor-tui/src/app/tracks.rs:131` |

What ports well as-is: the lock-free `Plugin`/`Mixer` core, the `AudioRequest`/`StreamFormat`
device negotiation in `cpal_backend.rs:37-100` (which resolves the device's own rate rather than
pinning one), and the fact that the TUI never touches audio state directly.

## 15. Recommended path

1. Benchmark (§9) — one afternoon, decides everything else
2. Prototype on Pi 4 or CM4 + Pisound + Patchbox OS; build the RP2040 panel in parallel
3. Build the control-surface layer in Phosphor — the largest software item
4. Add aarch64 to CI, implement `AudioBackend` for `CpalBackend`, add an appliance run mode
5. Design the CM4/CM5 carrier once the panel and audio path are proven on the prototype
6. EMC pre-scan before committing to an enclosure
