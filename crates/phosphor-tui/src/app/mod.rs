//! TUI application — wires up audio engine, MIDI input, and the terminal UI.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use phosphor_core::clip::ClipSnapshot;
use phosphor_core::cpal_backend::{CpalBackend, StreamFormat};
use phosphor_core::engine::{Engine, EngineAudio};
use phosphor_core::fx::GrMeter;
use phosphor_core::mixer::{Mixer, MixerCommand, clip_snapshot_channel, mixer_command_channel};
use phosphor_core::transport::Transport;
use phosphor_core::project::{TrackHandle, TrackKind};
use phosphor_core::{AudioRequest, EngineConfig};
use phosphor_dsp::synth::PhosphorSynth;
use phosphor_midi::ring::midi_ring_buffer;

use crate::state::{self, ClipTab, ClipViewFocus, ConfirmKind, FxPanelTab, InputModalKind, InstrumentType, NavState, Pane, PianoRollFocus, SpaceAction, TransportElement};
mod delete;
mod edit_mode;
mod keys;
mod fx_keys;
pub(crate) use fx_keys::pan_label;
mod sequencer_bounce;
pub(crate) mod sequencer_keys;
mod sequencer_record;
mod piano_roll;
mod presets;
mod session_io;
mod clips;
mod midi_fx_ops;
mod practice_ops;
mod tracks;
mod transport;
mod undo_redo;
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
    /// Where user preset banks are kept — `~/.phosphor/presets` on Unix,
    /// `%APPDATA%\phosphor\presets` on Windows, or `None` when the
    /// environment names no home directory at all. A field rather than a call
    /// so the tests can point it at a scratch directory instead of the
    /// player's own presets.
    pub(crate) preset_dir: Option<std::path::PathBuf>,
    /// Status message shown briefly at bottom of screen.
    pub(crate) status_message: Option<(String, std::time::Instant)>,
    /// Yanked (copied) clips, for paste and for cross-track layering. One
    /// entry from `y` on a clip; a whole arrangement from `y` on the track
    /// label. Each clip keeps its own `start_tick`, which is what lets `P`
    /// lay the set onto another track on exactly the bars it came from.
    pub(crate) yanked_clips: Vec<crate::state::Clip>,
    /// One sequencer step, yanked with everything on it — chord, voicing,
    /// gate, accent — waiting for `p` on another grid position.
    pub(crate) seq_step_clip: Option<phosphor_core::pattern::Step>,
    /// A whole pattern, yanked from the instrument row, waiting for `p` on
    /// any sequencer — this track's or another's.
    pub(crate) seq_pattern_clip: Option<Box<phosphor_core::pattern::PatternBlock>>,
    /// The UI's tap on MIDI input, for step record.
    ///
    /// The audio thread's ring has one consumer and this is not it: the
    /// `midir` callback fills both. `None` when MIDI is off, which is every
    /// test — those call the step-record entry points directly.
    pub(crate) midi_ui_rx: Option<crossbeam_channel::Receiver<phosphor_midi::MidiMessage>>,
    /// Notes with a key still down.
    pub(crate) held_notes: Vec<u8>,
    /// Every note touched since the last one was let go — the chord being
    /// played, which is written when the last finger lifts.
    pub(crate) recorded_notes: Vec<u8>,
    /// Note-ons seen since the recorder last emptied its buffer — by
    /// committing a take, starting, stopping, or discarding a pass. This is
    /// how undo-while-recording knows whether the newest layer is the pass
    /// under the player's fingers or the take that already committed: the
    /// UI's MIDI tap sees every note the recorder can, so a zero here means
    /// the pass is empty. Approximate on purpose — it does not know which
    /// armed track a note landed on — and the cost of being wrong is one
    /// press of `u` peeling the other layer first.
    pub(crate) live_take_notes: usize,
    /// The receiving end of the mixer command channel, kept alive only when
    /// there is no mixer to own it — which is only ever in tests, since a
    /// headless app has no audio thread.
    ///
    /// Without it, a headless send goes to a disconnected channel and a test
    /// cannot tell "the UI told the audio thread" from "the UI updated its
    /// own copy and nothing else". That distinction is the difference between
    /// a control that works and one that only looks like it does.
    #[cfg(test)]
    pub(crate) mixer_rx: Option<crossbeam_channel::Receiver<MixerCommand>>,
}

