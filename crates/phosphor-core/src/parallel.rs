//! Spreading the audio thread's work over the machine's other cores.
//!
//! [`Workers`] is a small pool of threads that wait for the audio thread to
//! hand them a batch of independent jobs — one per track, say — and run them
//! alongside it. [`Workers::for_each`] returns only when every job in the
//! batch has finished, so whatever the jobs wrote is ready for the audio
//! thread to read the moment it returns.
//!
//! It is built for the callback, so nothing on its path allocates or takes a
//! lock:
//!
//! * The audio thread works too. It claims jobs from the same counter the
//!   workers do, so a worker that is slow to wake costs only the share of
//!   the batch it would have done, never a missed deadline by itself.
//! * A job is claimed with one compare-and-swap on a word that names the
//!   batch as well as the job, so a worker that wakes late can never take a
//!   job from a batch that has already finished and run it against the next
//!   batch's data.
//! * Between batches the workers spin for a moment — the next batch is
//!   usually a fraction of a millisecond away — and then sleep, so an idle
//!   Phosphor costs nothing.
//! * A job that panics is caught where it ran and raised again on the audio
//!   thread once the whole batch has finished, which is where it would have
//!   been raised before there were workers, and never while another job
//!   could still be reading the batch.
//!
//! With no workers — a single-core machine, or `PHOSPHOR_AUDIO_THREADS=1` —
//! `for_each` runs the jobs in order on the caller, which is exactly what
//! the code did before this module existed.

use std::any::Any;
use std::cell::Cell;
use std::marker::PhantomData;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The most threads the audio work is spread over, the audio thread
/// included. Past this a block's tracks are too few to share out.
const MAX_THREADS: usize = 8;

/// How many times a worker checks for the next batch before sleeping:
/// tens of microseconds, enough to stay awake between the batches of one
/// block.
const SPINS: u32 = 20_000;

/// How many times the caller checks for a batch to finish before it starts
/// yielding its core. Under real-time scheduling a thread of equal priority
/// is never pre-empted, so a worker that landed on the caller's own core
/// could otherwise wait behind the caller's spinning forever.
const WAIT_SPINS: u32 = 2_000;

/// How many threads to spread the audio over: every core, up to
/// [`MAX_THREADS`], or what `PHOSPHOR_AUDIO_THREADS` says. One means none
/// but the audio thread.
#[must_use]
pub fn audio_threads() -> usize {
    let asked = std::env::var("PHOSPHOR_AUDIO_THREADS").ok().and_then(|v| v.parse::<usize>().ok());
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    asked.unwrap_or(cores).clamp(1, MAX_THREADS)
}

// ── The ticket ──
//
// One word: the batch number in the high 32 bits, the batch's job count in
// the next 16, and the next unclaimed job in the low 16.

const COUNT_LIMIT: usize = u16::MAX as usize;

fn pack(round: u32, count: usize, next: usize) -> u64 {
    (u64::from(round) << 32) | ((count as u64) << 16) | next as u64
}

fn unpack(ticket: u64) -> (u32, usize, usize) {
    ((ticket >> 32) as u32, ((ticket >> 16) & 0xFFFF) as usize, (ticket & 0xFFFF) as usize)
}

/// A batch's job, as a thin pointer the workers can load atomically.
struct Job {
    data: *const (),
    call: unsafe fn(*const (), usize),
}

impl Job {
    fn new<F: Fn(usize) + Sync>(f: &F) -> Self {
        unsafe fn call<F: Fn(usize) + Sync>(data: *const (), index: usize) {
            (*data.cast::<F>())(index);
        }
        Self { data: (f as *const F).cast(), call: call::<F> }
    }
}

struct Shared {
    ticket: AtomicU64,
    job: AtomicPtr<Job>,
    done: AtomicUsize,
    stop: AtomicBool,
    /// Set when a job panicked, so the audio thread only touches the lock
    /// below on the way to re-raising it.
    panicked: AtomicBool,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
    #[cfg(test)]
    allocations: AtomicU64,
}

impl Shared {
    /// Claim and run jobs from batch `round` until none are left.
    fn work(&self, round: u32) {
        loop {
            let ticket = self.ticket.load(Ordering::Acquire);
            let (r, count, next) = unpack(ticket);
            if r != round || next >= count {
                return;
            }
            let claimed = pack(r, count, next + 1);
            if self
                .ticket
                .compare_exchange_weak(ticket, claimed, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            // The batch cannot finish while this job is unfinished, so the
            // job it was published with is still alive.
            let job = unsafe { &*self.job.load(Ordering::Acquire) };
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| unsafe { (job.call)(job.data, next) })) {
                if let Ok(mut slot) = self.panic.lock() {
                    slot.get_or_insert(payload);
                }
                self.panicked.store(true, Ordering::Release);
            }
            self.done.fetch_add(1, Ordering::Release);
        }
    }
}

