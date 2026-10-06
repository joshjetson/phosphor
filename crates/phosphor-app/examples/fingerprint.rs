//! A fingerprint of every sound Phosphor makes, for proving that a change
//! made for speed changed none of them.
//!
//!     cargo run --release -p phosphor-app --example fingerprint -- save before.txt
//!     (make the change)
//!     cargo run --release -p phosphor-app --example fingerprint -- check before.txt
//!
//! Every instrument plays the same short phrase — a note, two drum-range
//! hits, a four-note chord at four velocities, the mod wheel, a pitch bend,
//! channel pressure, releases and a tail — once for every factory sound it
//! ships (all 256 DX7 voices, all 500 Prophet-6 programs, all 256 TEO-5
//! programs, every patch and kit), and once more on its default panel for
//! every control, turned mid-phrase, so that a cached value that failed to
//! follow its knob is heard. Every effect plays a fixed test signal through
//! every position of its selectors and every control moved to each end of its
//! travel mid-signal. What is recorded for each is a hash of the exact bits
//! of the output: a change of one bit in one sample is a different
//! fingerprint.
//!
//! `check` renders everything again and lists every sound whose output
//! changed. Zero changed is the proof; anything else is a list of what to go
//! and listen to.
//!
//! **The file is a local before-and-after tool, not a golden file.** The
//! exact bits depend on the platform's maths library (`sin`, `exp`, `powf`
//! are not correctly rounded and differ between macOS, Linux and Windows) and
//! on the CPU, so a table saved on one machine will not match another even
//! when nothing has changed. Save and check on the same machine, with the
//! same build settings.
//!
//! `ftz` renders everything twice, as plain code and inside the engine's
//! flush-to-zero guard ([`phosphor_core::denormal::NoDenormals`]), and
//! reports how far apart the two come out — which is how much difference the
//! guard makes to what anyone hears.
//!
//! `save` and `check` render inside that guard, because that is how the
//! engine plays; `--plain` renders without it. `--only <text>` limits any of
//! these to the sounds whose name contains the text, e.g. `--only Prophet`.

use std::sync::Arc;

use phosphor_app::factory;
use phosphor_app::state::{FxType, InstrumentType};
use phosphor_core::denormal::NoDenormals;
use phosphor_core::fx::{Effect, FxContext};
use phosphor_plugin::sample::{pad_index, PadConfig, PadLayer, SamplePcm};
use phosphor_plugin::MidiEvent;

#[path = "support/mod.rs"]
mod support;
use support::{dbfs, each};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 128;
/// Long enough for the phrase, its releases and most of their tails.
const SECONDS: f64 = 2.5;
/// When a sweep moves its control: mid-chord, with notes sounding.
const SWEEP_AT: f64 = 0.45;

fn frame(seconds: f64) -> usize {
    (seconds * RATE) as usize
}

/// The phrase every instrument plays, as `(seconds, status, data1, data2)`.
/// Off the block grid on purpose, so that every event lands mid-block.
const PHRASE: &[(f64, u8, u8, u8)] = &[
    (0.000, 0x90, 48, 100),
    (0.120, 0x90, 36, 60),
    (0.131, 0x90, 38, 90),
    (0.300, 0x90, 60, 30),
    (0.300, 0x90, 64, 64),
    (0.300, 0x90, 67, 100),
    (0.300, 0x90, 71, 127),
    (0.500, 0xB0, 1, 100),
    (0.610, 0xE0, 0x00, 0x50),
    (0.750, 0xD0, 80, 0),
    (0.900, 0x80, 48, 0),
    (0.900, 0x80, 36, 0),
    (0.900, 0x80, 38, 0),
    (0.903, 0x80, 60, 0),
    (0.905, 0x80, 64, 0),
    (0.907, 0x80, 67, 0),
    (0.909, 0x80, 71, 0),
    (0.910, 0xB0, 1, 0),
    (1.000, 0xE0, 0x00, 0x40),
    (1.000, 0xD0, 0, 0),
    (1.050, 0x90, 42, 80),
    (1.100, 0x80, 42, 0),
    (1.200, 0x90, 46, 110),
    (1.400, 0x80, 46, 0),
];

