//! The busy song through the real audio engine, kept sample for sample.
//!
//! What the engine produces for [`BusySong`] — every sample, every meter,
//! every recorded take — is its whole behaviour in one value, so two ways of
//! running the engine that must sound the same can be held to producing
//! exactly the same thing.

use phosphor_app::busy_song::{BusySong, FRAMES, SCRIPT_BLOCKS};
use phosphor_core::mixer::Mixer;

/// Everything the engine produced.
#[derive(Debug, PartialEq)]
struct Rendered {
    /// Interleaved output, as bits so that equality is exact.
    output: Vec<u32>,
    /// Every track's and the master's meter after every block.
    meters: Vec<(u32, u32)>,
    /// Every take the recorder committed, in the order it sent them.
    takes: Vec<String>,
}

fn render(configure: impl FnOnce(&mut Mixer)) -> Rendered {
    let mut song = BusySong::new(configure);
    let mut rendered = Rendered { output: Vec::with_capacity(FRAMES * 2 * SCRIPT_BLOCKS), meters: Vec::new(), takes: Vec::new() };
    for _ in 0..SCRIPT_BLOCKS {
        let out = song.next_block();
        rendered.output.extend(out.iter().map(|s| s.to_bits()));
        rendered.meters.extend(song.meters());
    }
    rendered.takes = song.finish().iter().map(|take| format!("{take:?}")).collect();
    rendered
}

/// The song is a song: something sounded, takes were committed, and the
/// meters moved. A harness that rendered silence would make every
/// comparison built on it pass.
#[test]
fn the_song_sounds_and_records() {
    let r = render(|_| {});
    let loud = r.output.iter().filter(|&&b| f32::from_bits(b).abs() > 0.01).count();
    assert!(loud > r.output.len() / 4, "the song is mostly silence: {loud} of {}", r.output.len());
    assert!(r.takes.len() >= 3, "the loop recorder committed {} takes", r.takes.len());
    assert!(r.meters.iter().any(|&(l, _)| f32::from_bits(l) > 0.0));
}

/// The comparisons are only worth anything if the song is the same song
/// every time it is rendered.
#[test]
fn the_song_renders_the_same_every_time() {
    assert!(render(|_| {}) == render(|_| {}), "two renders of the same song differ");
}

/// The engine spreads tracks over however many cores the machine has. How
/// many must never change a single sample, meter or take.
#[test]
fn every_thread_count_renders_the_same_song() {
    let one = render(|mixer| mixer.set_threads(1));
    assert_eq!(one.output.len(), FRAMES * 2 * SCRIPT_BLOCKS);
    for threads in [2, 3, 4, 8] {
        let many = render(|mixer| mixer.set_threads(threads));
        assert!(many == one, "{threads} threads rendered a different song from one");
    }
}