/// A pool of threads the audio work is shared with. See the module notes.
pub struct Workers {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    round: Cell<u32>,
}

impl Workers {
    /// A pool spreading work over `threads` threads in all: the caller and
    /// `threads - 1` workers, started here. Never call this on the audio
    /// thread — starting threads is exactly what it must not do.
    ///
    /// With `realtime`, the audio's block size and sample rate, each worker
    /// asks to be scheduled as real-time audio when it starts. See
    /// [`crate::realtime`].
    #[must_use]
    pub fn new(threads: usize, realtime: Option<(u32, u32)>) -> Self {
        let shared = Arc::new(Shared {
            ticket: AtomicU64::new(0),
            job: AtomicPtr::new(std::ptr::null_mut()),
            done: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            panicked: AtomicBool::new(false),
            panic: Mutex::new(None),
            #[cfg(test)]
            allocations: AtomicU64::new(0),
        });
        let threads = (1..threads.max(1))
            .filter_map(|n| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("phosphor-audio-{n}"))
                    .spawn(move || {
                        if let Some((frames, rate)) = realtime {
                            crate::realtime::promote_current_thread(frames, rate);
                        }
                        worker(&shared);
                    })
                    .ok()
            })
            .collect();
        Self { shared, threads, round: Cell::new(0) }
    }

    /// How many threads the work is spread over, the caller included.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads.len() + 1
    }

    /// Run `job(0)` to `job(count - 1)`, spread over the pool, and return
    /// when every one has finished. The jobs run in no particular order and
    /// at the same time as each other, so each must touch only what is its
    /// own (see [`EachMut`]).
    pub fn for_each<F: Fn(usize) + Sync>(&self, count: usize, job: &F) {
        if self.threads.is_empty() || count < 2 {
            (0..count).for_each(job);
            return;
        }
        assert!(count <= COUNT_LIMIT, "a batch of {count} jobs");
        let shared = &*self.shared;
        let published = Job::new(job);
        let round = self.round.get().wrapping_add(1);
        self.round.set(round);
        shared.done.store(0, Ordering::Relaxed);
        shared.job.store((&published as *const Job).cast_mut(), Ordering::Release);
        shared.ticket.store(pack(round, count, 0), Ordering::Release);
        for thread in &self.threads {
            thread.thread().unpark();
        }
        shared.work(round);
        let mut spins = 0u32;
        while shared.done.load(Ordering::Acquire) < count {
            if spins < WAIT_SPINS {
                spins += 1;
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
        shared.job.store(std::ptr::null_mut(), Ordering::Release);
        if shared.panicked.swap(false, Ordering::Acquire) {
            if let Some(payload) = shared.panic.lock().ok().and_then(|mut slot| slot.take()) {
                resume_unwind(payload);
            }
        }
    }

    /// Allocations made on the workers' own threads, for the tests that hold
    /// the audio path to never calling the allocator.
    #[cfg(test)]
    pub(crate) fn worker_allocations(&self) -> u64 {
        self.shared.allocations.load(Ordering::Relaxed)
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        for thread in &self.threads {
            thread.thread().unpark();
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn worker(shared: &Shared) {
    #[cfg(test)]
    crate::alloc_count::count_into(&shared.allocations);
    let mut seen = 0u32;
    loop {
        let mut spins = 0u32;
        loop {
            if shared.stop.load(Ordering::Acquire) {
                return;
            }
            let (round, _, _) = unpack(shared.ticket.load(Ordering::Acquire));
            if round != seen {
                seen = round;
                break;
            }
            if spins < SPINS {
                spins += 1;
                std::hint::spin_loop();
            } else {
                std::thread::park();
            }
        }
        shared.work(seen);
    }
}

/// A slice lent out to a batch one element per job: job `i` gets element
/// `i`, mutably, and nothing else.
pub struct EachMut<'a, T> {
    ptr: *mut T,
    len: usize,
    _borrow: PhantomData<&'a mut [T]>,
}

// SAFETY: each element goes to one job (see `get`), and `T: Send` means it
// may be used from whichever thread runs that job.
unsafe impl<T: Send> Sync for EachMut<'_, T> {}

impl<'a, T> EachMut<'a, T> {
    pub fn new(slice: &'a mut [T]) -> Self {
        Self { ptr: slice.as_mut_ptr(), len: slice.len(), _borrow: PhantomData }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Element `index`.
    ///
    /// # Safety
    ///
    /// No two live borrows of the same index: in a [`Workers::for_each`]
    /// batch, call it only with the job's own index.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self, index: usize) -> &mut T {
        assert!(index < self.len);
        &mut *self.ptr.add(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn every_job_runs_exactly_once() {
        let workers = Workers::new(4, None);
        for count in [0usize, 1, 2, 3, 7, 64, 1000] {
            let hits: Vec<AtomicU32> = (0..count).map(|_| AtomicU32::new(0)).collect();
            workers.for_each(count, &|i| {
                hits[i].fetch_add(1, Ordering::Relaxed);
            });
            assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1), "batch of {count}");
        }
    }

    /// Thousands of batches back to back, the way blocks arrive: none
    /// overlaps the next, and every job's write is visible on return.
    #[test]
    fn batches_follow_each_other_cleanly() {
        let workers = Workers::new(4, None);
        let mut cells = vec![0u64; 16];
        for round in 1..=5_000u64 {
            let each = EachMut::new(&mut cells);
            workers.for_each(each.len(), &|i| {
                // SAFETY: the job's own index.
                let cell = unsafe { each.get(i) };
                *cell += round;
            });
            let expected: u64 = (1..=round).sum();
            assert!(cells.iter().all(|&c| c == expected), "round {round}");
        }
    }

    #[test]
    fn the_work_really_is_shared() {
        let workers = Workers::new(4, None);
        if workers.threads() < 2 {
            return; // a machine that could not start a worker
        }
        let seen = Mutex::new(std::collections::HashSet::new());
        for _ in 0..200 {
            workers.for_each(32, &|_| {
                std::thread::sleep(std::time::Duration::from_micros(50));
                seen.lock().unwrap().insert(std::thread::current().id());
            });
        }
        assert!(seen.lock().unwrap().len() > 1, "every job ran on one thread");
    }

    #[test]
    fn one_thread_runs_in_order_on_the_caller() {
        let workers = Workers::new(1, None);
        assert_eq!(workers.threads(), 1);
        let order = Mutex::new(Vec::new());
        workers.for_each(5, &|i| order.lock().unwrap().push((i, std::thread::current().id())));
        let here = std::thread::current().id();
        assert_eq!(*order.lock().unwrap(), (0..5).map(|i| (i, here)).collect::<Vec<_>>());
    }

    /// A job that panics on a worker panics the caller, once the batch is
    /// done — and the pool still works afterwards.
    #[test]
    fn a_panic_reaches_the_caller_and_the_pool_survives() {
        let workers = Workers::new(4, None);
        let finished = AtomicU32::new(0);
        let result = catch_unwind(AssertUnwindSafe(|| {
            workers.for_each(16, &|i| {
                if i == 9 {
                    panic!("job nine");
                }
                std::thread::sleep(std::time::Duration::from_micros(200));
                finished.fetch_add(1, Ordering::Relaxed);
            });
        }));
        assert!(result.is_err(), "the panic was swallowed");
        assert_eq!(finished.load(Ordering::Relaxed), 15, "the batch was abandoned mid-way");
        let ran = AtomicU32::new(0);
        workers.for_each(8, &|_| {
            ran.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(ran.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn dropping_the_pool_stops_its_threads() {
        for _ in 0..20 {
            let workers = Workers::new(4, None);
            workers.for_each(8, &|_| {});
            drop(workers);
        }
    }

    #[test]
    fn handing_out_a_batch_does_not_allocate() {
        let workers = Workers::new(4, None);
        let mut cells = vec![0u32; 12];
        // Warm up: the first batch may wake threads for the first time.
        let each = EachMut::new(&mut cells);
        workers.for_each(each.len(), &|i| unsafe { *each.get(i) += 1 });
        let before = workers.worker_allocations();
        let here = crate::alloc_count::allocations_during(|| {
            for _ in 0..100 {
                let each = EachMut::new(&mut cells);
                workers.for_each(each.len(), &|i| unsafe { *each.get(i) += 1 });
            }
        });
        assert_eq!(here, 0, "the caller allocated");
        assert_eq!(workers.worker_allocations() - before, 0, "a worker allocated");
    }
}
