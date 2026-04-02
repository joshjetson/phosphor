//! TUI application — wires up audio engine, MIDI input, and the terminal UI.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use phosphor_core::clip::ClipSnapshot;
use phosphor_core::cpal_backend::CpalBackend;
use phosphor_core::engine::{Engine, EngineAudio};
use phosphor_core::mixer::{Mixer, MixerCommand, clip_snapshot_channel, mixer_command_channel};
use phosphor_core::transport::Transport;
use phosphor_core::project::{TrackHandle, TrackKind};
use phosphor_core::EngineConfig;
use phosphor_dsp::synth::PhosphorSynth;
use phosphor_midi::ring::midi_ring_buffer;

use crate::state::{self, ClipViewFocus, ConfirmKind, FxPanelTab, InputModalKind, InstrumentType, NavState, Pane, PianoRollFocus, SpaceAction, TransportElement};
mod delete;
mod edit_mode;
mod keys;
mod piano_roll;
mod session_io;
mod clips;
mod tracks;
mod transport;
mod undo_redo;
use crate::state::undo::UndoAction;
use crate::ui;

/// Shared MIDI status for the UI to display.
pub struct MidiStatus {
    /// Last received note (for display).
    pub last_note: AtomicU8,
    /// Whether any MIDI port is connected.
    pub connected: AtomicBool,
    /// Number of messages received (wraps).
    pub message_count: std::sync::atomic::AtomicU32,
}

impl MidiStatus {
    pub fn new() -> Self {
        Self {
            last_note: AtomicU8::new(0),
            connected: AtomicBool::new(false),
            message_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

pub struct App {
    pub(crate) engine: Arc<Engine>,
    pub(crate) nav: NavState,
    pub(crate) running: bool,
    _audio_backend: Option<CpalBackend>,
    _midi_status: Arc<MidiStatus>,
    _midi_connection: Option<midir::MidiInputConnection<()>>,
    next_track_id: usize,
    clip_rx: crossbeam_channel::Receiver<ClipSnapshot>,
    /// Last saved/loaded file path for Ctrl+S quick save.
    session_path: Option<std::path::PathBuf>,
    /// Status message shown briefly at bottom of screen.
    pub(crate) status_message: Option<(String, std::time::Instant)>,
    /// Yanked (copied) clip for cross-track paste.
    pub(crate) yanked_clip: Option<crate::state::Clip>,
    /// Timeline position of the yanked clip (for cross-track paste at same position).
    pub(crate) yanked_clip_start: i64,
}

impl App {
    /// Create the app with a splash screen shown during init.
    /// Enters alternate screen once — `run()` reuses it.
    pub fn new_with_splash(config: EngineConfig, enable_audio: bool, enable_midi: bool) -> Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let mut splash_terminal = Terminal::new(backend)?;

        // Show splash while we init
        crate::splash::show_splash(&mut splash_terminal)?;

        // Now init — splash stays visible on screen
        let app = Self::new(config, enable_audio, enable_midi);

        // Clean up splash terminal (raw mode stays, alternate screen stays)
        // App::run will create its own terminal on the same alternate screen
        drop(splash_terminal);
        let _ = terminal::disable_raw_mode();

        Ok(app)
    }

    pub fn new(config: EngineConfig, enable_audio: bool, enable_midi: bool) -> Self {
        let (mixer_tx, mixer_rx) = mixer_command_channel();
        let (clip_tx, clip_rx) = clip_snapshot_channel();

        let engine = Arc::new(Engine::with_command_tx(config, mixer_tx.clone()));
        let transport = engine.transport.clone();

        let midi_status = Arc::new(MidiStatus::new());
        let (midi_tx, midi_rx) = midi_ring_buffer();

        // Start MIDI input FIRST so the controller can finish its init burst
        let midi_connection = if enable_midi {
            let status = midi_status.clone();
            start_midi_input(status, midi_tx)
        } else {
            drop(midi_tx);
            None
        };

        // Brief pause to let MIDI controller finish sending init data
        if midi_connection.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        // Start audio engine — flush any stale MIDI before first callback
        let audio_backend = if enable_audio {
            let panic_flag = engine.panic_flag.clone();
            let vu_levels = engine.vu_levels.clone();

            // Create the mixer
            let mixer = Mixer::new(
                mixer_rx,
                vu_levels.clone(),
                clip_tx,
                config.sample_rate,
                config.buffer_size as usize,
            );

            let mut engine_audio = EngineAudio::with_mixer(
                &config,
                mixer,
                Some(midi_rx),
                panic_flag,
                vu_levels,
            );
            // Drain and discard any MIDI events that arrived during init
            engine_audio.flush_midi();
            let transport_clone = transport.clone();

            let mut backend = match CpalBackend::new(config.sample_rate, config.buffer_size) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("Failed to init audio: {e}");
                    return Self {
                        engine,
                        nav: NavState::new(state::initial_tracks()),
                        running: true,
                        _audio_backend: None,
                        _midi_status: midi_status,
                        _midi_connection: midi_connection,
                        next_track_id: 0,
                        clip_rx,
                        session_path: None,
                        status_message: None,
                        yanked_clip: None,
                        yanked_clip_start: 0,
                    };
                }
            };

            if let Err(e) = backend.start(move |data: &mut [f32]| {
                engine_audio.process(data, &transport_clone);
            }) {
                tracing::warn!("Failed to start audio stream: {e}");
            }

            Some(backend)
        } else {
            None
        };

        Self {
            engine,
            nav: NavState::new(state::initial_tracks()),
            running: true,
            _audio_backend: audio_backend,
            _midi_status: midi_status,
            _midi_connection: midi_connection,
            next_track_id: 0,
            clip_rx,
            session_path: None,
            status_message: None,
            yanked_clip: None,
            yanked_clip_start: 0,
        }
    }

