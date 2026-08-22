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
use phosphor_dsp::{drum_rack, dx7, juno, jupiter, odyssey, phatty, rhodes, synth};
use phosphor_plugin::{MidiEvent, Plugin};

const SAMPLE_RATE: f64 = 44_100.0;
const BLOCK: usize = 256;
/// 2.3 s — long enough for the slowest pad attack in the four banks.
const BLOCKS: usize = 400;
/// 9.3 s, for the tables that have to see a sound effect's peak. The Juno's
/// 77 SURF reaches its loudest 4.7 s into a held chord and the Jupiter's 61
/// STARTING UP at 5.5 s, both by design — a slow LFO and a long delay are
/// what those patches are.
const HELD_BLOCKS: usize = 1600;

struct Measured {
    peak: f32,
    percent_at_full_scale: f64,
    rms: f32,
}

fn render(plugin: &mut dyn Plugin, notes: &[u8], velocity: u8) -> Measured {
    render_for(plugin, notes, velocity, BLOCKS)
}

fn render_for(plugin: &mut dyn Plugin, notes: &[u8], velocity: u8, blocks: usize) -> Measured {
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

    for block in 0..blocks {
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
        rms: (energy / (blocks * BLOCK) as f64).sqrt() as f32,
    }
}

const VOICINGS: [(&str, &[u8]); 5] = [
    ("single", &[60]),
    ("triad", &[60, 64, 67]),
    ("7th", &[60, 64, 67, 71]),
    ("wide5", &[36, 48, 60, 64, 67]),
    ("8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
];

const INSTRUMENTS: [(&str, usize); 7] = [
    ("dx7", dx7::VOICE_COUNT),
    ("jupiter", jupiter::PATCH_COUNT),
    ("odyssey", odyssey::PATCH_COUNT),
    ("juno", juno::PATCH_COUNT),
    ("rhodes", rhodes::PATCH_COUNT),
    ("phatty", phatty::PATCH_COUNT),
    ("phosphor", synth::PATCH_COUNT),
];

/// The loudest preset of each bank, measured on an eight-note chord at
/// velocity 127. Re-derive with the `scan` mode after editing a bank.
const LOUDEST: [(&str, usize); 7] = [
    ("dx7", 147),
    ("jupiter", 40), // 61 STARTING UP, whose peak needs `BLOCKS` past 5.5 s
    ("odyssey", 1),
    ("juno", 27), // 44 TUBA
    ("rhodes", 22), // Hard Bark
    ("phatty", 85), // Blip
    ("phosphor", 8), // SYNTH KIT
];

fn build(name: &str, patch: usize) -> Box<dyn Plugin> {
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
            // 64 patches: the knob has to land on the midpoint of a step
            // rather than on its edge, or the sweep measures the patch
            // before — it did that for seven of this bank's indices when it
            // held 42.
            s.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(patch));
            Box::new(s)
        }
        "odyssey" => {
            let mut s = odyssey::OdysseySynth::new();
            // 44 patches: the knob has to land on the midpoint of a step
            // rather than on its edge, or the sweep measures the patch before.
            s.set_parameter(odyssey::P_PATCH, odyssey::patch_knob(patch));
            Box::new(s)
        }
        "juno" => {
            let mut s = juno::Juno60Synth::new();
            // 56 patches: the knob has to land on the midpoint of a step
            // rather than on its edge, or the sweep measures the patch before.
            s.set_parameter(juno::P_PATCH, juno::patch_knob(patch));
            Box::new(s)
        }
        "rhodes" => {
            let mut s = rhodes::RhodesPiano::new();
            s.set_parameter(rhodes::P_PATCH, rhodes::patch_knob(patch));
            Box::new(s)
        }
        "phatty" => {
            let mut s = phatty::LittlePhatty::new();
            s.set_parameter(phatty::P_PATCH, phatty::patch_knob(patch));
            Box::new(s)
        }
        _ => {
            let mut s = synth::PhosphorSynth::new();
            s.set_parameter(synth::P_PATCH, synth::patch_knob(patch));
            Box::new(s)
        }
    }
}

fn patch_name(name: &str, index: usize) -> &'static str {
    match name {
        "dx7" => dx7::voice_name(index),
        "jupiter" => jupiter::PATCH_LABELS[index],
        "odyssey" => odyssey::PATCH_NAMES[index],
        "juno" => juno::PATCH_LABELS[index],
        "rhodes" => rhodes::PATCH_NAMES[index],
        "phatty" => phatty::PATCH_NAMES[index],
        "phosphor" => synth::PATCH_NAMES[index],
        _ => "-",
    }
}

