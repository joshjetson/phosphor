//! App methods: session io.

use super::*;

impl App {

    // ── Session save/load ──

    pub(crate) fn handle_save(&mut self) {
        if let Some(ref path) = self.session_path.clone() {
            // Quick save to existing path
            self.do_save(&path.display().to_string());
        } else {
            // First save — prompt for filename
            self.nav.input_modal.open_save("untitled.phos");
        }
    }


    pub(crate) fn do_save(&mut self, path_str: &str) {
        let path = std::path::PathBuf::from(path_str);
        // Ensure .phos extension
        let path = if path.extension().map(|e| e == "phos").unwrap_or(false) {
            path
        } else {
            path.with_extension("phos")
        };

        match crate::session::save(&path, &self.nav, &self.engine.transport) {
            Ok(()) => {
                self.session_path = Some(path.clone());
                self.status_message = Some((
                    format!("saved: {}", path.display()),
                    std::time::Instant::now(),
                ));
            }
            Err(e) => {
                self.status_message = Some((
                    format!("save failed: {e}"),
                    std::time::Instant::now(),
                ));
            }
        }
    }


    pub(crate) fn do_load(&mut self, path_str: &str) {
        // A relative path is tried against the working directory first, which
        // is where it has always resolved, and then against the application
        // directory — so `sessions/take3.phos` still opens once the player
        // stops launching from a checkout. Saving does not do this: a write
        // goes exactly where it was typed. See `phosphor_app::paths`.
        let path = phosphor_app::paths::find_session(std::path::Path::new(path_str));

        // Stop the transport before touching any session state. If playback
        // or recording was rolling, the audio thread keeps advancing the
        // playhead and honoring the `recording` atomic as we swap tracks in
        // and out — so new-session clips would get walked over by the old
        // playhead, armed tracks from the restored session could start
        // recording immediately, and the metronome would keep clicking
        // across the transition.
        //
        // `stop_playback` calls `transport.pause()` (clears `playing`),
        // sets recording_grace for any armed old-session tracks, and calls
        // `engine.panic()` to kill live voices. It does NOT clear the
        // `recording` atomic (pause() leaves it set), so we follow up with
        // `stop_loop_record()` which unconditionally clears both `playing`
        // and `recording`. Finally we reset position to match the
        // `transport.stop()` convention used elsewhere — the loop range is
        // about to be overwritten with the new session's values anyway, so
        // leaving the playhead at an arbitrary old-session offset is
        // nonsense in the new session.
        //
        // Ordering note: if `crate::session::load` fails below, we've
        // already stopped the user's playback. That's a minor UX cost
        // (they hit Space to resume) traded against silent corruption if
        // we parsed first and the transport kept running through a
        // successful load. Stopping first is the correct trade.
        //
        // Race note: stopping during an in-progress recording may cause
        // the mixer's next callback to commit a final ClipSnapshot (see
        // `commit_recording` in mixer.rs when `!should_record &&
        // track.was_recording`). That snapshot may arrive before or
        // after our clip_rx drain below. The drain catches anything in
        // flight at drain time; `recording_grace = 0` below ensures any
        // straggler that arrives later is dropped by
        // `receive_clip_snapshot`.
        self.stop_playback();
        self.engine.transport.stop_loop_record();
        self.engine.transport.set_position(0);

        let session = match crate::session::load(&path) {
            Ok(s) => s,
            Err(e) => {
                self.status_message = Some((
                    format!("open failed: {e}"),
                    std::time::Instant::now(),
                ));
                return;
            }
        };

        // Apply transport settings
        self.engine.transport.set_tempo(session.transport.tempo_bpm);
        if session.transport.metronome != self.engine.transport.is_metronome_on() {
            self.engine.transport.toggle_metronome();
        }
        self.nav.loop_editor.start_bar = session.transport.loop_start_bar;
        self.nav.loop_editor.end_bar = session.transport.loop_end_bar;
        self.nav.loop_editor.enabled = session.transport.loop_enabled;
        self.sync_loop_to_transport();

        // Remove existing instrument tracks (keep bus tracks)
        // First: tell the audio-thread mixer to drop each instrument track's
        // plugin. Without this, old Box<dyn Plugin> instances stay resident
        // and a new session's MIDI triggers both old and new voices.
        for track in &self.nav.tracks {
            if track.instrument_type.is_some() {
                if let Some(mid) = track.mixer_id {
                    let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::RemoveTrack {
                        track_id: mid,
                    });
                }
            }
        }
        // Now clear instrument tracks from TUI state
        self.nav.tracks.retain(|t| t.instrument_type.is_none());
        self.nav.track_cursor = 0;
        self.nav.track_scroll = 0;
        self.nav.track_selected = false;
        self.nav.clip_view_visible = false;

        // Kill all sound
        self.engine.panic();