    /// Execute a single action. Used by the test harness.
    /// Future: the key handler should map keys→actions then call this.
    #[allow(dead_code)]
    pub(crate) fn execute_action(&mut self, action: crate::actions::Action) {
        use crate::actions::Action;
        use crate::debug_log as dbg;

        match action {
            // Global
            Action::Quit => { self.running = false; }
            Action::OpenSpaceMenu => { self.nav.toggle_space_menu(); }
            Action::CloseSpaceMenu => { self.nav.space_menu.open = false; }
            Action::NextPane => { self.nav.focus_next_pane(); }
            Action::PrevPane => { self.nav.focus_pane(self.nav.focused_pane.prev()); }

            // Space menu
            Action::SpaceMenuUp => { self.nav.move_up(); }
            Action::SpaceMenuDown => { self.nav.move_down(); }
            Action::SpaceMenuSelect => {
                if let Some(sa) = self.nav.enter() {
                    self.handle_space_action(sa);
                }
            }
            Action::SpaceMenuSwitchTab => { self.nav.space_menu.switch_section(); }
            Action::SpaceMenuKey(ch) => {
                if let Some(sa) = self.nav.space_menu_handle(ch) {
                    self.handle_space_action(sa);
                }
            }

            // Transport
            Action::PlayPause => {
                if self.engine.transport.is_playing() {
                    dbg::system("action: stop playback");
                    self.stop_playback();
                } else {
                    if self.nav.loop_editor.enabled {
                        let start = self.nav.loop_editor.start_ticks();
                        dbg::system(&format!("action: play from loop start (tick {start})"));
                        self.engine.transport.set_position(start);
                    }
                    self.sync_loop_to_transport();
                    self.engine.transport.play();
                }
                self.log_transport_state();
            }
            Action::ToggleRecord => {
                self.engine.transport.toggle_record();
                self.log_transport_state();
            }
            Action::ToggleMetronome => {
                self.engine.transport.toggle_metronome();
            }
            Action::Panic => {
                self.engine.panic();
            }
            Action::Save => { /* future */ }

            // Loop editor
            Action::FocusLoopEditor => {
                self.nav.loop_editor.focus();
            }
            Action::LoopToggleEnabled => {
                self.nav.loop_editor.toggle_enabled();
                self.sync_loop_to_transport();
                self.log_transport_state();
            }
            Action::LoopStartLeft => {
                self.nav.loop_editor.move_start_left();
                self.sync_loop_to_transport();
            }
            Action::LoopStartRight => {
                self.nav.loop_editor.move_start_right();
                self.sync_loop_to_transport();
            }
            Action::LoopEndLeft => {
                self.nav.loop_editor.move_end_left();
                self.sync_loop_to_transport();
            }
            Action::LoopEndRight => {
                self.nav.loop_editor.move_end_right();
                self.sync_loop_to_transport();
            }
            Action::LoopUnfocus => {
                self.nav.loop_editor.unfocus();
            }

            // Track navigation
            Action::MoveUp => { self.nav.move_up(); }
            Action::MoveDown => { self.nav.move_down(); }
            Action::MoveLeft => {
                self.nav.move_left();
                self.send_synth_param_update();
            }
            Action::MoveRight => {
                self.nav.move_right();
                self.send_synth_param_update();
            }
            Action::Select => { self.nav.enter(); }
            Action::Back => { self.nav.escape(); }

            // Track controls
            Action::ToggleMute => { self.nav.toggle_mute(); }
            Action::ToggleSolo => { self.nav.toggle_solo(); }
            Action::ToggleArm => { self.nav.toggle_arm(); }
            Action::ToggleLoopRecord => { self.toggle_loop_record(); }

            // Instrument
            Action::AddInstrument => {
                self.nav.instrument_modal.open = true;
                self.nav.instrument_modal.cursor = 0;
            }
            Action::InstrumentSelect => {
                let instrument = self.nav.instrument_modal.selected();
                self.nav.instrument_modal.open = false;
                self.create_instrument_track(instrument);
            }
            Action::InstrumentCancel => {
                self.nav.instrument_modal.open = false;
            }

            // Clip view
            Action::CycleTab => { self.nav.cycle_tab(); }

            // Synth params
            Action::ParamUp => { self.nav.move_up(); }
            Action::ParamDown => { self.nav.move_down(); }
            Action::ParamDecrease => {
                self.nav.move_left();
                self.send_synth_param_update();
            }
            Action::ParamIncrease => {
                self.nav.move_right();
                self.send_synth_param_update();
            }

            Action::None => {}
        }
    }