// ── What gets rendered ──

enum Source {
    Instrument { kind: InstrumentType, params: Vec<f32> },
    Effect { kind: FxType, params: Vec<f32> },
}

/// One rendering: a sound, and optionally one control moved mid-render.
struct Case {
    name: String,
    source: Source,
    sweep: Option<(usize, f32)>,
}

fn instruments() -> Vec<InstrumentType> {
    InstrumentType::ALL.iter().copied().filter(|i| !i.is_sequencer()).collect()
}

fn instrument_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for kind in instruments() {
        for sound in factory::sounds(kind) {
            cases.push(Case {
                name: sound.name(kind),
                source: Source::Instrument { kind, params: sound.params },
                sweep: None,
            });
        }
        // Every control on the default panel, turned halfway round its
        // travel while the chord sounds.
        let defaults = phosphor_app::preset::defaults(kind);
        for (index, &value) in defaults.iter().enumerate() {
            let to = (value + 0.5) % 1.0;
            cases.push(Case {
                name: format!("{} sweep {index:03}", kind.label()),
                source: Source::Instrument { kind, params: defaults.clone() },
                sweep: Some((index, to)),
            });
        }
    }
    cases
}

/// Whether an effect control picks a thing rather than sets a level: a short
/// run of whole numbers.
fn is_selector(info: &phosphor_core::fx::FxParamInfo) -> bool {
    let whole = |x: f32| x.fract() == 0.0;
    info.max - info.min <= 16.0 && whole(info.min) && whole(info.max) && whole(info.default)
}

fn effect_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for &kind in FxType::ALL {
        let effect = phosphor_app::fx::build(kind).expect("every menu effect builds");
        let defaults = phosphor_app::fx::params_of(effect.as_ref());
        let infos: Vec<_> = (0..effect.parameter_count()).filter_map(|i| effect.parameter_info(i)).collect();
        // Every selector position, one selector at a time, each with every
        // other control swept to both ends of its travel.
        let mut bases: Vec<(String, Vec<f32>)> = vec![("default".into(), defaults.clone())];
        for (index, info) in infos.iter().enumerate() {
            if !is_selector(info) {
                continue;
            }
            let mut at = info.min;
            while at <= info.max {
                if at != info.default {
                    let mut params = defaults.clone();
                    params[index] = at;
                    bases.push((format!("p{index:02}={at}"), params));
                }
                at += 1.0;
            }
        }
        for (base, params) in bases {
            cases.push(Case {
                name: format!("fx {} {base}", kind.label()),
                source: Source::Effect { kind, params: params.clone() },
                sweep: None,
            });
            for (index, info) in infos.iter().enumerate() {
                if is_selector(info) {
                    continue;
                }
                for (end, to) in [("min", info.min), ("max", info.max)] {
                    cases.push(Case {
                        name: format!("fx {} {base} sweep p{index:02} {end}", kind.label()),
                        source: Source::Effect { kind, params: params.clone() },
                        sweep: Some((index, to)),
                    });
                }
            }
        }
    }
    cases
}

// ── Rendering ──

/// A sample for the sampler's pads, recorded at a different rate from the
/// engine's so that its resampler is in the path: a decaying sawtooth.
fn sampler_pcm() -> Arc<SamplePcm> {
    let rate = 44_100.0f32;
    let data = (0..(rate as usize))
        .map(|n| {
            let t = n as f32 / rate;
            let saw = 2.0 * ((t * 220.0).fract()) - 1.0;
            saw * (-3.0 * t).exp() * 0.5
        })
        .collect();
    Arc::new(SamplePcm { data, channels: 1, sample_rate: rate })
}