/// What the engine gets built from once the device has had its say.
struct AudioFormat {
    /// The rate and block size the engine actually runs at.
    config: EngineConfig,
    /// The largest block the audio callback can be handed — what the mixer's
    /// buffers are sized to, so `Mixer::process` never has to grow one.
    max_buffer_frames: usize,
    /// Set when the device would not take the requested format, so the
    /// divergence reaches the player instead of being inaudible until they
    /// notice the whole session is sharp.
    notice: Option<String>,
}

impl AudioFormat {
    /// `granted` is `None` when there is no device at all — `--no-audio`, or
    /// a backend that would not open. Then there is nothing to follow and
    /// nothing to override the request, so `AudioRequest::without_device`
    /// fills the gaps; nothing is listening to the result, so nothing can be
    /// out of tune.
    fn resolve(request: AudioRequest, granted: Option<StreamFormat>) -> Self {
        let Some(format) = granted else {
            let config = request.without_device();
            return Self {
                config,
                max_buffer_frames: config.buffer_size as usize,
                notice: None,
            };
        };
        Self {
            config: EngineConfig::from(format),
            max_buffer_frames: format.max_buffer_frames as usize,
            notice: format.divergence_notice(),
        }
    }
}

impl App {
    /// Create the app with a splash screen shown during init.
    /// Enters alternate screen once — `run()` reuses it.
    pub fn new_with_splash(request: AudioRequest, enable_audio: bool, enable_midi: bool) -> Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let mut splash_terminal = Terminal::new(backend)?;

        // Show splash while we init
        crate::splash::show_splash(&mut splash_terminal)?;

        // Now init — splash stays visible on screen
        let app = Self::new(request, enable_audio, enable_midi);

        // Clean up splash terminal (raw mode stays, alternate screen stays)
        // App::run will create its own terminal on the same alternate screen
        drop(splash_terminal);
        let _ = terminal::disable_raw_mode();

