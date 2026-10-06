//! What the sound-checking examples share: running many independent renders
//! over every core, and reading levels in decibels.
//!
//! Included by path (`#[path = "support/mod.rs"] mod support;`) rather than
//! built as an example of its own.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Run `job` over every item on every core, returning the results in item
/// order. Each item is rendered whole on one thread, so anything timed inside
/// `job` is timed on one core.
pub fn each<I: Sync, T: Send>(items: &[I], job: impl Fn(&I) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<T>>> = items.iter().map(|_| Mutex::new(None)).collect();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(i) else { break };
                let result = job(item);
                *results[i].lock().expect("no job panics while holding it") = Some(result);
            });
        }
    });
    results.into_iter().map(|r| r.into_inner().expect("unpoisoned").expect("every item ran")).collect()
}

/// A level as dBFS, or minus infinity for silence.
#[allow(dead_code)] // not every example that includes this reads levels
pub fn dbfs(level: f64) -> f64 {
    if level > 0.0 { 20.0 * level.log10() } else { f64::NEG_INFINITY }
}
