//! What a dying sound costs: every factory program of an instrument plays a
//! chord, lets go, and fades for many seconds while every block is timed.
//!
//!     cargo run --release -p phosphor-app --example tail_cost
//!     cargo run --release -p phosphor-app --example tail_cost -- --only TEO --seconds 30
//!
//! A tail that decays towards zero eventually reaches numbers too small for
//! the floating-point format's exponent — *subnormal* numbers — and many CPUs
//! do arithmetic on those far more slowly than on ordinary ones. The symptom
//! is an instrument that costs more as its sound dies away than while it
//! plays: a delay's feedback fading out, a filter ringing down to nothing.
//!
//! Each program is rendered twice on the same thread, back to back: once as
//! plain code and once inside the engine's flush-to-zero guard
//! ([`phosphor_core::denormal::NoDenormals`]), which is how the engine plays.
//! The report compares the two, window by window through the tail, and
//! counts the subnormal samples each one put out. Where the CPU handles
//! subnormals at full speed the two columns come out the same; that is a
//! measurement too.
//!
//! The Prophet-6 and the TEO-5 are measured by default, since their delays
//! and filters are the long tails in the rack; `--only <text>` picks any
//! instruments whose name contains the text, `--seconds` sets the tail.

use std::time::Instant;

use phosphor_app::factory;
use phosphor_app::state::InstrumentType;
use phosphor_core::denormal::NoDenormals;
use phosphor_plugin::MidiEvent;

#[path = "support/mod.rs"]
mod support;
use support::each;

const RATE: f64 = 48_000.0;
const BLOCK: usize = 128;
const HELD_SECONDS: f64 = 0.5;
const CHORD: [u8; 4] = [48, 55, 60, 64];
/// Where the tail is cut into windows, in seconds after the release.
const WINDOWS: [f64; 5] = [0.0, 1.0, 5.0, 10.0, f64::INFINITY];

/// One render of one program.
#[derive(Default, Clone)]
struct Run {
    held_us: f64,
    held_blocks: usize,
    /// Total microseconds and blocks per tail window.
    tail_us: [f64; 4],
    tail_blocks: [usize; 4],
    worst_tail_us: f64,
    subnormals: usize,
}

impl Run {
    fn held(&self) -> f64 {
        self.held_us / self.held_blocks.max(1) as f64
    }
    fn window(&self, w: usize) -> f64 {
        self.tail_us[w] / self.tail_blocks[w].max(1) as f64
    }
    /// The last window that has any blocks in it: the late tail.
    fn late(&self) -> f64 {
        (0..4).rev().find(|&w| self.tail_blocks[w] > 0).map_or(0.0, |w| self.window(w))
    }
}

fn render(kind: InstrumentType, params: &[f32], tail_seconds: f64) -> Run {
    let mut plugin = factory::load(kind, params, RATE, BLOCK);
    let held = (HELD_SECONDS * RATE) as usize / BLOCK;
    let total = held + (tail_seconds * RATE) as usize / BLOCK;
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut run = Run::default();
    for block in 0..total {
        let events: Vec<MidiEvent> = if block == 0 || block == held {
            let status = if block == 0 { 0x90 } else { 0x80 };
            CHORD.iter().map(|&note| MidiEvent { sample_offset: 0, status, data1: note, data2: 100 }).collect()
        } else {
            Vec::new()
        };
        let start = Instant::now();
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        plugin.process(&[], &mut outs, &events);
        let us = start.elapsed().as_secs_f64() * 1e6;
        run.subnormals += left.iter().chain(&right).filter(|s| s.is_subnormal()).count();
        if block < held {
            run.held_us += us;
            run.held_blocks += 1;
        } else {
            let after = (block - held) as f64 * BLOCK as f64 / RATE;
            let w = WINDOWS.windows(2).position(|span| after >= span[0] && after < span[1]).unwrap_or(3);
            run.tail_us[w] += us;
            run.tail_blocks[w] += 1;
            run.worst_tail_us = run.worst_tail_us.max(us);
        }
    }
    run
}