        Ok(app)
    }

    /// `request` is anything that can name an [`AudioRequest`]: the command
    /// line's optional pair, or a concrete [`EngineConfig`] for the headless
    /// tests, which open no device and so have nothing to follow.
    pub fn new(request: impl Into<AudioRequest>, enable_audio: bool, enable_midi: bool) -> Self {
        let request = request.into();
        let (mixer_tx, mixer_rx) = mixer_command_channel();
        let (clip_tx, clip_rx) = clip_snapshot_channel();

        // With audio running the mixer is the only receiver and this stays
        // `None`: a second receiver on the same channel would take commands
        // the mixer needs, since each message goes to exactly one of them.
        #[cfg(test)]
        let mixer_rx_test = (!enable_audio).then(|| mixer_rx.clone());

        // The device is opened first, before anything is built from a sample
        // rate, because the device is the one that decides what the sample
        // rate is. `request` is what was asked for on the command line, and
        // is usually empty; building the engine from it while the stream runs
        // at something else detunes the whole application by the ratio
        // between them.
        //
        // `--no-audio` never gets here at all, so it queries no device.
        let mut backend = enable_audio
            .then(|| CpalBackend::new(request))
            .transpose()
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to init audio: {e}");
                None
            });

        let AudioFormat { config, max_buffer_frames, notice: format_notice } =
            AudioFormat::resolve(request, backend.as_ref().map(CpalBackend::format));
        // What the engine was actually built at, every launch. The rate is
        // not visible anywhere in the UI, and "it sounds a bit sharp" is a
        // hard thing to debug without it written down.
        crate::debug_log::log(
            "AUDIO",
            &format!(
                "engine at {}Hz, blocks up to {} frames, device: {}",
                config.sample_rate,
                max_buffer_frames,
                if backend.is_some() { "yes" } else { "none" },
            ),
        );
        if let Some(notice) = format_notice.as_deref() {
            crate::debug_log::log("AUDIO", notice);
        }

        let engine = Arc::new(Engine::with_command_tx(config, mixer_tx.clone()));
        let transport = engine.transport.clone();
        let limiter_gr = Arc::new(GrMeter::new());

        let midi_status = Arc::new(MidiStatus::new());
        let (midi_tx, midi_rx) = midi_ring_buffer();

        // Start MIDI input FIRST so the controller can finish its init burst
        let (midi_ui_tx, midi_ui_rx) = crossbeam_channel::unbounded();
        let midi_connection = if enable_midi {
            let status = midi_status.clone();
            start_midi_input(status, midi_tx, midi_ui_tx)
        } else {
            drop(midi_tx);
            drop(midi_ui_tx);
            None
        };

        // Brief pause to let MIDI controller finish sending init data
        if midi_connection.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        // Start audio engine — flush any stale MIDI before first callback
        if let Some(backend) = backend.as_mut() {
            let panic_flag = engine.panic_flag.clone();
            let vu_levels = engine.vu_levels.clone();

            // Create the mixer. Sized for the largest block the device says it
            // may deliver, not for the block we asked for: growing a buffer
            // inside `Mixer::process` is a heap allocation on the audio thread.
            let mut mixer = Mixer::new(
                mixer_rx,
                vu_levels.clone(),
                clip_tx,
                config.sample_rate,
                max_buffer_frames,
            );
            mixer.set_limiter_gr_meter(limiter_gr.clone());

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

            if let Err(e) = backend.start(move |data: &mut [f32]| {
                engine_audio.process(data, &transport_clone);
            }) {
                tracing::warn!("Failed to start audio stream: {e}");
            }
        }

        let mut nav = NavState::new(state::initial_tracks());
        // The UI reads the limiter's gain reduction the same way it reads a
        // track's meter: through a handle onto audio-thread state.
        nav.limiter_gr = limiter_gr.clone();
        nav.sample_rate = config.sample_rate;
        // The two send buses and the master are strips in the mixer, not
        // tracks: the command carries their kind, and the mixer files each
        // one where it belongs. Without this the buses have no meters, no
        // return level and nowhere to put an effect.
        for track in &nav.tracks {
            if let (true, Some(handle)) = (track.is_bus(), track.handle.clone()) {
                let _ = mixer_tx.send(MixerCommand::AddTrack { kind: track.kind, handle });
            }
        }

        let mut app = Self {
            engine,
            nav,
            running: true,
            _audio_backend: backend,
            _midi_status: midi_status,
            _midi_connection: midi_connection,
            next_track_id: 0,
            clip_rx,
            session_path: None,
            preset_dir: phosphor_app::preset::default_dir(),
            // A device that would not take the requested format says so on the
            // bottom bar. Stderr is not available here — it would be painted
            // over by the UI — and silence is how the mismatch went unnoticed.
            status_message: format_notice.map(|m| (m, std::time::Instant::now())),
            yanked_clips: Vec::new(),
            seq_step_clip: None,
            seq_pattern_clip: None,
            midi_ui_rx: enable_midi.then_some(midi_ui_rx),
            held_notes: Vec::new(),
            recorded_notes: Vec::new(),
            live_take_notes: 0,
            #[cfg(test)]
            mixer_rx: mixer_rx_test,
        };

        // What a new session's buses start with. Empty today — see
        // `phosphor_app::fx::bus_default_chain`, which is where the plate
        // reverb and the synced delay land when they exist. The wiring is
        // here so that adding them is a change to that one function.
        for index in 0..app.nav.tracks.len() {
            if app.nav.tracks[index].is_bus() && !app.nav.tracks[index].fx_chain.is_empty() {
                app.install_chain(index);
            }
        }
        app
    }

    /// How long a status message stays on the bottom bar.
    ///
    /// Long enough to read a file path, short enough that the key hints it
    /// covers come back before they are missed.
    const STATUS_TIMEOUT: Duration = Duration::from_secs(4);

    /// The status message if it is still current, `None` once it has expired.
    pub(crate) fn live_status(&self) -> Option<&str> {
        self.status_message
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Self::STATUS_TIMEOUT)
            .map(|(msg, _)| msg.as_str())
    }

    /// Everything the UI has sent to the audio thread since the last drain.
    #[cfg(test)]
    pub(crate) fn drain_mixer_commands(&self) -> Vec<MixerCommand> {
        let Some(rx) = self.mixer_rx.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            out.push(cmd);
        }
        out
    }

    /// Execute a single action. Used by the test harness.
    /// Future: the key handler should map keys→actions then call this.
    #[allow(dead_code)]
    pub(crate) fn execute_action(&mut self, action: crate::actions::Action) {
        use crate::actions::Action;

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
                self.toggle_play_pause();
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
                self.edit_loop_range(|l| l.move_start_left());
            }
            Action::LoopStartRight => {
                self.edit_loop_range(|l| l.move_start_right());
            }
            Action::LoopEndLeft => {
                self.edit_loop_range(|l| l.move_end_left());
            }
            Action::LoopEndRight => {
                self.edit_loop_range(|l| l.move_end_right());
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
            Action::Select => {
                if self.nav.fx_menu.open {
                    self.fx_menu_choose();
                } else {
                    self.nav.enter();
                }
            }
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
                self.create_instrument_track_undoable(instrument);
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
            self.refresh_ghost_notes();
            self.tick_practice();
            // Which pattern is playing, and where in it — decided on the
            // audio thread, so it is read back rather than guessed at.
            self.nav.sync_sequencers_from_audio();
            // What was played since the last frame, for a sequencer that is
            // armed. Drained whether or not anything is armed, so the channel
            // cannot grow while a controller is idling.
            self.poll_step_record();
            for track in &self.nav.tracks {
                track.sync_to_audio();
            }

            // Poll for recorded clip snapshots from the audio thread
            let is_recording = self.engine.transport.is_recording();
            let mut committed_while_recording = false;
            while let Ok(snap) = self.clip_rx.try_recv() {
                if let Some((mid, absorbed)) = self.nav.receive_clip_snapshot(snap, is_recording) {
                    // Absorption changed the UI's clip list; rebuild the audio
                    // thread's copy, which still holds the absorbed clips too.
                    if let Some(track_idx) =
                        self.nav.tracks.iter().position(|t| t.mixer_id == Some(mid))
                    {
                        let ui_len = self.nav.tracks[track_idx].clips.len();
                        self.resync_track_clips_to_audio(track_idx, ui_len + absorbed);
                    }
                }
                // A commit emptied the recorder's buffer: the in-flight pass
                // is whatever gets played from here on.
                if is_recording {
                    self.live_take_notes = 0;
                    committed_while_recording = true;
                }
            }
            // Say that a layer landed, and that it is one keypress deep. An
            // invisible take stack is a take stack nobody trusts.
            if committed_while_recording && self.nav.undo_stack.top_is_take() {
                self.flash(format!("take {} · u undoes", self.nav.take_count));
            }

            let snapshot = self.engine.transport.snapshot();

            // Update piano roll dimensions to match terminal size and clip
            let term_size = terminal.size()?;
            let term_h = term_size.height;
            let term_w = term_size.width;
            // How much of a help card is on the screen, so that scrolling it
            // stops where the drawing of it does.
            self.nav.space_menu.set_terminal_rows(term_h);
            // Which way the effect panel's cursor keys point. The panel puts
            // bands in columns when there is room and in rows when there is
            // not, and `h` has to move the cursor the way `h` points either
            // way — so the layout is decided once, here, from the same width
            // the renderer is about to use.
            self.nav.clip_view.fx.wide =
                crate::ui::fx_panel_is_wide((term_w as usize).saturating_sub(25));
            // The tempo, for the panels that have to say what a synced setting
            // means in milliseconds. Read here rather than stored on an edit,
            // so a tempo the player changes from the top bar moves the delay's
            // readout in the same frame.
            self.nav.tempo_bpm = self.engine.transport.tempo_bpm() as f32;
            // Key listen never outlives the panel it was armed from. One rule
            // here rather than a clear on each of the half-dozen ways out of a
            // panel — see `App::enforce_key_listen`.
            self.enforce_key_listen();
            let piano_h = term_h.saturating_sub(30).max(6) as u8;
            self.nav.clip_view.piano_roll.set_view_height(piano_h);

            // Set column count based on actual clip length (beats)
            let ppq = phosphor_core::transport::Transport::PPQ;
            let total_beats = self.nav.active_clip()
                .map(|c| ((c.length_ticks as f64) / ppq as f64).ceil() as usize)
                .unwrap_or(16)
                .max(1);
            self.nav.clip_view.piano_roll.total_beats = total_beats;
            self.nav.clip_view.piano_roll.update_column_count();

            // Set visible columns based on terminal width
            let key_w = 7usize; // key labels + separator
            let fx_panel_w = 25usize; // FX panel + separator
            let note_w = (term_w as usize).saturating_sub(key_w + fx_panel_w);
            let vis_cols = (note_w / 3).max(1).min(self.nav.clip_view.piano_roll.column_count);
            self.nav.clip_view.piano_roll.visible_columns = vis_cols;

            // Log frame details periodically and on first frame after track creation
            if frame_count < 3 || frame_count % 500 == 0 {
                dbg::system(&format!(
                    "frame={frame_count} term={}x{} tracks={} focused={:?} cursor={}",
                    term_w, term_h, self.nav.tracks.len(),
                    self.nav.focused_pane, self.nav.track_cursor,
                ));
            }

            let status = self.live_status();
            terminal.draw(|frame| {
                ui::render(frame, &snapshot, &self.nav, status);
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
    ui_tx: crossbeam_channel::Sender<phosphor_midi::MidiMessage>,
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
        move |_timestamp, data, _| {
            if let Some(mut msg) = phosphor_midi::MidiMessage::from_bytes(data) {
                msg.received_micros = Some(phosphor_midi::clock::now_micros());
                if let phosphor_midi::MidiMessageType::NoteOn { note, .. } = msg.message_type {
                    status_clone.last_note.store(note, Ordering::Relaxed);
                }
                status_clone.message_count.fetch_add(1, Ordering::Relaxed);
                midi_tx.push(msg);
                // The UI's copy, for step record. A send that fails means
                // nothing is listening, which is not a reason to stop playing.
                let _ = ui_tx.send(msg);
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

#[cfg(test)]
mod tests {
    use super::*;
    use phosphor_core::cpal_backend::Requested;

    /// A device sitting at 48000 with blocks of its own choosing.
    fn following(sample_rate: u32) -> StreamFormat {
        StreamFormat {
            sample_rate,
            buffer_size: None,
            max_buffer_frames: 4096,
            channels: 2,
            sample_rate_request: Requested::Unasked,
            buffer_size_request: Requested::Unasked,
        }
    }

    /// A device answering a request, one way or the other.
    fn answering(sample_rate: u32, rate: Requested, block: Requested, pinned: u32) -> StreamFormat {
        StreamFormat {
            sample_rate,
            buffer_size: Some(pinned),
            max_buffer_frames: 4096,
            channels: 2,
            sample_rate_request: rate,
            buffer_size_request: block,
        }
    }

    /// The default: nothing on the command line, so the engine is built at
    /// whatever the device was already set to and the device is left alone.
    #[test]
    fn asking_for_nothing_runs_at_the_devices_rate() {
        let resolved = AudioFormat::resolve(AudioRequest::follow_device(), Some(following(48000)));
        assert_eq!(resolved.config.sample_rate, 48000);
        assert_ne!(
            resolved.config.sample_rate,
            EngineConfig::default().sample_rate,
            "the no-device fallback must not leak into the path where there is a device"
        );
    }

    /// ...and says nothing about it. Following the device is the ordinary
    /// case; a line on every launch is a line nobody reads.
    #[test]
    fn following_the_device_is_silent() {
        let resolved = AudioFormat::resolve(AudioRequest::follow_device(), Some(following(48000)));
        assert!(resolved.notice.is_none());
    }

    /// The defect, in one assertion: the engine was built from the command
    /// line while the stream ran at the device's rate. 44100 into a 48000
    /// stream is every note 1.47 semitones sharp and 120 BPM playing at 130.6.
    #[test]
    fn a_device_that_disagrees_decides_the_rate() {
        let request = AudioRequest { sample_rate: Some(22050), buffer_size: None };
        let resolved = AudioFormat::resolve(
            request,
            Some(answering(48000, Requested::Refused(22050), Requested::Unasked, 64)),
        );
        assert_eq!(resolved.config.sample_rate, 48000);
        assert_ne!(resolved.config.sample_rate, request.sample_rate.unwrap());
    }

    #[test]
    fn a_device_that_agrees_gives_what_was_asked_for() {
        let request = AudioRequest { sample_rate: Some(44100), buffer_size: Some(64) };
        let resolved = AudioFormat::resolve(
            request,
            Some(answering(44100, Requested::Granted, Requested::Granted, 64)),
        );
        assert_eq!(resolved.config, EngineConfig { buffer_size: 64, sample_rate: 44100 });
        assert!(resolved.notice.is_none(), "nothing to report when the request stood");
    }

    #[test]
    fn a_device_that_disagrees_says_so() {
        let resolved = AudioFormat::resolve(
            AudioRequest { sample_rate: Some(22050), buffer_size: None },
            Some(answering(48000, Requested::Refused(22050), Requested::Unasked, 64)),
        );
        let notice = resolved.notice.expect("a silent divergence is the bug");
        assert!(notice.contains("48000"), "{notice}");
        assert!(notice.contains("22050"), "{notice}");
    }

    #[test]
    fn a_clamped_block_size_is_reported_too() {
        let resolved = AudioFormat::resolve(
            AudioRequest { sample_rate: None, buffer_size: Some(4) },
            Some(answering(48000, Requested::Unasked, Requested::Refused(4), 15)),
        );
        assert_eq!(resolved.config.buffer_size, 15);
        assert!(resolved.notice.is_some());
    }

    /// Buffers are sized for the largest block the device admits to, not for
    /// the one we asked for — a bigger block arriving means `Mixer::process`
    /// grows a Vec on the audio thread.
    #[test]
    fn buffers_are_sized_for_the_devices_largest_block() {
        let resolved = AudioFormat::resolve(AudioRequest::follow_device(), Some(following(48000)));
        assert_eq!(resolved.max_buffer_frames, 4096);
    }

    /// `--no-audio`, and the backend that would not open, both land here.
    #[test]
    fn without_a_device_the_defaults_stand() {
        let resolved = AudioFormat::resolve(AudioRequest::follow_device(), None);
        assert_eq!(resolved.config, EngineConfig::default());
        assert_eq!(resolved.max_buffer_frames, EngineConfig::default().buffer_size as usize);
        assert!(resolved.notice.is_none());
    }

    /// ...but a request made without a device is still honoured, since there
    /// is nothing to overrule it.
    #[test]
    fn without_a_device_what_was_asked_for_still_stands() {
        let resolved = AudioFormat::resolve(
            AudioRequest { sample_rate: Some(96000), buffer_size: Some(256) },
            None,
        );
        assert_eq!(resolved.config, EngineConfig { buffer_size: 256, sample_rate: 96000 });
    }

    /// The same thing end to end: a headless app opens no device, so it runs
    /// at the no-device numbers.
    #[test]
    fn a_headless_app_runs_at_the_no_device_defaults() {
        let app = App::new(AudioRequest::follow_device(), false, false);
        assert_eq!(app.engine.config, EngineConfig::default());
        assert!(
            app.live_status().is_none(),
            "no device means no format to complain about"
        );
    }

    /// The test apps hand `App::new` concrete numbers rather than a request,
    /// and with no device those numbers are what it runs at.
    #[test]
    fn a_headless_app_honours_concrete_numbers() {
        for config in [
            EngineConfig { buffer_size: 64, sample_rate: 44100 },
            EngineConfig { buffer_size: 256, sample_rate: 96000 },
        ] {
            let app = App::new(config, false, false);
            assert_eq!(app.engine.config, config);
            assert!(app.live_status().is_none());
        }
    }
}
