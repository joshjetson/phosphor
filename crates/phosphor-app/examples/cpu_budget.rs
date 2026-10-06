//! How much of one CPU core Phosphor's sound takes, instrument by
//! instrument, and for a heavy session of all of them at once.
//!
//!     cargo run --release -p phosphor-app --example cpu_budget
//!
//! Every track is processed on the one audio thread, so what decides whether
//! a computer keeps up is how fast a single core is, not how many it has.
//! Each 128-frame block at 48 kHz (the default; `BLOCK=256` for a bigger
//! buffer) is 2.667 ms of sound and has to be made in
//! less than that, every time: an average that fits is not enough if one
//! block in a thousand runs late, because that block is a click.
//!
//! Each instrument holds an 8-note chord and strikes it again every 48
//! blocks (about every eighth of a second), so the moment notes start —
//! often the most expensive instant — is in the measurement, and the worst
//! block is reported beside the average. Run it on the machine you mean to
//! build on: the number that matters is the "heavy session" line's worst
//! block against the 2.667 ms budget.

use std::time::Instant;

use phosphor_app::instrument::build_plugin;
use phosphor_app::state::InstrumentType;
use phosphor_dsp::fx::compressor::{Compressor, PARAM_RATIO, PARAM_THRESHOLD_DB};
use phosphor_dsp::fx::delay::{Delay, Mode, PARAM_FEEDBACK, PARAM_HEADS, PARAM_MIX, PARAM_MODE};
use phosphor_dsp::fx::reverb::{Algorithm, Reverb, PARAM_ALGORITHM, PARAM_EARLY};
use phosphor_plugin::{MidiEvent, Plugin};

const RATE: f64 = 48_000.0;
/// Frames per block; `BLOCK=256 cargo run ...` measures a bigger buffer.
fn block_size() -> usize {
    std::env::var("BLOCK").ok().and_then(|v| v.parse().ok()).unwrap_or(128)
}
const BLOCKS: usize = 4_000;
const WARMUP: usize = 200;
const RESTRIKE: usize = 48;

fn budget_us() -> f64 {
    block_size() as f64 / RATE * 1e6
}

const CHORD: [u8; 8] = [36, 43, 48, 52, 55, 59, 62, 67];

fn strike(on: bool) -> Vec<MidiEvent> {
    CHORD
        .iter()
        .map(|&note| MidiEvent { sample_offset: 0, status: if on { 0x90 } else { 0x80 }, data1: note, data2: 100 })
        .collect()
}

/// One thing that makes sound or changes it, one block at a time.
trait Stage {
    fn block(&mut self, n: usize, left: &mut [f32], right: &mut [f32]);
}

struct Instrument(Box<dyn Plugin + Send>);

impl Stage for Instrument {
    fn block(&mut self, n: usize, left: &mut [f32], right: &mut [f32]) {
        let events = match n % RESTRIKE {
            0 => strike(true),
            r if r == RESTRIKE - 2 => strike(false),
            _ => Vec::new(),
        };
        let mut outs: [&mut [f32]; 2] = [left, right];
        self.0.process(&[], &mut outs, &events);
    }
}

struct Verb(Reverb);
impl Stage for Verb {
    fn block(&mut self, _: usize, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right);
    }
}

struct Echo(Delay);
impl Stage for Echo {
    fn block(&mut self, _: usize, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right, 120.0);
    }
}

struct Squash(Compressor);
impl Stage for Squash {
    fn block(&mut self, _: usize, left: &mut [f32], right: &mut [f32]) {
        self.0.process(left, right, None);
    }
}

fn instrument(kind: InstrumentType) -> Box<dyn Stage> {
    let mut plugin = build_plugin(kind);
    plugin.init(RATE, block_size());
    Box::new(Instrument(plugin))
}

fn reverb() -> Box<dyn Stage> {
    let mut r = Reverb::new(RATE);
    r.set_param_natural_immediate(PARAM_ALGORITHM, Algorithm::Hall.index() as f32);
    r.set_param_natural_immediate(PARAM_EARLY, Algorithm::Hall.suggested_early());
    r.snap();
    Box::new(Verb(r))
}

fn delay() -> Box<dyn Stage> {
    let mut d = Delay::new(RATE);
    d.set_param_natural(PARAM_MODE, Mode::Tape.index() as f32);
    d.set_param_natural(PARAM_HEADS, 3.0);
    d.set_param_natural(PARAM_FEEDBACK, 70.0);
    d.set_param_natural(PARAM_MIX, 100.0);
    d.snap();
    Box::new(Echo(d))
}

