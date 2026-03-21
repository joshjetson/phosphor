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

use crate::state::{self, ClipViewFocus, FxPanelTab, InstrumentType, NavState, Pane, SpaceAction};
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
    engine: Arc<Engine>,
    nav: NavState,
    running: bool,
    _audio_backend: Option<CpalBackend>,
    _midi_status: Arc<MidiStatus>,
    _midi_connection: Option<midir::MidiInputConnection<()>>,
    next_track_id: usize,
    clip_rx: crossbeam_channel::Receiver<ClipSnapshot>,
}

impl App {
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
        }
    }

    pub fn run(&mut self) -> Result<()> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        let result = self.main_loop(&mut terminal);
        terminal::disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        result
    }

    fn main_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        while self.running {
            self.nav.tick();
            for track in &self.nav.tracks {
                track.sync_to_audio();
            }

            // Poll for recorded clip snapshots from the audio thread
            while let Ok(snap) = self.clip_rx.try_recv() {
                self.nav.receive_clip_snapshot(snap);
            }

            let snapshot = self.engine.transport.snapshot();

            terminal.draw(|frame| {
                ui::render(frame, &snapshot, &self.nav);
            })?;

            if event::poll(Duration::from_millis(16))? {
                self.handle_event(event::read()?);
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) {
        let Event::Key(key) = event else { return };

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.running = false;
            return;
        }

        // Loop editor active — controls locked to loop markers
        if self.nav.loop_editor.active {
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Esc => self.nav.loop_editor.escape(),
                KeyCode::Char('h') | KeyCode::Left => {
                    if shift {
                        self.nav.loop_editor.move_end_left();
                    } else {
                        self.nav.loop_editor.move_start_left();
                    }
                    self.sync_loop_to_transport();
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    if shift {
                        self.nav.loop_editor.move_end_right();
                    } else {
                        self.nav.loop_editor.move_start_right();
                    }
                    self.sync_loop_to_transport();
                }
                KeyCode::Char('H') => {
                    self.nav.loop_editor.move_end_left();
                    self.sync_loop_to_transport();
                }
                KeyCode::Char('L') => {
                    self.nav.loop_editor.move_end_right();
                    self.sync_loop_to_transport();
                }
                _ => {} // all other keys ignored while loop editor is active
            }
            return;
        }

        // Instrument modal open
        if self.nav.instrument_modal.open {
            match key.code {
                KeyCode::Esc => self.nav.escape(),
                KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
                KeyCode::Enter => {
                    let instrument = self.nav.instrument_modal.selected();
                    self.nav.instrument_modal.open = false;
                    self.create_instrument_track(instrument);
                }
                _ => {}
            }
            return;
        }

        // Space menu open
        if self.nav.space_menu.open {
            match key.code {
                KeyCode::Char(' ') | KeyCode::Esc => { self.nav.space_menu.open = false; }
                KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
                KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
                KeyCode::Tab => self.nav.space_menu.switch_section(),
                KeyCode::Enter => {
                    if let Some(action) = self.nav.enter() {
                        self.handle_space_action(action);
                    }
                }
                KeyCode::Char(ch) => {
                    if let Some(action) = self.nav.space_menu_handle(ch) {
                        self.handle_space_action(action);
                    }
                }
                _ => {}
            }
            return;
        }

        // Space → open space menu
        if key.code == KeyCode::Char(' ') {
            self.nav.toggle_space_menu();
            return;
        }

        // Tab
        match key.code {
            KeyCode::Tab if self.nav.focused_pane == Pane::ClipView => {
                self.nav.cycle_tab();
                return;
            }
            KeyCode::Tab => { self.nav.focus_next_pane(); return; }
            KeyCode::BackTab => { self.nav.focus_pane(self.nav.focused_pane.prev()); return; }
            _ => {}
        }

        match self.nav.focused_pane {
            Pane::Tracks => self.handle_tracks_keys(key),
            Pane::ClipView => self.handle_clip_view_keys(key),
        }
    }

    fn handle_tracks_keys(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') if !self.nav.track_selected && !self.nav.fx_menu.open => {
                self.running = false;
            }
            KeyCode::Esc => self.nav.escape(),
            KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
            KeyCode::Char('h') | KeyCode::Left => self.nav.move_left(),
            KeyCode::Char('l') | KeyCode::Right => self.nav.move_right(),
            KeyCode::Enter => { self.nav.enter(); }
            KeyCode::Char('m') if !self.nav.fx_menu.open => self.nav.toggle_mute(),
            KeyCode::Char('s') if !self.nav.fx_menu.open => self.nav.toggle_solo(),
            KeyCode::Char('r') if !self.nav.fx_menu.open => self.nav.toggle_arm(),
            // R (shift+r) = toggle loop recording on the selected track
            KeyCode::Char('R') if !self.nav.fx_menu.open => {
                self.toggle_loop_record();
            }
            KeyCode::Char(ch @ '0'..='9') if self.nav.track_selected && !self.nav.fx_menu.open => {
                self.nav.digit_input(ch);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.engine.transport.set_tempo(self.engine.transport.tempo_bpm() + 1.0);
            }
            KeyCode::Char('-') => {
                self.engine.transport.set_tempo((self.engine.transport.tempo_bpm() - 1.0).max(20.0));
            }
            _ => {}
        }
    }

    fn handle_clip_view_keys(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.nav.escape(),
            KeyCode::Char('j') | KeyCode::Down => self.nav.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.nav.move_up(),
            KeyCode::Char('h') | KeyCode::Left => {
                self.nav.move_left();
                self.send_synth_param_update();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.nav.move_right();
                self.send_synth_param_update();
            }
            _ => {}
        }
    }

    /// If a synth param was just adjusted, send the update to the audio thread.
    fn send_synth_param_update(&self) {
        if self.nav.focused_pane != Pane::ClipView
            || self.nav.clip_view.focus != ClipViewFocus::FxPanel
            || self.nav.clip_view.fx_panel_tab != FxPanelTab::Synth
        {
            return;
        }
        let idx = self.nav.clip_view.synth_param_cursor;
        if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
            if let (Some(mixer_id), Some(&val)) = (track.mixer_id, track.synth_params.get(idx)) {
                let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetParameter {
                    track_id: mixer_id,
                    param_index: idx,
                    value: val,
                });
            }
        }
    }

    fn handle_space_action(&mut self, action: SpaceAction) {
        match action {
            SpaceAction::PlayPause => {
                if self.engine.transport.is_playing() {
                    self.engine.transport.pause();
                } else {
                    self.engine.transport.play();
                }
            }
            SpaceAction::ToggleRecord => self.engine.transport.toggle_record(),
            SpaceAction::ToggleLoop => {
                // Space+l: activate the loop editor (enter it)
                self.nav.loop_editor.enter();
                // Enable looping on the transport
                if !self.engine.transport.is_looping() {
                    self.engine.transport.toggle_loop();
                }
                self.sync_loop_to_transport();
            }
            SpaceAction::Panic => {
                self.engine.panic();
                tracing::info!("PANIC: all sound killed");
            }
            SpaceAction::AddInstrument => {
                self.nav.instrument_modal.open = true;
                self.nav.instrument_modal.cursor = 0;
            }
            SpaceAction::Save => { /* future */ }
            SpaceAction::NewTrack => { /* future */ }
        }
    }
    /// Push the loop editor's bar range to the transport.
    fn sync_loop_to_transport(&self) {
        self.engine.transport.set_loop_range(
            self.nav.loop_editor.start_ticks(),
            self.nav.loop_editor.end_ticks(),
        );
    }

    /// Toggle loop recording on the current track.
    /// First press: arms track, sets loop range, rewinds, starts record+play.
    /// Second press: stops recording, commits clip.
    fn toggle_loop_record(&mut self) {
        let is_recording = self.engine.transport.is_recording()
            && self.engine.transport.is_playing();

        if is_recording {
            // Stop recording
            self.engine.transport.stop_loop_record();
            tracing::info!("Loop recording stopped");
        } else {
            // Make sure current track is armed and has a synth
            if let Some(track) = self.nav.tracks.get(self.nav.track_cursor) {
                if !track.is_live() {
                    tracing::info!("Cannot record on a non-instrument track");
                    return;
                }
            } else {
                return;
            }

            // Arm the track if not already
            if let Some(track) = self.nav.tracks.get_mut(self.nav.track_cursor) {
                track.armed = true;
                track.sync_to_audio();
            }

            // Ensure this track is selected for MIDI
            self.nav.show_current_track_controls();

            // Start loop recording (rewinds to loop start, enables loop+record+play)
            self.engine.transport.start_loop_record();
            tracing::info!(
                "Loop recording started: bars {}..{} (ticks {}..{})",
                self.engine.transport.loop_start() / (Transport::PPQ * 4) + 1,
                self.engine.transport.loop_end() / (Transport::PPQ * 4),
                self.engine.transport.loop_start(),
                self.engine.transport.loop_end(),
            );
        }
    }

    /// Create an instrument track in both the audio mixer and the TUI.
    fn create_instrument_track(&mut self, instrument: InstrumentType) {
        let track_id = self.next_track_id;
        self.next_track_id += 1;

        // Create shared handle
        let handle = Arc::new(TrackHandle::new(track_id, TrackKind::Instrument));
        handle.config.armed.store(true, Ordering::Relaxed);

        // Send AddTrack command to the audio mixer
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::AddTrack {
            kind: TrackKind::Instrument,
            handle: handle.clone(),
        });

        // Send SetInstrument command — for now all types use PhosphorSynth
        let synth = Box::new(PhosphorSynth::new());
        let _ = self.engine.shared.mixer_command_tx.send(MixerCommand::SetInstrument {
            track_id,
            instrument: synth,
        });

        // Add to TUI track list with the handle wired in
        self.nav.add_instrument_track(instrument, track_id, handle);
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
                // Update status for UI
                if let phosphor_midi::MidiMessageType::NoteOn { note, .. } = msg.message_type {
                    status_clone.last_note.store(note, Ordering::Relaxed);
                }
                status_clone.message_count.fetch_add(1, Ordering::Relaxed);

                // Push to ring buffer for audio thread
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
