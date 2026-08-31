use criterion::{Criterion, criterion_group, criterion_main};
use phosphor_dsp::oscillator::{Oscillator, Waveform};
use phosphor_dsp::fx::delay::{
    Delay, Mode, PARAM_FEEDBACK, PARAM_HEADS, PARAM_MODE, PARAM_MIX,
};
use phosphor_dsp::fx::reverb::{Algorithm, Reverb, PARAM_ALGORITHM, PARAM_EARLY};
use phosphor_dsp::fx::tape::{
    Speed, Tape, PARAM_AZIMUTH_DEG, PARAM_FLUTTER, PARAM_SPEED, PARAM_WOW,
};
use phosphor_dsp::fx::compressor::{
    Compressor, Sense, PARAM_RATIO, PARAM_SC_HPF_HZ, PARAM_SENSE, PARAM_THRESHOLD_DB,
};
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

/// One delay, one 512-frame block, stereo, at 48 kHz.
///
/// The brief's budgets are 0.31% of a core for the digital mode, 0.44% for the
/// bucket brigade and 0.48% for the tape — so 512 frames, which is 10.667 ms
/// of audio, must land in 33, 47 and 51 µs respectively.
fn bench_delay(c: &mut Criterion, mode: Mode, heads: f32, name: &str) {
    let mut delay = Delay::new(48_000.0);
    delay.set_param_natural(PARAM_MODE, mode.index() as f32);
    delay.set_param_natural(PARAM_HEADS, heads);
    delay.set_param_natural(PARAM_FEEDBACK, 70.0);
    delay.set_param_natural(PARAM_MIX, 100.0);
    delay.snap();
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    // A running tail rather than silence: the cost is the same either way, but
    // a benchmark on zeros is a benchmark an optimiser can cheat.
    for (index, sample) in left.iter_mut().enumerate() {
        *sample = ((index as f32) * 0.07).sin() * 0.25;
    }
    right.copy_from_slice(&left);
    for _ in 0..64 {
        delay.process(&mut left, &mut right, 120.0);
    }
    c.bench_function(name, |b| {
        b.iter(|| delay.process(&mut left, &mut right, 120.0));
    });
}

fn bench_delay_digital(c: &mut Criterion) {
    bench_delay(c, Mode::Digital, 0.0, "delay_digital_512_samples");
}

fn bench_delay_bbd(c: &mut Criterion) {
    bench_delay(c, Mode::Bbd, 0.0, "delay_bbd_512_samples");
}

fn bench_delay_tape(c: &mut Criterion) {
    bench_delay(c, Mode::Tape, 0.0, "delay_tape_512_samples");
}

fn bench_delay_tape_three_heads(c: &mut Criterion) {
    bench_delay(c, Mode::Tape, 6.0, "delay_tape_three_heads_512_samples");
}

/// One compressor, one 512-frame block, stereo, at 48 kHz.
///
/// **The budget is four times the master limiter.** The limiter is two
/// finiteness checks, two `abs`, a `max`, a compare, a conditional divide, a
/// multiply-add and two clamps per frame, and no transcendentals at all; the
/// compressor is all of that plus one `ln` and one `exp`, which are most of
/// what it costs. Four times a stage that measures under a microsecond a block
/// is still nothing next to the 10.667 ms a 512-frame block represents, so the
/// number to watch is not the absolute one — it is what happens to it the day
/// somebody adds a second transcendental to the inner loop.
///
/// The variants are the three shapes the inner loop takes: peak with the
/// detector filter out, peak with it in, and the mean-square front end.
///
/// Measured, release, Apple silicon, per 512-frame stereo block:
///
/// | variant | time | share of one core |
/// |---|---|---|
/// | peak, filter out | 5.02 µs | 0.047% |
/// | peak, filter in | 7.49 µs | 0.070% |
/// | rms | 7.77 µs | 0.073% |
///
/// Six slots on each of thirty-two tracks would be 192 instances, which at the
/// cheapest variant is 0.96 ms of a 10.667 ms block — under a tenth of a core.
/// If that ever stops being comfortable, an `ln`/`exp` approximation is the
/// first optimisation and it is contained to two lines.
fn bench_compressor(c: &mut Criterion, sense: Sense, hpf: f32, name: &str) {
    let mut comp = Compressor::new(48_000.0);
    comp.set_param_natural(PARAM_THRESHOLD_DB, -24.0);
    comp.set_param_natural(PARAM_RATIO, 75.0);
    comp.set_param_natural(PARAM_SENSE, sense.index() as f32);
    comp.set_param_natural(PARAM_SC_HPF_HZ, hpf);
    comp.snap();
    let mut left = vec![0.0f32; 512];
    let mut right = vec![0.0f32; 512];
    // Loud enough to be *working*, because a compressor sitting under its
    // threshold takes the cheap branch of the gain computer and would flatter
    // the measurement.
    for (index, sample) in left.iter_mut().enumerate() {
        *sample = ((index as f32) * 0.07).sin() * 0.5;
    }
    right.copy_from_slice(&left);
    for _ in 0..64 {
        comp.process(&mut left, &mut right, None);
    }
    c.bench_function(name, |b| {
        b.iter(|| comp.process(&mut left, &mut right, None));
    });
}