    pub fn run(&mut self) -> Result<()> {
        // Clean up any phantom clips from previous sessions
        self.sync_dedup_to_audio();
        // Sync initial loop range to transport
        self.sync_loop_to_transport();
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Install panic hook that restores terminal before printing the panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                LeaveAlternateScreen,
                crossterm::cursor::Show
            );
            original_hook(info);
        }));

        let result = self.main_loop(&mut terminal);

        // Restore terminal — always runs even if main_loop errored
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );

        result
    }

    fn main_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        use crate::debug_log as dbg;
        let mut frame_count: u64 = 0;
        while self.running {
            self.nav.tick();
            self.nav.sync_clip_view_target();
            for track in &self.nav.tracks {
                track.sync_to_audio();
            }

            // Poll for recorded clip snapshots from the audio thread
            let is_recording = self.engine.transport.is_recording();
            while let Ok(snap) = self.clip_rx.try_recv() {
                let _mixer_id = snap.track_id;
                if let Some((mid, absorbed)) = self.nav.receive_clip_snapshot(snap, is_recording) {
                    // Sync absorbed clips to audio — remove them in reverse order
                    // The audio thread's clip array shifted, so remove from highest index down.
                    // Since we don't know exact indices, rebuild all clips on this track.
                    if let Some(track) = self.nav.tracks.iter().find(|t| t.mixer_id == Some(mid)) {
                        // Remove ALL clips from audio for this track, then re-add current ones
                        // This is the safest way to resync after absorption.
                        for i in (0..track.clips.len() + absorbed).rev() {
                            let _ = self.engine.shared.mixer_command_tx.send(
                                MixerCommand::RemoveClip { track_id: mid, clip_index: i }
                            );
                        }
                        for (ci, clip) in track.clips.iter().enumerate() {
                            let _ = self.engine.shared.mixer_command_tx.send(
                                MixerCommand::CreateClip {
                                    track_id: mid,
                                    start_tick: clip.start_tick,
                                    length_ticks: clip.length_ticks,
                                }
                            );
                            let events = phosphor_core::clip::NoteSnapshot::to_clip_events(
                                &clip.notes, clip.length_ticks,
                            );
                            let _ = self.engine.shared.mixer_command_tx.send(
                                MixerCommand::UpdateClip {
                                    track_id: mid, clip_index: ci, events,
                                }
                            );
                        }
                        crate::debug_log::system(&format!(
                            "audio resync after absorption: track={} clips={}",
                            mid, track.clips.len()
                        ));
                    }
                }
            }

            let snapshot = self.engine.transport.snapshot();

            // Update piano roll dimensions to match terminal size and clip
            let term_h = terminal.size()?.height;
            let term_w = terminal.size()?.width;
            let piano_h = term_h.saturating_sub(30).max(6) as u8;
            self.nav.clip_view.piano_roll.set_view_height(piano_h);

            // Set column count based on actual clip length (beats)
            let ppq = phosphor_core::transport::Transport::PPQ;
            let total_beats = self.nav.active_clip()
                .map(|c| ((c.length_ticks as f64) / ppq as f64).ceil() as usize)
                .unwrap_or(16)
                .max(1);
            self.nav.clip_view.piano_roll.set_column_count(total_beats);

            // Set visible columns based on terminal width
            let key_w = 7usize; // key labels + separator
            let fx_panel_w = 25usize; // FX panel + separator
            let note_w = (term_w as usize).saturating_sub(key_w + fx_panel_w);
            let vis_cols = (note_w / 3).max(1).min(total_beats);
            self.nav.clip_view.piano_roll.visible_columns = vis_cols;

            // Log frame details periodically and on first frame after track creation
            if frame_count < 3 || frame_count % 500 == 0 {
                dbg::system(&format!(
                    "frame={frame_count} term={}x{} tracks={} focused={:?} cursor={}",
                    term_w, term_h, self.nav.tracks.len(),
                    self.nav.focused_pane, self.nav.track_cursor,
                ));
            }

            terminal.draw(|frame| {
                ui::render(frame, &snapshot, &self.nav);
            })?;

            frame_count += 1;

            if event::poll(Duration::from_millis(16))? {
                self.handle_event(event::read()?);
            }
        }
        Ok(())
    }
}

