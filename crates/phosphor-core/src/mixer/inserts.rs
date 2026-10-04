//! Pass two's first half: every strip's inserts, each strip on its own.
//!
//! A track's inserts read two things and write one. They read the track's
//! own pass-one output and, when its chain is keyed, another track's; they
//! write the track's own work buffers and nothing else. So the strips can be
//! run in any order, on any thread, and every one comes out the same — which
//! is what lets [`super::Mixer::process`] hand them to however many cores the
//! machine has, and still sum them into the mix in track order.
//!
//! [`Strips`] is how the tracks are shared out to that work a field at a
//! time: a job gets its own track's chain and work buffers to write, and
//! every track's pass-one buffers to read. That split is the whole of the
//! safety argument, and it lives here and nowhere else.

use std::marker::PhantomData;
use std::ptr::{addr_of, addr_of_mut};

use super::{AudioTrack, BusStrip};
use crate::fx::FxContext;

/// The tracks, lent out to the insert jobs a field at a time.
pub(super) struct Strips<'a> {
    tracks: *mut AudioTrack,
    len: usize,
    frames: usize,
    key_listen: Option<usize>,
    _borrow: PhantomData<&'a mut [AudioTrack]>,
}

// SAFETY: a `Strips` hands out, for each track, a mutable borrow of fields
// pass one does not touch afterwards (`chain`, `work_l`, `work_r`,
// `fx_scratch`) to exactly one job, and shared borrows of fields nothing
// writes while it is alive (`buf_l`, `buf_r`, `id`, `key_from`) to any job.
// Those sets are disjoint, so no two threads ever reach the same memory with
// one of them writing. `AudioTrack` is `Send`, so its fields may be used from
// another thread.
unsafe impl Sync for Strips<'_> {}

impl<'a> Strips<'a> {
    pub(super) fn new(tracks: &'a mut [AudioTrack], frames: usize, key_listen: Option<usize>) -> Self {
        Self { tracks: tracks.as_mut_ptr(), len: tracks.len(), frames, key_listen, _borrow: PhantomData }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// Track `i`'s pass-one output: the sidechain tap.
    fn tap(&self, i: usize) -> (&[f32], &[f32]) {
        assert!(i < self.len);
        // SAFETY: in bounds; `buf_l`/`buf_r` are only read while the strips
        // are out (see the `Sync` impl).
        unsafe {
            let track = self.tracks.add(i);
            (&(*addr_of!((*track).buf_l))[..self.frames], &(*addr_of!((*track).buf_r))[..self.frames])
        }
    }

    /// Run track `i`'s inserts into its own work buffers.
    ///
    /// # Safety
    ///
    /// Each index is run by at most one caller at a time: the job owns that
    /// track's chain and work buffers while it runs.
    pub(super) unsafe fn run(&self, i: usize, context: &FxContext<'_>) {
        assert!(i < self.len);
        let frames = self.frames;
        let track = self.tracks.add(i);
        let id = *addr_of!((*track).id);
        let key = (*addr_of!((*track).key_from)).map(|source| self.tap(source));
        let (own_l, own_r) = self.tap(i);
        let chain = &mut *addr_of_mut!((*track).chain);
        let work_l = &mut (*addr_of_mut!((*track).work_l))[..frames];
        let work_r = &mut (*addr_of_mut!((*track).work_r))[..frames];
        let scratch = &mut *addr_of_mut!((*track).fx_scratch);

        // The inserts run on a copy so that `buf_l`/`buf_r` stay as the
        // instrument left them — they are every keyed chain's tap.
        work_l.copy_from_slice(own_l);
        work_r.copy_from_slice(own_r);
        if !chain.is_empty() {
            let ctx = FxContext { key, ..*context };
            chain.process(work_l, work_r, &ctx, scratch);
        }

        // ── Key listen ──
        //
        // The key replaces this track's signal, *after* the chain has run —
        // so the gain-reduction meter goes on moving while the key is being
        // auditioned, which is most of what the switch is for. What is heard
        // is the key as the sidechain tap defines it: post-instrument,
        // pre-insert, and this track's own signal when no other track has
        // been named.
        //
        // It is here and not inside the compressor deliberately. A slot
        // cannot replace the track's output, and a version that wrote the key
        // into the buffer from inside the chain would make what you hear
        // depend on which effects happen to sit after it. The cost is that
        // the detector's high-pass is not in what you hear; the compensation
        // is that what you hear does not change when you add a delay.
        if self.key_listen == Some(id) {
            let (left, right) = key.unwrap_or((own_l, own_r));
            work_l.copy_from_slice(left);
            work_r.copy_from_slice(right);
        }
    }
}

/// Run a send bus's own inserts over what the tracks fed it.
///
/// A bus with nothing in it and nothing fed to it is skipped entirely, which
/// is what keeps a session with no sends bit-identical to one from before
/// they existed.
pub(super) fn run_bus(bus: &mut BusStrip, frames: usize, context: &FxContext<'_>) {
    if !bus.fed && bus.chain.is_empty() {
        return;
    }
    bus.chain.process(&mut bus.buf_l[..frames], &mut bus.buf_r[..frames], context, &mut bus.fx_scratch);
}
