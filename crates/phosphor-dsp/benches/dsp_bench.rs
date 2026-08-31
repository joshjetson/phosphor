use criterion::{Criterion, criterion_group, criterion_main};
use phosphor_dsp::oscillator::{Oscillator, Waveform};
use phosphor_dsp::fx::reverb::{Algorithm, Reverb, PARAM_ALGORITHM, PARAM_EARLY};
use phosphor_dsp::{jupiter, prophet6, teo5};
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


fn bench_teo5_chord(c: &mut Criterion) {
    let mut synth = teo5::Teo5::new();
    synth.init(44_100.0, 512);
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    // Five notes, which is the whole instrument: the TEO-5 has five voices
    // where the Prophet-6 has six, so the two benches are compared per voice.
    let events: Vec<MidiEvent> = [48u8, 52, 55, 59, 62]
        .iter()
        .map(|&note| MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: 100 })
        .collect();
    let mut first = true;
    c.bench_function("polysynth_teo5_512_samples", |b| {
        b.iter(|| {
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            synth.process(&[], &mut outs, if first { &events } else { &[] });
            first = false;
        });
    });
}

/// One reverb, one 512-frame block, stereo, at 48 kHz.
///
/// The budget is 0.5% of one core per instance, and the number to compare is
/// `time / (512 / 48000 s)`: 512 frames is 10.667 ms of audio, so 53 µs a
/// block is 0.5%.
fn bench_reverb(c: &mut Criterion, algorithm: Algorithm, name: &str) {
    let mut verb = Reverb::new(48_000.0);
    verb.set_param_natural_immediate(PARAM_ALGORITHM, algorithm.index() as f32);
    verb.set_param_natural_immediate(PARAM_EARLY, algorithm.suggested_early());
    verb.snap();
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    // A running tail rather than silence: a reverb's cost is the same either
    // way, but a benchmark on zeros is a benchmark an optimiser can cheat.
    for (index, sample) in left.iter_mut().enumerate() {
        *sample = ((index as f32) * 0.07).sin() * 0.25;
    }
    right.copy_from_slice(&left);
    c.bench_function(name, |b| {
        b.iter(|| verb.process(&mut left, &mut right));
    });
}

fn bench_reverb_plate(c: &mut Criterion) {
    bench_reverb(c, Algorithm::Plate, "reverb_plate_512_samples");
}

fn bench_reverb_room(c: &mut Criterion) {
    bench_reverb(c, Algorithm::Room, "reverb_room_512_samples");
}

fn bench_reverb_hall(c: &mut Criterion) {
    bench_reverb(c, Algorithm::Hall, "reverb_hall_512_samples");
}

fn bench_reverb_spring(c: &mut Criterion) {
    bench_reverb(c, Algorithm::Spring, "reverb_spring_512_samples");
}

criterion_group!(
    benches,
    bench_reverb_plate,
    bench_reverb_room,
    bench_reverb_hall,
    bench_reverb_spring,
    bench_prophet6_chord,
    bench_teo5_chord,
    bench_jupiter_chord,
    bench_sine_64,
    bench_saw_64,
    bench_square_64,
    bench_triangle_64,
    bench_sine_512,
);
criterion_main!(benches);
