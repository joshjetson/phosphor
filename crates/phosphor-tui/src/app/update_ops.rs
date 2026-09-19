//! App methods: the crates.io update notice.
//!
//! The check itself — the thread, the network, the cache — lives in
//! [`crate::update`]. These are the two touch-points the running app has with
//! it: starting it (from the launch path only), and lifting its answer out of
//! the shared slot into the navigation state each frame so the bottom bar can
//! draw it. Dismissing the notice is a one-line flag flip and lives with the
//! key handling.

use super::*;

impl App {
    /// Start the background update check.
    ///
    /// Called once from the real launch path, never from `App::new`: it spawns
    /// a thread that reaches the network, and both of those are forbidden to a
    /// headless or test app. Disabling, caching, and the six-hour window are all
    /// decided inside [`crate::update::spawn`].
    pub(crate) fn start_update_check(&self) {
        crate::update::spawn(
            self.update_notice.clone(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
    }

    /// Whether an Esc right now should be spent dismissing the update notice.
    ///
    /// Only at the plain track list with nothing selected and no overlay up —
    /// exactly the state where Esc otherwise does nothing. Everywhere else Esc
    /// already means "back out of this", and a notice in the bottom bar has no
    /// claim on it; the player backs out to the top level and Esc there waves
    /// the notice away. This keeps one key from ever doing two jobs.
    #[must_use]
    pub(crate) fn update_notice_dismissible(&self) -> bool {
        let n = &self.nav;
        n.focused_pane == crate::state::Pane::Tracks
            && !n.track_selected
            && n.number_buf.display().is_empty()
            && !n.loop_editor.active
            && !n.element_locked
            && !n.space_menu.open
            && !n.fx_menu.open
            && !n.preset_modal.open
            && !n.prog_editor.open
            && !n.quantize_modal.open
            && !n.practice.open
            && !n.question_is_up()
    }

    /// Lift a newer version out of the shared slot into the navigation state.
    ///
    /// A non-blocking `try_lock`: the background thread writes the slot exactly
    /// once, so a frame that loses the lock simply shows the notice one frame
    /// later. Once carried across it stays — the check does not run twice — so
    /// there is nothing to clear.
    pub(crate) fn poll_update_notice(&mut self) {
        if self.nav.update_available.is_some() {
            return;
        }
        if let Ok(slot) = self.update_notice.try_lock() {
            if let Some(version) = slot.as_ref() {
                self.nav.update_available = Some(version.clone());
            }
        }
    }
}
