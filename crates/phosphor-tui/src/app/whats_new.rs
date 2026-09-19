//! App methods: the "what's new" card.
//!
//! The parsing and the decision are pure and live in `phosphor_app::whats_new`;
//! this is only the two places the running application touches the disk — filling
//! the card at startup from the last-seen file, and writing that file back when
//! the card is dismissed. Both are reached only from the real launch path (see
//! [`crate::run`]), never from `App::new`, so no headless build or test ever
//! reads or writes the player's home directory by constructing an app.

use super::*;

impl App {
    /// Fill the card with whatever this launch should announce, if anything.
    ///
    /// Called once from the launch path after the app is built. Reads the
    /// last-seen version off disk and asks `phosphor_app::whats_new` for the
    /// entries between it and the running version; an empty answer leaves the
    /// card closed, which is the ordinary case for a version already seen.
    pub(crate) fn show_whats_new_on_startup(&mut self) {
        let last_seen = phosphor_app::whats_new::read_last_seen();
        let entries = phosphor_app::whats_new::whats_new(
            phosphor_app::whats_new::CHANGELOG,
            env!("CARGO_PKG_VERSION"),
            last_seen.as_deref(),
        );
        self.nav.whats_new.show(entries, env!("CARGO_PKG_VERSION"));
    }

    /// Take the card down and record the version as seen.
    ///
    /// The write happens on dismiss rather than on show: a crash while the card
    /// is up costs the player a second look at the same card, which is a kinder
    /// failure than a version whose news is never told.
    pub(crate) fn dismiss_whats_new(&mut self) {
        if let Some(version) = self.nav.whats_new.dismiss() {
            phosphor_app::whats_new::write_last_seen(&version);
        }
    }
}
