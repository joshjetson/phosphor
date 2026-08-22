use criterion::{Criterion, criterion_group, criterion_main};
use phosphor_dsp::oscillator::{Oscillator, Waveform};
use phosphor_dsp::{jupiter, prophet6};
use phosphor_plugin::{MidiEvent, Plugin};

fn bench_sine_64(c: &mut Criterion) {
    let mut osc = Oscillator::new(Waveform::Sine, 440.0, 44100.0);
    let mut buf = [0.0f32; 64];
    c.bench_function("sine_osc_64_samples", |b| {
        b.iter(|| osc.process(&mut buf));
    });
}

fn bench_saw_64(c: &mut Criterion) {
    let mut osc = Oscillator::new(Waveform::Saw, 440.0, 44100.0);
    let mut buf = [0.0f32; 64];
    c.bench_function("saw_osc_64_samples", |b| {
        b.iter(|| osc.process(&mut buf));
    });
}

fn bench_square_64(c: &mut Criterion) {
    let mut osc = Oscillator::new(Waveform::Square, 440.0, 44100.0);
    let mut buf = [0.0f32; 64];
    c.bench_function("square_osc_64_samples", |b| {
        b.iter(|| osc.process(&mut buf));
    });
}

fn bench_triangle_64(c: &mut Criterion) {
    let mut osc = Oscillator::new(Waveform::Triangle, 440.0, 44100.0);
    let mut buf = [0.0f32; 64];
    c.bench_function("triangle_osc_64_samples", |b| {
        b.iter(|| osc.process(&mut buf));
    });
}

fn bench_sine_512(c: &mut Criterion) {
    let mut osc = Oscillator::new(Waveform::Sine, 440.0, 44100.0);
    let mut buf = [0.0f32; 512];
    c.bench_function("sine_osc_512_samples", |b| {
        b.iter(|| osc.process(&mut buf));
    });
}

/// A six-note chord held through a full buffer, on the two analog polys.
///
/// The Prophet-6 is the heaviest voice in the rack — two morphing
/// oscillators, a sub, noise, two filters, two envelopes and two poly-mod
/// paths per voice, and a stereo effects chain after them — so the number
/// worth knowing is how it compares with the Jupiter, which is the same
/// shape of instrument with a simpler voice. Run with
/// `cargo bench -p phosphor-dsp -- polysynth`.
fn chord() -> Vec<MidiEvent> {
    [48u8, 52, 55, 59, 62, 67]
        .iter()
        .map(|&note| MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: 100 })
        .collect()
}

fn bench_prophet6_chord(c: &mut Criterion) {
    let mut synth = prophet6::Prophet6::new();
    synth.init(44_100.0, 512);
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    let events = chord();
    let mut first = true;
    c.bench_function("polysynth_prophet6_512_samples", |b| {
        b.iter(|| {
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            synth.process(&[], &mut outs, if first { &events } else { &[] });
            first = false;
        });
    });
}

fn bench_jupiter_chord(c: &mut Criterion) {
    let mut synth = jupiter::Jupiter8Synth::new();
    synth.init(44_100.0, 512);
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    let events = chord();
    let mut first = true;
    c.bench_function("polysynth_jupiter8_512_samples", |b| {
        b.iter(|| {
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            synth.process(&[], &mut outs, if first { &events } else { &[] });
            first = false;
        });
    });
}

criterion_group!(
    benches,
    bench_prophet6_chord,
    bench_jupiter_chord,
    bench_sine_64,
    bench_saw_64,
    bench_square_64,
    bench_triangle_64,
    bench_sine_512,
);
criterion_main!(benches);
