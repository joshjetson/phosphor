//! Output level measurement for the built-in instruments.
//!
//! Renders the voicings people actually play through every instrument and
//! reports the peak and the share of samples pinned at full scale — the two
//! numbers that decide whether the headroom trims are still right after a
//! preset is added or revoiced.
//!
//! ```text
//! cargo run --release -p phosphor-dsp --example levels          # default presets
//! cargo run --release -p phosphor-dsp --example levels -- loud  # loudest of each bank
//! cargo run --release -p phosphor-dsp --example levels -- scan  # rank every preset
//! cargo run --release -p phosphor-dsp --example levels -- stage # the gain structure
//! ```
//!
//! `tests/headroom.rs` asserts the same thing; this prints the numbers.

use phosphor_dsp::level::saturation_input;
use phosphor_dsp::{drum_rack, dx7, juno, jupiter, odyssey, synth};
use phosphor_plugin::{MidiEvent, Plugin};

const SAMPLE_RATE: f64 = 44_100.0;
const BLOCK: usize = 256;
/// 2.3 s — long enough for the slowest pad attack in the four banks.
const BLOCKS: usize = 400;

struct Measured {
    peak: f32,
    percent_at_full_scale: f64,
    rms: f32,
}

fn render(plugin: &mut dyn Plugin, notes: &[u8], velocity: u8) -> Measured {
    plugin.init(SAMPLE_RATE, BLOCK);
    plugin.reset();
    let events: Vec<MidiEvent> = notes
        .iter()
        .map(|&note| MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: velocity })
        .collect();

    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut peak = 0.0f32;
    let mut pinned = 0u64;
    let mut total = 0u64;
    let mut energy = 0.0f64;

    for block in 0..BLOCKS {
        left.fill(0.0);
        right.fill(0.0);
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        if block == 0 {
            plugin.process(&[], &mut outs, &events);
        } else {
            plugin.process(&[], &mut outs, &[]);
        }
        for sample in left.iter().chain(right.iter()).copied() {
            let mag = sample.abs();
            peak = peak.max(mag);
            if mag >= 1.0 {
                pinned += 1;
            }
            total += 1;
        }
        for sample in &left {
            energy += f64::from(*sample) * f64::from(*sample);
        }
    }

    Measured {
        peak,
        percent_at_full_scale: 100.0 * pinned as f64 / total as f64,
        rms: (energy / (BLOCKS * BLOCK) as f64).sqrt() as f32,
    }
}

const VOICINGS: [(&str, &[u8]); 5] = [
    ("single", &[60]),
    ("triad", &[60, 64, 67]),
    ("7th", &[60, 64, 67, 71]),
    ("wide5", &[36, 48, 60, 64, 67]),
    ("8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
];

const INSTRUMENTS: [(&str, usize); 5] = [
    ("dx7", dx7::VOICE_COUNT),
    ("jupiter", jupiter::PATCH_COUNT),
    ("odyssey", odyssey::PATCH_COUNT),
    ("juno", juno::PATCH_COUNT),
    ("phosphor", 1),
];

/// The loudest preset of each bank, measured on an eight-note chord at
/// velocity 127. Re-derive with the `scan` mode after editing a bank.
const LOUDEST: [(&str, usize); 5] = [
    ("dx7", 147),
    ("jupiter", 9),
    ("odyssey", 1),
    ("juno", 27), // 44 TUBA
    ("phosphor", 0),
];

fn build(name: &str, patch: usize, count: usize) -> Box<dyn Plugin> {
    let value = if count > 1 { patch as f32 / (count as f32 - 0.01) } else { 0.0 };
    match name {
        "dx7" => {
            let mut s = dx7::Dx7Synth::new();
            // Two selectors, so a sweep index is a cartridge and a voice.
            let (bank, voice) = dx7::voice_knobs(patch);
            s.set_parameter(dx7::P_BANK, bank);
            s.set_parameter(dx7::P_PATCH, voice);
            Box::new(s)
        }
        "jupiter" => {
            let mut s = jupiter::Jupiter8Synth::new();
            s.set_parameter(jupiter::P_PATCH, value);
            Box::new(s)
        }
        "odyssey" => {
            let mut s = odyssey::OdysseySynth::new();
            s.set_parameter(odyssey::P_PATCH, value);
            Box::new(s)
        }
        "juno" => {
            let mut s = juno::Juno60Synth::new();
            // 56 patches: the knob has to land on the midpoint of a step
            // rather than on its edge, or the sweep measures the patch before.
            s.set_parameter(juno::P_PATCH, juno::patch_knob(patch));
            Box::new(s)
        }
        _ => Box::new(synth::PhosphorSynth::new()),
    }
}

fn patch_name(name: &str, index: usize) -> &'static str {
    match name {
        "dx7" => dx7::voice_name(index),
        "jupiter" => jupiter::PATCH_NAMES[index],
        "odyssey" => odyssey::PATCH_NAMES[index],
        "juno" => juno::PATCH_LABELS[index],
        _ => "-",
    }
}

