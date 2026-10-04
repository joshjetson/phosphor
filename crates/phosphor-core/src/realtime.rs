//! Asking the operating system to treat a thread as real-time audio.
//!
//! A thread that has to finish its work every few milliseconds should not
//! wait behind the screen redraw or a background download. On macOS and
//! Windows the audio library already runs the audio callback at real-time
//! priority; on Linux it does not, and nor does anything else, so there the
//! callback asks for itself. The worker threads that share the callback's
//! work ask everywhere, since they are as urgent as the callback is.
//!
//! Asking can be refused — on Linux the user needs real-time permission
//! (membership of the `audio` group, an `rtprio` limit, or `CAP_SYS_NICE`).
//! A refusal is reported once and changes nothing else: the thread runs at
//! normal priority, as it always did.

use std::sync::atomic::{AtomicBool, Ordering};

/// Ask for real-time scheduling for the calling thread, which does
/// `frames`-frame blocks of audio at `rate` Hz. Returns whether it was
/// granted. Makes system calls: call it once, when the thread starts.
pub fn promote_current_thread(frames: u32, rate: u32) -> bool {
    match audio_thread_priority::promote_current_thread_to_real_time(frames, rate) {
        // The handle is only for demoting the thread again, which an audio
        // thread never wants; letting it go leaves the thread real-time
        // until it ends.
        Ok(_) => true,
        Err(error) => {
            static REPORTED: AtomicBool = AtomicBool::new(false);
            if !REPORTED.swap(true, Ordering::Relaxed) {
                tracing::info!("audio threads run at normal priority: real-time scheduling was refused ({error:?})");
            }
            false
        }
    }
}
