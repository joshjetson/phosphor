use criterion::{Criterion, criterion_group, criterion_main};
use phosphor_dsp::oscillator::{Oscillator, Waveform};

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

criterion_group!(
    benches,
    bench_sine_64,
    bench_saw_64,
    bench_square_64,
    bench_triangle_64,
    bench_sine_512,
);
criterion_main!(benches);