fn table(title: &str, pick: impl Fn(&str) -> usize) {
    println!("\n== {title} ==");
    println!("{:<9} {:<4} {:<8} {:<5} {:>8} {:>8}", "synth", "pidx", "voicing", "vel", "peak", "%clamp");
    for (name, count) in INSTRUMENTS {
        let index = pick(name);
        for (voicing, notes) in VOICINGS {
            for velocity in [100u8, 127] {
                let mut plugin = build(name, index, count);
                let m = render(plugin.as_mut(), notes, velocity);
                println!(
                    "{name:<9} {index:<4} {voicing:<8} {velocity:<5} {:>8.4} {:>8.2}",
                    m.peak, m.percent_at_full_scale
                );
            }
        }
    }
}

/// Rank every preset of every bank on the worst voicing, so the loudest is
/// found by measurement rather than by memory.
fn scan() {
    println!("== preset ranking (8-note chord @127) ==");
    let chord: &[u8] = &[36, 43, 48, 55, 60, 64, 67, 72];
    for (name, count) in INSTRUMENTS {
        let mut ranked: Vec<(usize, f32, f32)> = (0..count)
            .map(|index| {
                let mut plugin = build(name, index, count);
                let m = render(plugin.as_mut(), chord, 127);
                (index, m.peak, m.rms)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        for (index, peak, rms) in ranked.iter().take(5) {
            println!(
                "  {name:<9} {index:>3} {:<10} peak {peak:.4} rms {rms:.5}",
                patch_name(name, *index)
            );
        }
        println!();
    }
}

// ── Gain structure ──

/// Linear amplitude as dBFS. Silence reports as `-inf`.
fn db(x: f32) -> f32 {
    if x <= 0.0 { f32::NEG_INFINITY } else { 20.0 * x.log10() }
}

/// The reference voicing for "ordinary playing": a triad at velocity 100 on
/// the preset the instrument loads with.
const ORDINARY: &[u8] = &[60, 64, 67];

/// The median preset of each bank by triad RMS — the same indices
/// `tests/headroom.rs` uses to check the instruments are level-matched, so
/// the RMS column here and that assertion move together.
const MEDIAN: [(&str, usize); 5] =
    [("dx7", 8), ("jupiter", 1), ("odyssey", 4), ("juno", 3), ("phosphor", 0)];

/// The worst case each bank can be driven to. Voicing included, because the
/// duophonic Odyssey peaks on a single note rather than on a chord.
const WORST: [(&str, usize, &str, &[u8]); 5] = [
    ("dx7", 147, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    ("jupiter", 9, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    ("odyssey", 1, "single", &[60]),
    ("juno", 27, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    ("phosphor", 0, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
];

/// Kick, snare and closed hat on one sample — a downbeat, which is the drum
/// rack's equivalent of a triad.
const DRUM_ORDINARY: &[u8] = &[36, 38, 42];
/// A full kit struck together, which is what a quantised fill lands as.
const DRUM_WORST: &[u8] = &[36, 38, 41, 42, 45, 46, 49, 51];
const DRUM_KIT_COUNT: usize = 10;

fn drum_rack(kit: usize) -> drum_rack::DrumRack {
    let mut rack = drum_rack::DrumRack::new();
    rack.set_parameter(drum_rack::P_KIT, kit as f32 / (DRUM_KIT_COUNT as f32 - 0.01));
    rack
}

/// The three numbers the gain structure is judged on.
///
/// * **Ordinary** — the level the user actually hears while playing. The
///   target is a peak near −12 dBFS: loud enough to be usable without the OS
///   volume control, quiet enough that several tracks still sum cleanly.
/// * **Median RMS** — what the ear tracks, and what keeps the instruments
///   from stepping in level as you switch between them.
/// * **Worst** — the loudest thing the bank can produce. `pre-sat` is the
///   trimmed voice sum before the bounding stage; `sat` is how much the
///   saturator took off it. Anything but 0.00 there means the bounding stage
///   is working, which is what it is for.
fn stage() {
    println!("\n== ordinary playing: default preset, triad @100 ==");
    println!("{:<10} {:>9} {:>8} {:>9} {:>8}", "instrument", "peak", "dBFS", "rms", "dBFS");
    for (name, count) in INSTRUMENTS {
        let index = if name == "dx7" { 10 } else { 0 }; // the DX7 loads E.PIANO 1
        let mut plugin = build(name, index, count);
        let m = render(plugin.as_mut(), ORDINARY, 100);
        println!(
            "{name:<10} {:>9.4} {:>8.1} {:>9.5} {:>8.1}",
            m.peak, db(m.peak), m.rms, db(m.rms)
        );
    }
    {
        let m = render(&mut drum_rack(0), DRUM_ORDINARY, 100);
        println!(
            "{:<10} {:>9.4} {:>8.1} {:>9.5} {:>8.1}",
            "drums", m.peak, db(m.peak), m.rms, db(m.rms)
        );
    }

    println!("\n== level match: median preset, triad @100 ==");
    println!(
        "{:<10} {:>4} {:>9} {:>8} {:>9} {:>8}",
        "instrument", "pidx", "peak", "dBFS", "rms", "dBFS"
    );
    let mut rms_values = Vec::new();
    for (name, index) in MEDIAN {
        let count = INSTRUMENTS.iter().find(|(n, _)| *n == name).map_or(1, |(_, c)| *c);
        let mut plugin = build(name, index, count);
        let m = render(plugin.as_mut(), ORDINARY, 100);
        rms_values.push(m.rms);
        println!(
            "{name:<10} {index:>4} {:>9.4} {:>8.1} {:>9.5} {:>8.1}",
            m.peak, db(m.peak), m.rms, db(m.rms)
        );
    }
    {
        let m = render(&mut drum_rack(0), DRUM_ORDINARY, 100);
        println!(
            "{:<10} {:>4} {:>9.4} {:>8.1} {:>9.5} {:>8.1}",
            "drums", 0, m.peak, db(m.peak), m.rms, db(m.rms)
        );
    }
    let quietest = rms_values.iter().copied().fold(f32::MAX, f32::min);
    let loudest = rms_values.iter().copied().fold(0.0f32, f32::max);
    println!("keyboard spread: {:.1} dB", db(loudest) - db(quietest));
    println!(
        "drums are matched on peak, not RMS — an RMS figure over a fixed\n\
         window compares a one-shot that decays inside it against a pad that\n\
         does not, so it says nothing. See the peaks in the next table."
    );

    println!("\n== worst case each bank can reach, @127 ==");
    println!(
        "{:<10} {:>4} {:<8} {:>9} {:>8} {:>9} {:>8}",
        "instrument", "pidx", "voicing", "peak", "dBFS", "pre-sat", "sat"
    );
    for (name, index, voicing, notes) in WORST {
        let count = INSTRUMENTS.iter().find(|(n, _)| *n == name).map_or(1, |(_, c)| *c);
        let mut plugin = build(name, index, count);
        let m = render(plugin.as_mut(), notes, 127);
        let raw = saturation_input(m.peak);
        println!(
            "{name:<10} {index:>4} {voicing:<8} {:>9.4} {:>8.1} {:>9.4} {:>7.2}",
            m.peak, db(m.peak), raw, db(m.peak) - db(raw)
        );
    }
    {
        // The loudest kit is found by measurement, not by memory.
        let mut worst = (0usize, 0.0f32);
        for kit in 0..DRUM_KIT_COUNT {
            let m = render(&mut drum_rack(kit), DRUM_WORST, 127);
            if m.peak > worst.1 {
                worst = (kit, m.peak);
            }
        }
        let raw = saturation_input(worst.1);
        println!(
            "{:<10} {:>4} {:<8} {:>9.4} {:>8.1} {:>9.4} {:>7.2}",
            "drums", worst.0, "8pad", worst.1, db(worst.1), raw, db(worst.1) - db(raw)
        );
    }
}

fn main() {
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "scan" => scan(),
        "stage" => stage(),
        "loud" => table("loudest preset of each bank", |name| {
            LOUDEST.iter().find(|(n, _)| *n == name).map_or(0, |(_, i)| *i)
        }),
        _ => table("default preset", |_| 0),
    }
}