fn render_instrument(kind: InstrumentType, params: &[f32], sweep: Option<(usize, f32)>) -> Vec<f32> {
    let mut plugin = factory::load(kind, params, RATE, BLOCK);
    if kind == InstrumentType::Sampler {
        let pcm = sampler_pcm();
        for &(_, status, note, _) in PHRASE {
            if status & 0xF0 == 0x90 {
                let pad = pad_index(note).expect("the phrase is on the pad bed");
                plugin.set_sampler_pad(pad as u8, &PadConfig::for_key(note), &[PadLayer::from_pcm(pcm.clone())]);
            }
        }
    }
    let total = frame(SECONDS);
    let mut out = Vec::with_capacity(total * 2);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut events = Vec::with_capacity(PHRASE.len());
    let mut start = 0;
    while start < total {
        let n = BLOCK.min(total - start);
        if let Some((index, value)) = sweep {
            if (start..start + n).contains(&frame(SWEEP_AT)) {
                plugin.set_parameter(index, value);
            }
        }
        events.clear();
        events.extend(PHRASE.iter().filter(|e| (start..start + n).contains(&frame(e.0))).map(|&(at, status, data1, data2)| {
            MidiEvent { sample_offset: (frame(at) - start) as u32, status, data1, data2 }
        }));
        let mut outs: [&mut [f32]; 2] = [&mut left[..n], &mut right[..n]];
        plugin.process(&[], &mut outs, &events);
        for i in 0..n {
            out.push(left[i]);
            out.push(right[i]);
        }
        start += n;
    }
    out
}

/// The test signal every effect is played: a second of decaying tones and
/// noise bursts that differ left to right, then silence for the tails.
fn test_signal(n: usize, state: &mut u32) -> (f32, f32) {
    let t = n as f64 / RATE;
    if t >= 1.0 {
        return (0.0, 0.0);
    }
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let noise = (*state >> 8) as f64 / f64::from(1u32 << 24) * 2.0 - 1.0;
    let burst = if (t * 4.0).fract() < 0.05 { noise * 0.6 } else { 0.0 };
    let decay = (-4.0 * (t * 2.0).fract()).exp();
    let l = (std::f64::consts::TAU * 220.0 * t).sin() * decay * 0.5 + burst;
    let r = (std::f64::consts::TAU * 331.0 * t).sin() * decay * 0.4 - burst * 0.5;
    (l as f32, r as f32)
}

fn render_effect(kind: FxType, params: &[f32], sweep: Option<(usize, f32)>) -> Vec<f32> {
    let mut effect: Box<dyn Effect> = phosphor_app::fx::build(kind).expect("every menu effect builds");
    effect.init(RATE, BLOCK);
    for (index, &value) in params.iter().enumerate() {
        effect.set_parameter(index, value);
    }
    let ctx = FxContext { tempo_bpm: 120.0, playing: true, ..FxContext::bare(RATE as f32) };
    let total = frame(SECONDS + 0.5);
    let mut out = Vec::with_capacity(total * 2);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut noise = 1u32;
    let mut start = 0;
    while start < total {
        let n = BLOCK.min(total - start);
        if let Some((index, value)) = sweep {
            if (start..start + n).contains(&frame(SWEEP_AT)) {
                effect.set_parameter(index, value);
            }
        }
        for i in 0..n {
            (left[i], right[i]) = test_signal(start + i, &mut noise);
        }
        effect.process(&mut left[..n], &mut right[..n], &ctx);
        for i in 0..n {
            out.push(left[i]);
            out.push(right[i]);
        }
        start += n;
    }
    out
}

fn render(case: &Case) -> Vec<f32> {
    match &case.source {
        Source::Instrument { kind, params } => render_instrument(*kind, params, case.sweep),
        Source::Effect { kind, params } => render_effect(*kind, params, case.sweep),
    }
}

// ── Summaries ──