fn table(title: &str, pick: impl Fn(&str) -> usize) {
    println!("\n== {title} ==");
    println!("{:<9} {:<4} {:<8} {:<5} {:>8} {:>8}", "synth", "pidx", "voicing", "vel", "peak", "%clamp");
    for (name, _) in INSTRUMENTS {
        let index = pick(name);
        for (voicing, notes) in VOICINGS {
            for velocity in [100u8, 127] {
                let mut plugin = build(name, index);
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
///
/// Held for 9.3 s rather than 2.3, because the two banks that carry sound
/// effects hide their loudest patch behind a slow envelope: the Jupiter's 61
/// STARTING UP measures 33 times more over the long window than the short
/// one, and a ranking that cannot see it would name the wrong patch.
fn scan() {
    println!("== preset ranking (8-note chord @127, 9.3 s hold) ==");
    let chord: &[u8] = &[36, 43, 48, 55, 60, 64, 67, 72];
    for (name, count) in INSTRUMENTS {
        let mut ranked: Vec<(usize, f32, f32)> = (0..count)
            .map(|index| {
                let mut plugin = build(name, index);
                let m = render_for(plugin.as_mut(), chord, 127, HELD_BLOCKS);
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
const MEDIAN: [(&str, usize); 7] = [
    ("dx7", 8), ("jupiter", 1), ("odyssey", 0), ("juno", 3), ("rhodes", 13),
    ("phatty", 78), ("phosphor", 0),
];

/// The worst case each bank can be driven to. Voicing included, because the
/// duophonic Odyssey does not stack the way the polys do and its worst case
/// is not reliably a chord — most of its bank peaks louder on a single note,
/// and `tests/headroom.rs` sweeps it on both.
const WORST: [(&str, usize, &str, &[u8]); 7] = [
    ("dx7", 147, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    // 61 STARTING UP. Its peak lands 5.5 s into a held chord, so it is the
    // one entry here that `BLOCKS` has to be long enough to see.
    ("jupiter", 40, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    ("odyssey", 1, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    ("juno", 27, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]),
    ("rhodes", 22, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]), // Hard Bark
    // Monophonic, so a chord is one note: this bank's worst case is a single
    // note in the top of the keyboard rather than a stack.
    ("phatty", 85, "single", &[93]), // Blip
    ("phosphor", 8, "8note", &[36, 43, 48, 55, 60, 64, 67, 72]), // SYNTH KIT
];

/// Kick, snare and closed hat on one sample — a downbeat, which is the drum
/// rack's equivalent of a triad.
const DRUM_ORDINARY: &[u8] = &[36, 38, 42];
/// A full kit struck together, which is what a quantised fill lands as.
const DRUM_WORST: &[u8] = &[36, 38, 41, 42, 45, 46, 49, 51];

fn drum_rack(kit: usize) -> drum_rack::DrumRack {
    let mut rack = drum_rack::DrumRack::new();
    rack.set_parameter(drum_rack::P_KIT, drum_rack::kit_knob(kit));
    rack
}

/// The three numbers the gain structure is judged on.
///
/// * **Ordinary** — the level the user actually hears while playing. The
///   target is a peak near −12 dBFS: loud enough to be usable without the OS
///   volume control, quiet enough that several tracks still sum cleanly.
/// * **Median RMS** — what the ear tracks, and what keeps the instruments
///   from stepping in level as you switch between them.
/// * **Worst** — the loudest thing the bank can produce, over a 9.3 s hold
///   rather than the 2.3 s the other tables use, because two of the entries
///   are sound effects whose peak arrives after five seconds. `pre-sat` is
///   the trimmed voice sum before the bounding stage; `sat` is how much the
///   saturator took off it. Anything but 0.00 there means the bounding stage
///   is working, which is what it is for.
fn stage() {
    println!("\n== ordinary playing: default preset, triad @100 ==");
    println!("{:<10} {:>9} {:>8} {:>9} {:>8}", "instrument", "peak", "dBFS", "rms", "dBFS");
    for (name, _) in INSTRUMENTS {
        let index = if name == "dx7" { 10 } else { 0 }; // the DX7 loads E.PIANO 1
        let mut plugin = build(name, index);
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
        let mut plugin = build(name, index);
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
        let mut plugin = build(name, index);
        let m = render_for(plugin.as_mut(), notes, 127, HELD_BLOCKS);
        let raw = saturation_input(m.peak);
        println!(
            "{name:<10} {index:>4} {voicing:<8} {:>9.4} {:>8.1} {:>9.4} {:>7.2}",
            m.peak, db(m.peak), raw, db(m.peak) - db(raw)
        );
    }
    {
        // The loudest kit is found by measurement, not by memory.
        let mut worst = (0usize, 0.0f32);
        for kit in 0..drum_rack::KIT_COUNT {
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