fn bench_compressor_peak(c: &mut Criterion) {
    bench_compressor(c, Sense::Peak, 0.0, "compressor_peak_512_samples");
}

fn bench_compressor_peak_hpf(c: &mut Criterion) {
    bench_compressor(c, Sense::Peak, 80.0, "compressor_peak_hpf_512_samples");
}

fn bench_compressor_rms(c: &mut Criterion) {
    bench_compressor(c, Sense::Rms, 0.0, "compressor_rms_512_samples");
}


/// One tape machine, one 512-frame block, stereo, at 48 kHz.
///
/// **The brief's budget is 1.78% of one core**, measured on the hysteresis and
/// the oversampling filters alone; the shipped device adds the record and
/// reproduce EQ pairs, the wobbling line, the head and the smoothers and
/// measures 2.25%. So 512 frames, which is 10.667 ms of audio, must land
/// inside 267 µs for the ceiling the brief's own verification asks for, and
/// it lands at 240.
///
/// The cost is the differential equation and not the filters: splitting the
/// halfband's dot products across four accumulators — which turns a
/// latency-bound chain of multiply-adds into a throughput-bound one — was
/// worth 5%, and the libm `tanh` inside the Langevin function is worth 13%.
/// A rational `tanh` is therefore the first optimisation if one is ever
/// needed, and the reason it is not already here is that the exact one
/// reproduces the reference implementation's harmonic table to four decimal
/// places.
///
/// The variants are the two shapes the inner loop takes: the transport
/// running, which is the factory setting and costs a delay line and a cubic
/// read per sample per channel, and the transport stopped, which bypasses it.
fn bench_tape(c: &mut Criterion, moving: bool, azimuth: f32, name: &str) {
    let mut tape = Tape::new(48_000.0);
    tape.set_param_natural(PARAM_SPEED, Speed::Studio.index() as f32);
    tape.set_param_natural(PARAM_AZIMUTH_DEG, azimuth);
    if !moving {
        tape.set_param_natural(PARAM_WOW, 0.0);
        tape.set_param_natural(PARAM_FLUTTER, 0.0);
    }
    tape.snap();
    let source: Vec<f32> =
        (0..512).map(|index| ((index as f32) * 0.07).sin() * 0.25).collect();
    let mut left = source.clone();
    let mut right = source.clone();
    for _ in 0..64 {
        tape.process(&mut left, &mut right);
    }
    // **The source is written back every iteration**, and it is not
    // optional. An effect that is benchmarked on its own output is
    // benchmarked on whatever that output has become after ten thousand
    // passes, and this one is lined up a decibel below unity: after a few
    // hundred passes the buffer is silence, the medium takes the Langevin
    // series' cheap branch instead of its `tanh`, and the number that comes
    // out is half the truth.
    c.bench_function(name, |b| {
        b.iter(|| {
            left.copy_from_slice(&source);
            right.copy_from_slice(&source);
            tape.process(&mut left, &mut right);
        });
    });
}

fn bench_tape_running(c: &mut Criterion) {
    bench_tape(c, true, 0.0, "tape_512_samples");
}

fn bench_tape_stopped(c: &mut Criterion) {
    bench_tape(c, false, 0.0, "tape_still_transport_512_samples");
}

fn bench_tape_azimuth(c: &mut Criterion) {
    bench_tape(c, true, 0.5, "tape_azimuth_512_samples");
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
    bench_delay_digital,
    bench_delay_bbd,
    bench_delay_tape,
    bench_delay_tape_three_heads,
    bench_tape_running,
    bench_tape_stopped,
    bench_tape_azimuth,
    bench_reverb_plate,
    bench_reverb_room,
    bench_reverb_hall,
    bench_reverb_spring,
    bench_compressor_peak,
    bench_compressor_peak_hpf,
    bench_compressor_rms,
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