fn compressor() -> Box<dyn Stage> {
    let mut c = Compressor::new(RATE);
    c.set_param_natural(PARAM_THRESHOLD_DB, -24.0);
    c.set_param_natural(PARAM_RATIO, 75.0);
    c.snap();
    Box::new(Squash(c))
}

/// Run a chain of stages and time every block: (mean, 99.9th percentile,
/// worst) in microseconds.
fn measure(chains: &mut [Vec<Box<dyn Stage>>]) -> (f64, f64, f64) {
    let mut left = vec![0.0f32; block_size()];
    let mut right = vec![0.0f32; block_size()];
    let mut times = Vec::with_capacity(BLOCKS);
    for n in 0..WARMUP + BLOCKS {
        let start = Instant::now();
        for chain in chains.iter_mut() {
            left.fill(0.0);
            right.fill(0.0);
            for stage in chain.iter_mut() {
                stage.block(n, &mut left, &mut right);
            }
        }
        let us = start.elapsed().as_secs_f64() * 1e6;
        if n >= WARMUP {
            times.push(us);
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let p999 = times[(times.len() as f64 * 0.999) as usize - 1];
    (mean, p999, *times.last().unwrap())
}

fn row(name: &str, (mean, p999, worst): (f64, f64, f64)) {
    let b = budget_us();
    println!(
        "{name:<34} {mean:>8.1} µs {:>6.1}%   {p999:>8.1} µs   {worst:>8.1} µs {:>6.1}%",
        mean / b * 100.0,
        worst / b * 100.0
    );
}

fn main() {
    let instruments: Vec<InstrumentType> =
        InstrumentType::ALL.iter().copied().filter(|i| !i.is_sequencer()).collect();
    println!("one block = {} frames at {RATE} Hz = {:.0} µs of sound\n", block_size(), budget_us());
    println!("{:<34} {:>11} {:>7}   {:>11}   {:>11} {:>7}", "", "average", "", "1 in 1000", "worst", "");
    for &kind in &instruments {
        row(&format!("{} (8-note chord)", kind.label()), measure(&mut [vec![instrument(kind)]]));
    }
    row("reverb (hall)", measure(&mut [vec![reverb()]]));
    row("delay (tape, three heads)", measure(&mut [vec![delay()]]));
    row("compressor", measure(&mut [vec![compressor()]]));

    // Every instrument at once, each through a compressor and a reverb, and
    // half of them through a tape delay too: more than most songs ask for.
    let mut session: Vec<Vec<Box<dyn Stage>>> = instruments
        .iter()
        .enumerate()
        .map(|(i, &kind)| {
            let mut chain = vec![instrument(kind), compressor(), reverb()];
            if i % 2 == 0 {
                chain.push(delay());
            }
            chain
        })
        .collect();
    // A typical song: six instruments with a compressor each, and the
    // reverbs and delays shared on the two send buses rather than one per
    // track. (The sampler is measured with no sound loaded, so it reads
    // light; a chop playing back costs a little more.)
    use InstrumentType as I;
    let mut typical: Vec<Vec<Box<dyn Stage>>> = [I::DX7, I::Prophet6, I::Jupiter8, I::Juno60, I::DrumRack, I::Synth]
        .iter()
        .map(|&kind| vec![instrument(kind), compressor()])
        .collect();
    typical.push(vec![reverb(), delay()]);
    typical.push(vec![reverb(), delay()]);
    println!();
    row("typical song (6 tracks, 2 sends)", measure(&mut typical));
    row(&format!("heavy session ({} tracks)", session.len()), measure(&mut session));

    // The whole engine — mixer, recording, sends, meters, master and all —
    // playing the busy song, on one thread and then on every core.
    println!();
    let cores = phosphor_core::parallel::audio_threads();
    for threads in [1, cores] {
        row(&format!("busy song, whole engine, {threads} thread{}", if threads == 1 { "" } else { "s" }), measure_song(threads));
    }
}

/// [`measure`], for the busy song through the real mixer — the timing
/// `phosphor --benchmark` reports. It plays at 128-frame blocks whatever
/// `BLOCK` says.
fn measure_song(threads: usize) -> (f64, f64, f64) {
    let t = phosphor_app::benchmark::time_busy_song(threads, BLOCKS);
    (t.mean, t.p999, t.worst)
}