/// Every program's runs, summed into one row of averages.
fn summary(label: &str, runs: &[&Run]) {
    let blocks = |f: &dyn Fn(&Run) -> (f64, usize)| {
        let (us, n) = runs.iter().fold((0.0, 0), |(u, b), r| {
            let (ru, rb) = f(r);
            (u + ru, b + rb)
        });
        us / n.max(1) as f64
    };
    let held = blocks(&|r| (r.held_us, r.held_blocks));
    let windows: Vec<f64> = (0..4).map(|w| blocks(&|r| (r.tail_us[w], r.tail_blocks[w]))).collect();
    let worst = runs.iter().map(|r| r.worst_tail_us).fold(0.0, f64::max);
    let subnormals: usize = runs.iter().map(|r| r.subnormals).sum();
    let programs = runs.iter().filter(|r| r.subnormals > 0).count();
    println!(
        "  {label:<8} held {held:>6.2}   tail 0-1 s {:>6.2}   1-5 s {:>6.2}   5-10 s {:>6.2}   10 s+ {:>6.2}   worst {worst:>7.1}   subnormal samples out: {subnormals} (from {programs} programs)",
        windows[0], windows[1], windows[2], windows[3],
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let seconds: f64 = after("--seconds").and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let kinds: Vec<InstrumentType> = match after("--only") {
        Some(text) => InstrumentType::ALL
            .iter()
            .copied()
            .filter(|k| !k.is_sequencer() && k.label().contains(text.as_str()))
            .collect(),
        None => vec![InstrumentType::Prophet6, InstrumentType::Teo5],
    };
    println!(
        "µs a block of {BLOCK} frames at {RATE} Hz, averaged over every program: a {}-note chord held {HELD_SECONDS} s, then {seconds} s of tail",
        CHORD.len()
    );
    if !NoDenormals::available() {
        println!("(this platform has no flush-to-zero mode, so the two runs are the same code)");
    }
    for kind in kinds {
        let sounds = factory::sounds(kind);
        let started = Instant::now();
        let runs = each(&sounds, |sound| {
            let plain = render(kind, &sound.params, seconds);
            let flushed = {
                let _guard = NoDenormals::new();
                render(kind, &sound.params, seconds)
            };
            (plain, flushed)
        });
        println!("\n{} — {} programs, {:.0} s to measure", kind.label(), sounds.len(), started.elapsed().as_secs_f64());
        summary("plain", &runs.iter().map(|(p, _)| p).collect::<Vec<_>>());
        summary("flushed", &runs.iter().map(|(_, f)| f).collect::<Vec<_>>());

        // The programs whose tails really did go subnormal as plain code,
        // side by side: where a subnormal slowdown would show, if this CPU
        // has one. Single programs' late tails are a few microseconds a
        // block, so a lone ratio is mostly timer noise; the average is the
        // measurement.
        let subnormal: Vec<&(Run, Run)> = runs.iter().filter(|(p, _)| p.subnormals > 0).collect();
        let late = |pick: &dyn Fn(&(Run, Run)) -> f64| {
            subnormal.iter().map(|r| pick(r)).sum::<f64>() / subnormal.len().max(1) as f64
        };
        println!(
            "  the {} programs whose tails put out subnormals: late tail {:.2} µs a block plain, {:.2} flushed",
            subnormal.len(),
            late(&|(p, _)| p.late()),
            late(&|(_, f)| f.late()),
        );
        let mut slowest: Vec<(f64, usize)> = runs
            .iter()
            .enumerate()
            .filter(|(_, (p, _))| p.subnormals > 0)
            .map(|(i, (p, f))| (p.late() - f.late(), i))
            .collect();
        slowest.sort_by(|a, b| b.0.total_cmp(&a.0));
        for &(_, i) in slowest.iter().take(5) {
            let (p, f) = &runs[i];
            println!(
                "    {:<22} late tail {:>6.2} µs plain, {:>6.2} flushed (held {:.2}); {} subnormal samples out",
                sounds[i].name(kind),
                p.late(),
                f.late(),
                p.held(),
                p.subnormals
            );
        }
    }
}