        // Drain any in-flight clip snapshots from the old session.
        // The audio thread may have already queued ClipSnapshots for
        // old tracks (finalized recordings, or pending commits from the
        // panic we just requested). If we don't drain, they'll be delivered
        // on the next UI tick — after new-session tracks exist — and either
        // land on the wrong track (track-id collision is unlikely given
        // next_track_id is monotonic, but the invariant shouldn't rely on
        // that) or waste a recording_grace slot in receive_clip_snapshot.
        // Unlike the delete-clip drain, there's nothing to keep-and-replay:
        // every queued snapshot belongs to the old session.
        use crate::debug_log as dbg;
        let mut discarded = 0usize;
        while let Ok(snap) = self.clip_rx.try_recv() {
            dbg::system(&format!(
                "discarded snapshot for track {} during session load",
                snap.track_id,
            ));
            discarded += 1;
        }
        if discarded > 0 {
            dbg::system(&format!("session load: drained {discarded} stale clip snapshot(s)"));
        }
        // Reset recording grace so any late snapshots the audio thread emits
        // after the drain (processing our RemoveTrack / panic on its next
        // callback) don't decrement a grace slot intended for live recording.
        // receive_clip_snapshot gates on `!is_recording && recording_grace == 0`,
        // so zeroing this ensures late old-session snapshots are ignored.
        self.nav.recording_grace = 0;

        // Version 1 stored every selector — the kit, the patch, the cartridge
        // — as the fraction of the knob's travel it sat at, and a fraction
        // only names a patch while the bank is the size it was when the
        // fraction was written. Two banks have changed size since, so a
        // version 1 session can open on the wrong instrument and look
        // perfectly reasonable doing it. It is still loaded, because the
        // fraction is the only evidence of what the player chose and it is
        // right whenever the bank has not moved — but it is not loaded
        // quietly. See `crate::session::FORMAT_VERSION`.
        let legacy_selectors = session.version < crate::session::FORMAT_VERSION;
        let mut clamped_selectors = 0usize;

        // Recreate tracks from session
        for st in &session.tracks {
            let instrument = match crate::session::parse_instrument_type(&st.instrument_type) {
                Some(i) => i,
                None => continue, // skip unknown instruments
            };

            self.create_instrument_track(instrument);

            // Restore track state
            let track_idx = self.nav.track_cursor;
            if let Some(track) = self.nav.tracks.get_mut(track_idx) {
                track.name = st.name.clone();
                track.muted = st.muted;
                track.soloed = st.soloed;
                track.armed = st.armed;
                track.volume = st.volume;
                track.color_index = st.color_index;

                // Restore synth params. The block is positional, so a saved
                // block of a different length is a different panel — the
                // Juno's grew from 16 controls to 25 when its front panel was
                // finished, and the Jupiter's from 16 to 32 — and copying it
                // in slot by slot would load every value into the wrong
                // control. A mismatch keeps the instrument's defaults instead.
                if st.synth_params.len() == track.synth_params.len() {
                    track.synth_params.copy_from_slice(&st.synth_params);

                    // ...then put the selectors where the session said, rather
                    // than where their fractions land against today's bank.
                    for (param, wanted, given) in crate::session::apply_selectors(
                        instrument,
                        &mut track.synth_params,
                        &st.discrete,
                    ) {
                        clamped_selectors += 1;
                        tracing::warn!(
                            "track '{}': control {param} was at position {wanted}, \
                             which this build no longer has — loading {given}",
                            st.name
                        );
                    }
                } else {
                    tracing::warn!(
                        "track '{}': saved {} parameters, instrument has {} — \
                         loading its defaults",
                        st.name, st.synth_params.len(), track.synth_params.len()
                    );
                }

                // Send all params to audio thread
                if let Some(mixer_id) = track.mixer_id {
                    for (i, &val) in track.synth_params.iter().enumerate() {
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetParameter {
                            track_id: mixer_id,
                            param_index: i,
                            value: val,
                        });
                    }
                }

                // Restore clips
                track.clips.clear();
                for sc in &st.clips {
                    let notes = crate::session::session_notes_to_snapshots(&sc.notes);
                    track.clips.push(crate::state::Clip {
                        number: track.clips.len() + 1,
                        width: 4, // will be recalculated by renderer
                        has_content: !notes.is_empty(),
                        start_tick: sc.start_tick,
                        length_ticks: sc.length_ticks,
                        notes,
                        hidden_notes: Vec::new(),
                    });

                    // Send clip to audio thread: create then update events
                    if let Some(mixer_id) = track.mixer_id {
                        let clip_idx = track.clips.len() - 1;
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::CreateClip {
                            track_id: mixer_id,
                            start_tick: sc.start_tick,
                            length_ticks: sc.length_ticks,
                        });
                        let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                            &crate::session::session_notes_to_snapshots(&sc.notes),
                            sc.length_ticks,
                        );
                        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::UpdateClip {
                            track_id: mixer_id,
                            clip_index: clip_idx,
                            events,
                        });
                    }
                }

                track.sync_to_audio();
            }

            // The sequencer last, so that it is attached to a track whose
            // child instrument and panel are already in place.
            if let Some(stored) = &st.sequencer {
                for sync in self.nav.attach_sequencer(stored.to_state()) {
                    let _ = self.engine.shared.mixer_command_tx.send(sync.command());
                }
            }
        }

        // Clean up any phantom/duplicate clips
        self.sync_dedup_to_audio();

        self.session_path = Some(path.clone());
        // Both notes are about the same thing — a patch that may not be the
        // one the session named — and the bottom bar is the only place the
        // player would ever find that out.
        let note = if legacy_selectors {
            " (older format: check each track's patch)"
        } else if clamped_selectors > 0 {
            " (a patch it names is no longer in the bank)"
        } else {
            ""
        };
        if !note.is_empty() {
            dbg::system(&format!("session load:{note}"));
        }
        self.status_message = Some((
            format!("opened: {}{note}", path.display()),
            std::time::Instant::now(),
        ));
    }
}
