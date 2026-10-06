//! Will this computer keep up? The busy song through the real engine, timed.
//!
//! `phosphor --benchmark` runs this: the same eleven-instrument song the
//! engine's own tests render ([`crate::busy_song`]), with every effect, send
//! and the loop recorder going, block after block as fast as the machine can
//! make them. Each block is 128 frames at 48 kHz — 2.667 ms of sound — and
//! has to be made in less time than that, every time. An average that fits is
//! not enough: one block in a thousand running late is a click, so the worst
//! blocks are what decide the answer.
//!
//! It is run twice, on one core and then on every core the engine would use,
//! because the second is how Phosphor plays and the first says how much the
//! extra cores are carrying.

use std::time::Instant;

use crate::busy_song::{BusySong, FRAMES, RATE};

/// Blocks timed per run, after the warm-up: about eleven seconds of sound,
/// played in real time.
pub const BLOCKS: usize = 4_000;

/// Blocks played first and not timed, so caches and the worker threads are
/// warm and the first notes' set-up is behind us.
const WARMUP: usize = 200;

/// How long one block of sound lasts, in microseconds: the time there is to
/// make it.
#[must_use]
pub fn budget_us() -> f64 {
    FRAMES as f64 / f64::from(RATE) * 1e6
}

/// How long the blocks took, in microseconds.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub mean: f64,
    /// The one-in-a-thousand block: worse than all but the worst tenth of a
    /// percent.
    pub p999: f64,
    pub worst: f64,
}

/// Play the busy song on `threads` cores for `blocks` blocks and time each.
#[must_use]
pub fn time_busy_song(threads: usize, blocks: usize) -> Timing {
    // The audio callback runs at real-time priority when Phosphor plays,
    // and its helpers do too; timed at normal priority, the blocks would
    // measure the scheduler's patience rather than the engine.
    promote_once();
    let mut song = BusySong::new(|mixer| mixer.set_threads(threads));
    let mut times = Vec::with_capacity(blocks);
    // Paced like the sound card paces the engine: one block per block's
    // worth of time, resting in between. Real-time threads that never rest
    // are throttled by the system (macOS and Linux both police it), so a
    // run without the rests would time the throttling, not the engine.
    let period = std::time::Duration::from_secs_f64(budget_us() / 1e6);
    let mut due = Instant::now();
    for n in 0..WARMUP + blocks {
        let start = Instant::now();
        song.next_block();
        let us = start.elapsed().as_secs_f64() * 1e6;
        if n >= WARMUP {
            times.push(us);
        }
        due += period;
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        } else {
            // Late: start the next block at once, from now.
            due = now;
        }
    }
    times.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / times.len().max(1) as f64;
    let at = ((times.len() as f64 * 0.999) as usize).clamp(1, times.len()) - 1;
    Timing { mean, p999: times[at], worst: times[times.len() - 1] }
}

/// Ask for real-time scheduling for the calling thread, once per thread.
fn promote_once() {
    thread_local!(static PROMOTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    PROMOTED.with(|done| {
        if !done.replace(true) {
            phosphor_core::realtime::promote_current_thread(FRAMES as u32, RATE);
        }
    });
}

/// What the numbers mean for playing music, in a sentence.
#[must_use]
pub fn verdict(all_cores: &Timing) -> &'static str {
    let budget = budget_us();
    let worst = all_cores.p999.max(all_cores.worst * 0.8);
    if worst < budget * 0.5 {
        "Plenty of room: this computer plays a song heavier than most with half its time to spare."
    } else if worst < budget * 0.8 {
        "It keeps up, with some room. Very heavy songs, or a smaller audio buffer, may get close."
    } else if all_cores.mean < budget * 0.8 {
        "On average it keeps up, but its slowest moments come close to the deadline: expect an \
         occasional click on a song this busy. A bigger audio buffer (--buffer-size 256) gives it more time."
    } else {
        "This computer cannot make this song's sound in time. Lighter songs may play; a song \
         this busy will break up."
    }
}

/// Run the benchmark, saying each line as it is ready — the second run
/// takes a few seconds, and a screen that sits blank reads as hung.
pub fn run(mut say: impl FnMut(&str)) {
    let budget = budget_us();
    let cores = phosphor_core::parallel::audio_threads();
    say(&format!(
        "Each block is {FRAMES} frames at {} kHz: {:.0} µs of sound, to be made in less time than that.",
        RATE / 1000,
        budget
    ));
    say(&format!("Timing {BLOCKS} blocks of a busy eleven-instrument song, first on one core, then on {cores}."));
    say("");
    say(&format!("{:<16} {:>12} {:>16} {:>12}", "", "average", "1 in 1000", "worst"));
    let row = |label: &str, t: &Timing| {
        format!(
            "{label:<16} {:>6.0} µs {:>3.0}% {:>9.0} µs {:>3.0}% {:>6.0} µs {:>3.0}%",
            t.mean,
            t.mean / budget * 100.0,
            t.p999,
            t.p999 / budget * 100.0,
            t.worst,
            t.worst / budget * 100.0
        )
    };
    let one = time_busy_song(1, BLOCKS);
    say(&row("one core", &one));
    let all = if cores > 1 {
        let all = time_busy_song(cores, BLOCKS);
        say(&row(&format!("{cores} cores"), &all));
        all
    } else {
        one
    };
    say("");
    say("Percentages are of the time each block has. Under 100% keeps up.");
    say(verdict(&all));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_two_and_two_thirds_milliseconds() {
        assert!((budget_us() - 2666.67).abs() < 0.1);
    }

    #[test]
    fn the_verdict_follows_the_slowest_blocks() {
        let b = budget_us();
        let t = |mean: f64, p999: f64, worst: f64| Timing { mean: b * mean, p999: b * p999, worst: b * worst };
        assert!(verdict(&t(0.1, 0.2, 0.3)).starts_with("Plenty"));
        assert!(verdict(&t(0.3, 0.6, 0.7)).starts_with("It keeps up"));
        assert!(verdict(&t(0.5, 0.9, 1.2)).starts_with("On average"));
        assert!(verdict(&t(0.9, 1.1, 1.5)).starts_with("This computer cannot"));
    }

    /// A short run, to prove the timing works end to end; the numbers
    /// themselves depend on the machine.
    #[test]
    fn a_short_run_times_every_block() {
        let t = time_busy_song(2, 20);
        assert!(t.mean > 0.0 && t.mean <= t.p999 && t.p999 <= t.worst);
    }
}