/// Start MIDI input on the first available port.
fn start_midi_input(
    status: Arc<MidiStatus>,
    mut midi_tx: phosphor_midi::ring::MidiRingSender,
) -> Option<midir::MidiInputConnection<()>> {
    let midi_in = match midir::MidiInput::new("phosphor") {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Failed to init MIDI: {e}");
            return None;
        }
    };

    let ports = midi_in.ports();
    if ports.is_empty() {
        tracing::info!("No MIDI input ports found");
        return None;
    }

    let port = &ports[0];
    let port_name = midi_in.port_name(port).unwrap_or_else(|_| "unknown".into());
    tracing::info!("Connecting to MIDI port: {port_name}");

    let status_clone = status.clone();
    match midi_in.connect(
        port,
        "phosphor-in",
        move |timestamp, data, _| {
            if let Some(msg) = phosphor_midi::MidiMessage::from_bytes(data, timestamp) {
                if let phosphor_midi::MidiMessageType::NoteOn { note, .. } = msg.message_type {
                    status_clone.last_note.store(note, Ordering::Relaxed);
                }
                status_clone.message_count.fetch_add(1, Ordering::Relaxed);
                midi_tx.push(msg);
            }
        },
        (),
    ) {
        Ok(conn) => {
            status.connected.store(true, Ordering::Relaxed);
            tracing::info!("MIDI connected: {port_name}");
            Some(conn)
        }
        Err(e) => {
            tracing::warn!("Failed to connect MIDI: {e}");
            None
        }
    }
}