/// FNV-1a over the exact bits of every sample. Spelled out rather than
/// borrowed from the standard library, whose hasher is free to change
/// between Rust releases.
fn fingerprint(samples: &[f32]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for s in samples {
        for byte in s.to_bits().to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Every case's fingerprint and peak. Rendered inside the engine's
/// flush-to-zero guard, because that is how the engine plays them, unless
/// `plain`.
fn table(cases: &[Case], plain: bool) -> Vec<(u64, f32)> {
    each(cases, |case| {
        let out = if plain {
            render(case)
        } else {
            let _guard = NoDenormals::new();
            render(case)
        };
        (fingerprint(&out), peak(&out))
    })
}

fn usage() -> ! {
    eprintln!("usage: fingerprint save <file> | check <file> | ftz   [--only <text>] [--plain]");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let plain = args.iter().any(|a| a == "--plain");
    let only = args.iter().position(|a| a == "--only").and_then(|i| args.get(i + 1)).cloned();
    let mut cases: Vec<Case> = instrument_cases().into_iter().chain(effect_cases()).collect();
    if let Some(text) = &only {
        cases.retain(|c| c.name.contains(text.as_str()));
    }
    let started = std::time::Instant::now();
    match args.first().map(String::as_str) {
        Some("save") => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let rows = table(&cases, plain);
            let mut text = String::from(
                "# phosphor sound fingerprint: hash of the output bits, peak dBFS, sound.\n\
                 # Valid only on the machine and build that wrote it.\n",
            );
            for (case, (hash, level)) in cases.iter().zip(&rows) {
                text.push_str(&format!("{hash:016x} {:>8.2} {}\n", dbfs(f64::from(*level)), case.name));
            }
            std::fs::write(path, text).expect("the table file is writable");
            let silent = rows.iter().filter(|(_, level)| *level == 0.0).count();
            println!("{} sounds fingerprinted into {path} in {:.1} s", rows.len(), started.elapsed().as_secs_f64());
            // A fingerprint of silence proves nothing about the sound, so
            // say how many there are rather than let them pass as checked.
            println!("{silent} of them are silent and so prove nothing");
        }
        Some("check") => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let saved = std::fs::read_to_string(path).expect("the table file is readable");
            let before: std::collections::HashMap<&str, &str> = saved
                .lines()
                .filter(|l| !l.starts_with('#'))
                .filter_map(|l| {
                    let (hash, rest) = l.split_once(' ')?;
                    let (_peak, name) = rest.trim_start().split_once(' ')?;
                    Some((name, hash))
                })
                .collect();
            let rows = table(&cases, plain);
            let mut changed = Vec::new();
            let mut unknown = 0;
            for (case, (hash, _)) in cases.iter().zip(&rows) {
                match before.get(case.name.as_str()) {
                    Some(old) if *old == format!("{hash:016x}") => {}
                    Some(_) => changed.push(case.name.as_str()),
                    None => unknown += 1,
                }
            }
            for name in &changed {
                println!("changed: {name}");
            }
            println!(
                "{} sounds checked in {:.1} s: {} changed, {} not in the saved table",
                rows.len(),
                started.elapsed().as_secs_f64(),
                changed.len(),
                unknown
            );
            if !changed.is_empty() {
                std::process::exit(1);
            }
        }
        Some("ftz") => {
            let rows = each(&cases, |case| {
                let plain = render(case);
                let flushed = {
                    let _guard = NoDenormals::new();
                    render(case)
                };
                let worst = plain
                    .iter()
                    .zip(&flushed)
                    .fold(0.0f64, |m, (a, b)| m.max((f64::from(*a) - f64::from(*b)).abs()));
                (worst, f64::from(peak(&plain)))
            });
            let mut changed: Vec<(f64, f64, &str)> = cases
                .iter()
                .zip(&rows)
                .filter(|(_, (worst, _))| *worst > 0.0)
                .map(|(case, (worst, level))| (*worst, *level, case.name.as_str()))
                .collect();
            changed.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (worst, level, name) in &changed {
                println!("{:>9.1} dBFS difference (peak {:>6.1} dBFS)  {name}", dbfs(*worst), dbfs(*level));
            }
            println!(
                "{} sounds rendered both ways in {:.1} s: {} identical, {} differ{}",
                rows.len(),
                started.elapsed().as_secs_f64(),
                rows.len() - changed.len(),
                changed.len(),
                changed.first().map_or(String::new(), |c| format!(", by at most {:.1} dBFS", dbfs(c.0))),
            );
            if !NoDenormals::available() {
                println!("(this platform has no flush-to-zero mode, so the guard does nothing here)");
            }
        }
        _ => usage(),
    }
}
