use criterion::{Criterion, criterion_group, criterion_main};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use phosphor_core::engine::{EngineAudio, VuLevels};
use phosphor_core::transport::Transport;
use phosphor_core::EngineConfig;
use phosphor_dsp::synth::PhosphorSynth;

fn bench_engine_process_silence(c: &mut Criterion) {
    let config = EngineConfig {
        buffer_size: 64,
        sample_rate: 44100,
    };
    let transport = Arc::new(Transport::new(120.0));
    transport.play();

    let synth = Box::new(PhosphorSynth::new());
    let panic_flag = Arc::new(AtomicBool::new(false));
    let vu_levels = Arc::new(VuLevels::new());
    let mut engine_audio = EngineAudio::new(&config, synth, None, panic_flag, vu_levels);

    let mut buffer = vec![0.0f32; 128]; // 64 samples x 2 channels

    c.bench_function("engine_process_64_samples_stereo", |b| {
        b.iter(|| {
            engine_audio.process(&mut buffer, &transport);
        });
    });
}

fn bench_engine_process_large_buffer(c: &mut Criterion) {
    let config = EngineConfig {
        buffer_size: 512,
        sample_rate: 44100,
    };
    let transport = Arc::new(Transport::new(120.0));
    transport.play();

    let synth = Box::new(PhosphorSynth::new());
    let panic_flag = Arc::new(AtomicBool::new(false));
    let vu_levels = Arc::new(VuLevels::new());
    let mut engine_audio = EngineAudio::new(&config, synth, None, panic_flag, vu_levels);

    let mut buffer = vec![0.0f32; 1024]; // 512 samples x 2 channels

    c.bench_function("engine_process_512_samples_stereo", |b| {
        b.iter(|| {
            engine_audio.process(&mut buffer, &transport);
        });
    });
}

criterion_group!(benches, bench_engine_process_silence, bench_engine_process_large_buffer);
criterion_main!(benches);
