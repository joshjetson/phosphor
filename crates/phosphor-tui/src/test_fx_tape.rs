//! The tape, end to end: menu to mixer to loudspeaker, and the panel behind
//! its slot.
//!
//! The medium is tested where it lives, against its own harmonic signature
//! and its own transport, in `phosphor_dsp::fx::tape`; the adapter that puts
//! it in a slot is tested in `phosphor_app::fx::tape`. What is under test
//! here is everything between the two — that choosing "tape" in the menu
//! builds one, that it lands at its canonical place in the chain, that the
//! command reaches the audio thread, that a note played into a track with one
//! on it comes back *magnetised*, that the panel draws what the effect is
//! doing rather than what its knobs hold, and that all of it survives being
//! written to a file and read back.
//!
//! **The transport is stopped for every measurement of the medium**, and it
//! has to be: the factory wow is 0.1% at 0.6 Hz, which moves a 1 kHz tone by
//! a hertz either way over a cycle a second and a half long, and a DFT bin
//! measured over a fraction of that reads whichever part of the wow cycle it
//! happened to catch. That is not a defect in the tape; it is a defect in
//! measuring a wobbling tone with a stationary ruler.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use phosphor_app::state::{FxType, InstrumentType};
    use phosphor_core::clip::ClipSnapshot;
    use phosphor_core::engine::VuLevels;
    use phosphor_core::fx::{FxTarget, SendSlot};
    use phosphor_core::mixer::{clip_snapshot_channel, Mixer, MixerCommand};
    use phosphor_core::project::TrackKind;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::fx::tape::{
        Speed, PARAM_AUTO_MAKEUP, PARAM_AZIMUTH_DEG, PARAM_BIAS, PARAM_BUMP_DB, PARAM_COUNT,
        PARAM_DRIVE, PARAM_FLUTTER, PARAM_HISS, PARAM_MIX, PARAM_SAT, PARAM_SPEED, PARAM_TRIM_DB,
        PARAM_WOW,
    };
    use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

    use crate::app::App;

    const SAMPLE_RATE: u32 = 44_100;
    const FS: f64 = SAMPLE_RATE as f64;
    const BLOCK: usize = 64;
    const MAX_BLOCK: usize = 256;

    /// How loud the source is. −12 dBFS, which is where this box's
    /// instruments are gain-staged and where the tape's calibration puts its
    /// distortion at about a percent.
    const AMPLITUDE: f32 = 0.25;

    // ── A source that does not stop ──

    /// A 1 kHz sine, forever, from a sample counter rather than an
    /// accumulator so that two renders of the same length are the same
    /// samples however the buffer is cut up.
    struct Tone {
        sample_rate: f64,
        n: usize,
        hz: f64,
    }

    impl Tone {
        fn new(hz: f64) -> Self {
            Self { sample_rate: FS, n: 0, hz }
        }
    }

    impl Plugin for Tone {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "tone".into(),
                version: "1".into(),
                author: String::new(),
                category: PluginCategory::Utility,
            }
        }
        fn init(&mut self, sample_rate: f64, _max_buffer_size: usize) {
            self.sample_rate = sample_rate;
            self.n = 0;
        }
        fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]], _midi: &[MidiEvent]) {
            let frames = outputs.first().map_or(0, |o| o.len());
            for i in 0..frames {
                let value = AMPLITUDE
                    * (std::f64::consts::TAU * self.hz * self.n as f64 / self.sample_rate).sin()
                        as f32;
                for out in outputs.iter_mut() {
                    out[i] = value;
                }
                self.n += 1;
            }
        }
        fn parameter_count(&self) -> usize {
            0
        }
        fn parameter_info(&self, _index: usize) -> Option<ParameterInfo> {
            None
        }
        fn get_parameter(&self, _index: usize) -> f32 {
            0.0
        }
        fn set_parameter(&mut self, _index: usize, _value: f32) {}
        fn reset(&mut self) {
            self.n = 0;
        }
    }

    // ── The rig ──

    struct Rig {
        app: App,
        mixer: Mixer,
        transport: Arc<Transport>,
        _clip_rx: crossbeam_channel::Receiver<ClipSnapshot>,
    }

    impl Rig {
        fn new() -> Self {
            let app = App::new(
                EngineConfig { buffer_size: BLOCK as u32, sample_rate: SAMPLE_RATE },
                false,
                false,
            );
            let rx = app.mixer_rx.clone().expect("a headless app keeps the command receiver");
            let (clip_tx, clip_rx) = clip_snapshot_channel();
            let mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, SAMPLE_RATE, MAX_BLOCK);
            let transport = app.engine.transport.clone();
            Self { app, mixer, transport, _clip_rx: clip_rx }
        }

        fn with_tone() -> Self {
            let mut rig = Self::new();
            rig.app.create_instrument_track(InstrumentType::Synth);
            let track_id = rig.app.nav.tracks[rig.app.nav.track_cursor]
                .mixer_id
                .expect("an instrument track has an id");
            let _ = rig.app.engine.shared.mixer_command_tx.send(MixerCommand::SetInstrument {
                track_id,
                instrument: Box::new(Tone::new(1000.0)),
            });
            // Neither send bus is what these tests are about.
            rig.clear_bus(TrackKind::SendA);
            rig.clear_bus(TrackKind::SendB);
            rig
        }

        fn track_index(&self, kind: TrackKind) -> usize {
            self.app
                .nav
                .tracks
                .iter()
                .position(|t| t.kind == kind)
                .expect("the strip exists")
        }

        fn clear_bus(&mut self, kind: TrackKind) {
            let bus = self.track_index(kind);
            self.app.clear_chain(bus);
        }

        /// Put a tape on a strip through the path a keypress takes, with its
        /// transport stopped so that the medium can be measured, and answer
        /// the slot it landed in.
        fn add_tape(&mut self, track_index: usize) -> usize {
            self.app.nav.track_cursor = track_index;
            let outcome = self.app.nav.add_fx(FxType::Tape);
            let slot = match &outcome {
                phosphor_app::state::FxAdd::Added { slot, .. } => *slot,
                phosphor_app::state::FxAdd::NotBuilt(_) => panic!("the tape is not registered"),
                phosphor_app::state::FxAdd::ChainFull => panic!("the chain was full"),
                phosphor_app::state::FxAdd::Nothing => panic!("no strip under the cursor"),
            };
            self.app.apply_fx_add(outcome);
            self.app.set_fx_param(track_index, slot, PARAM_WOW, 0.0);
            self.app.set_fx_param(track_index, slot, PARAM_FLUTTER, 0.0);
            slot
        }

        fn settle_commands(&mut self) {
            for _ in 0..1000 {
                if self.app.mixer_rx.as_ref().is_some_and(crossbeam_channel::Receiver::is_empty) {
                    return;
                }
                self.mixer.process(&mut [], &[], &self.transport);
            }
            panic!("the command queue never drained");
        }

        fn render(&mut self, blocks: usize) -> Vec<f32> {
            let mut out = vec![0.0f32; BLOCK * 2];
            let mut all = Vec::with_capacity(BLOCK * 2 * blocks);
            for _ in 0..blocks {
                self.mixer.process(&mut out, &[], &self.transport);
                all.extend_from_slice(&out);
            }
            all
        }

        /// Half a second of it, after the smoothers have arrived.
        fn full_render(&mut self) -> Vec<f32> {
            self.settle_commands();
            let _ = self.render(200);
            self.render(400)
        }
    }

    // ── Measurement ──

    /// The amplitude of one frequency in the left channel of an interleaved
    /// render, by a single Hann-windowed DFT bin.
    fn bin(render: &[f32], hz: f64) -> f64 {
        let frames = render.len() / 2;
        let (mut re, mut im, mut norm) = (0.0f64, 0.0, 0.0);
        for i in 0..frames {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / frames as f64).cos();
            let p = std::f64::consts::TAU * hz * i as f64 / FS;
            let v = f64::from(render[i * 2]) * w;
            re += v * p.cos();
            im -= v * p.sin();
            norm += w;
        }
        2.0 * (re * re + im * im).sqrt() / norm
    }

    fn rms(render: &[f32]) -> f64 {
        let frames = render.len() / 2;
        (0..frames).map(|i| f64::from(render[i * 2]).powi(2)).sum::<f64>().sqrt()
            / (frames as f64).sqrt()
    }

    #[track_caller]
    fn assert_bit_identical(with: &[f32], without: &[f32], what: &str) {
        assert_eq!(with.len(), without.len(), "{what}: renders of different lengths");
        assert!(
            without.iter().any(|s| s.abs() > 0.001),
            "{what}: the reference render was silent, so identity proves nothing"
        );
        for (i, (a, b)) in without.iter().zip(with).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "{what}: sample {i} became {b} from {a}");
        }
    }

    /// The terminal, as text.
    fn screen(app: &App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        let status = app.live_status();
        terminal.draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_event(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }));
    }

    // ── The acceptance ──

    /// **A tape on a track magnetises the note.**
    ///
    /// Through the whole path: the menu builds the effect, the command
    /// carries it to the mixer, `Effect::init` designs the record EQ for the
    /// device's rate, and a 1 kHz tone comes back with a third harmonic, no
    /// second harmonic, and the level it went in with.
    #[test]
    fn a_tape_on_a_track_magnetises_the_note() {
        let mut dry = Rig::with_tone();
        let bare = dry.full_render();
        let clean_h3 = bin(&bare, 3000.0) / bin(&bare, 1000.0);

        let mut wet = Rig::with_tone();
        let track = wet.app.nav.track_cursor;
        let slot = wet.add_tape(track);
        assert_eq!(wet.app.nav.tracks[track].fx_target(), Some(FxTarget::Track(0)));
        assert_eq!(wet.app.nav.tracks[track].fx_chain[slot].params.len(), PARAM_COUNT);
        let taped = wet.full_render();

        let fundamental = bin(&taped, 1000.0);
        let h3 = bin(&taped, 3000.0) / fundamental;
        let h2 = bin(&taped, 2000.0) / fundamental;
        assert!(h3 > clean_h3 * 20.0, "the tape added no third harmonic: {h3:.5}");
        assert!(h3 > 0.002, "the third harmonic was only {h3:.5} of the fundamental");
        assert!(h2 < h3 / 20.0, "a second harmonic appeared: {h2:.6}");

        // And the level held: the automatic makeup is what makes an A/B
        // honest, and a tape that is a decibel louder always sounds better.
        let level = 20.0 * (rms(&taped) / rms(&bare)).log10();
        assert!(level.abs() < 1.0, "the tape moved the level by {level:+.2} dB");
    }

    /// **The chain is the same feature on every strip.** A tape on the master
    /// saturates the whole mix; one on a bus saturates the return.
    #[test]
    fn a_tape_works_on_the_master_and_on_a_bus() {
        let mut rig = Rig::with_tone();
        let master = rig.track_index(TrackKind::Master);
        let slot = rig.add_tape(master);
        assert_eq!(rig.app.nav.tracks[master].fx_target(), Some(FxTarget::Master));
        rig.app.set_fx_param(master, slot, PARAM_DRIVE, 100.0);
        let out = rig.full_render();
        assert!(
            bin(&out, 3000.0) / bin(&out, 1000.0) > 0.005,
            "a tape on the master did nothing"
        );

        let mut rig = Rig::with_tone();
        let bus = rig.track_index(TrackKind::SendB);
        let slot = rig.add_tape(bus);
        assert_eq!(rig.app.nav.tracks[bus].fx_target(), Some(FxTarget::BusB));
        rig.app.set_fx_param(bus, slot, PARAM_DRIVE, 100.0);
        let track = 0;
        rig.app.nav.tracks[track].set_send_db(SendSlot::B, 0.0);
        rig.app.sync_routing(track);
        let out = rig.full_render();
        assert!(
            bin(&out, 3000.0) / bin(&out, 1000.0) > 0.002,
            "a tape on the send bus did nothing with the send open"
        );
    }

    /// A tape nobody has turned up is inaudible, sample for sample. Adding
    /// one to a chain while the transport is rolling must not change the mix.
    #[test]
    fn a_tape_at_wet_zero_is_bit_identical_to_no_chain() {
        let mut bare = Rig::with_tone();
        let reference = bare.full_render();

        let mut wet = Rig::with_tone();
        let track = wet.app.nav.track_cursor;
        let slot = wet.add_tape(track);
        wet.app.set_fx_param(track, slot, PARAM_MIX, 0.0);
        wet.app.set_fx_param(track, slot, PARAM_DRIVE, 100.0);
        wet.app.set_fx_param(track, slot, PARAM_HISS, 100.0);
        assert_bit_identical(&wet.full_render(), &reference, "a tape at wet zero");
    }

    /// Bypassing one restores the dry signal exactly, once the switch's own
    /// crossfade has finished.
    #[test]
    fn bypassing_a_tape_restores_the_dry_signal_exactly() {
        let mut bare = Rig::with_tone();
        let reference = bare.full_render();

        let mut wet = Rig::with_tone();
        let track = wet.app.nav.track_cursor;
        let slot = wet.add_tape(track);
        wet.app.set_fx_bypass(track, slot, true);
        assert_bit_identical(&wet.full_render(), &reference, "a bypassed tape");
    }

    /// **The menu adds a working tape, at its canonical place in the chain.**
    ///
    /// The canonical order is `EQ → comp → tape → delay → reverb`, so a tape
    /// added to a chain that already has a delay in it lands *before* the
    /// delay, and the delay does not move.
    #[test]
    fn the_fx_menu_adds_a_working_tape() {
        let mut rig = Rig::with_tone();
        let track = rig.app.nav.track_cursor;
        let outcome = rig.app.nav.add_fx(FxType::Delay);
        rig.app.apply_fx_add(outcome);
        let _ = rig.app.drain_mixer_commands();

        rig.app.nav.fx_menu.open = true;
        rig.app.nav.fx_menu.cursor =
            FxType::ALL.iter().position(|f| *f == FxType::Tape).expect("the tape is in the menu");
        rig.app.fx_menu_choose();

        assert!(!rig.app.nav.fx_menu.open, "the menu stayed open");
        let chain = &rig.app.nav.tracks[track].fx_chain;
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].fx_type, FxType::Tape, "the tape did not land before the delay");
        assert_eq!(chain[1].fx_type, FxType::Delay, "the delay moved");
        assert!(chain[0].is_active(), "a new effect arrived bypassed");
        assert_eq!(chain[0].params.len(), PARAM_COUNT);
        let status = rig.app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("tape added at slot 1"), "the status bar said {status:?}");

        let commands = rig.app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::AddFx { target: FxTarget::Track(0), slot: 0, effect }
                    if effect.name() == "tape"
            )),
            "the tape never reached the mixer"
        );
    }

    /// **A session brings the tape back, every control of it.**
    #[test]
    fn a_session_restores_a_tape_with_every_control_moved() {
        let dir = std::env::temp_dir().join(format!("phosphor-tape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tape.phos");
        let path_str = path.to_string_lossy().to_string();

        let mut saving = Rig::with_tone();
        let track = saving.app.nav.track_cursor;
        let slot = saving.add_tape(track);
        let moved = [
            (PARAM_SPEED, Speed::Slow.index() as f32),
            (PARAM_DRIVE, 77.0),
            (PARAM_SAT, 23.0),
            (PARAM_BIAS, 68.0),
            (PARAM_WOW, 31.0),
            (PARAM_FLUTTER, 82.0),
            (PARAM_BUMP_DB, 2.7),
            (PARAM_AZIMUTH_DEG, 0.35),
            (PARAM_HISS, 45.0),
            (PARAM_TRIM_DB, -4.5),
            (PARAM_AUTO_MAKEUP, 0.0),
            (PARAM_MIX, 66.0),
        ];
        assert_eq!(moved.len(), PARAM_COUNT, "a control was left out of the round trip");
        for (index, value) in moved {
            saving.app.set_fx_param(track, slot, index, value);
        }
        let params = saving.app.nav.tracks[track].fx_chain[slot].params.clone();
        saving.app.do_save(&path_str);

        let mut reopened = Rig::new();
        reopened.app.do_load(&path_str);
        let track = reopened
            .app
            .nav
            .tracks
            .iter()
            .position(|t| t.instrument_type.is_some())
            .expect("the session had an instrument track");
        let chain = &reopened.app.nav.tracks[track].fx_chain;
        assert_eq!(chain.len(), 1, "the chain did not come back");
        assert_eq!(chain[0].fx_type, FxType::Tape);
        assert_eq!(chain[0].params.len(), PARAM_COUNT);
        assert_eq!(chain[0].params, params, "a control came back as a different number");
        for (index, value) in moved {
            assert_eq!(chain[0].params[index], value, "index {index}");
        }

        // And it is the same tape in the signal path, not just in the mirror:
        // two independent loads of the same file render the same samples.
        let with_tone = |rig: &mut Rig| {
            let id = rig.app.nav.tracks[track].mixer_id.expect("an id");
            let _ = rig.app.engine.shared.mixer_command_tx.send(MixerCommand::SetInstrument {
                track_id: id,
                instrument: Box::new(Tone::new(1000.0)),
            });
        };
        with_tone(&mut reopened);
        let first = reopened.full_render();
        let mut again = Rig::new();
        again.app.do_load(&path_str);
        with_tone(&mut again);
        assert_bit_identical(&again.full_render(), &first, "two loads of one session");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A mix full of tapes never reaches the allocator.
    #[test]
    fn a_mix_full_of_tapes_never_reaches_the_allocator() {
        let mut rig = Rig::with_tone();
        for _ in 0..2 {
            rig.app.create_instrument_track(InstrumentType::Synth);
        }
        for index in 0..rig.app.nav.tracks.len() {
            if rig.app.nav.tracks[index].fx_target().is_some()
                && rig.app.nav.tracks[index].fx_chain.is_empty()
            {
                rig.add_tape(index);
            }
        }
        rig.settle_commands();
        let _ = rig.render(8);

        let mut out = vec![0.0f32; BLOCK * 2];
        let allocations = crate::test_fx_eq::tests::alloc_count::allocations_during(|| {
            for _ in 0..200 {
                rig.mixer.process(&mut out, &[], &rig.transport);
            }
        });
        assert_eq!(
            allocations, 0,
            "the audio callback allocated {allocations} times with tapes in the chain"
        );
    }

    // ── The panel ──

    /// **The panel draws its twelve rows, and it draws what they mean.**
    ///
    /// Not the knob positions: the wow row reads the deviation it is asking
    /// for, the bump row reads the frequency the speed puts it at, and the
    /// makeup row reads the gain it decided on. A panel that says "wow 50%"
    /// has told a player nothing.
    #[test]
    fn the_tape_panel_draws_what_its_controls_mean() {
        let mut rig = Rig::with_tone();
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_tape(track);
        rig.app.set_fx_param(track, slot, PARAM_WOW, 50.0);
        rig.app.nav.clip_view.fx.open(slot);
        rig.app.nav.clip_view.clip_tab = crate::state::ClipTab::Fx;
        rig.app.nav.clip_view.focus = crate::state::ClipViewFocus::PianoRoll;
        rig.app.nav.focused_pane = crate::state::Pane::ClipView;

        let text = screen(&rig.app, 140, 44);
        assert!(text.contains("tap"), "the panel does not name itself:\n{text}");
        for name in [
            "speed", "drive", "sat", "bias", "wow", "flutr", "bump", "azimth", "hiss", "trim",
            "mkauto", "mix",
        ] {
            assert!(text.contains(name), "the panel is missing `{name}`:\n{text}");
        }
        assert!(text.contains("15 ips"), "the speed does not read in inches per second:\n{text}");
        assert!(text.contains("0.10%"), "the wow does not read as a deviation:\n{text}");
        assert!(text.contains("70"), "the bump does not say where it is:\n{text}");
        assert!(text.contains("auto"), "the automatic makeup does not read:\n{text}");
        assert!(text.contains("true"), "an azimuth of zero does not read as true:\n{text}");
        assert!(text.contains("off"), "the hiss does not read as off:\n{text}");

        // And on the makeup row, what "level-matched" means — because it
        // means matched on music, and a player who checks it with a sine
        // will find the tape a decibel down and think it is broken.
        rig.app.nav.clip_view.fx.band = PARAM_AUTO_MAKEUP;
        let text = screen(&rig.app, 140, 44);
        assert!(
            text.contains("matched on programme"),
            "the makeup row does not say what it matched:\n{text}"
        );
    }

    /// **The panel's keys walk and turn**, in the house grammar: `j`/`k`
    /// picks, `h`/`l` turns, and a control that moves says so on the strip.
    #[test]
    fn the_panel_keys_walk_and_turn_the_tape() {
        let mut rig = Rig::with_tone();
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_tape(track);
        rig.app.nav.clip_view.fx.open(slot);
        rig.app.nav.clip_view.clip_tab = crate::state::ClipTab::Fx;
        rig.app.nav.clip_view.focus = crate::state::ClipViewFocus::PianoRoll;
        rig.app.nav.focused_pane = crate::state::Pane::ClipView;

        // The cursor starts on the speed, and `l` moves it up a step.
        assert_eq!(rig.app.nav.clip_view.fx.band, 0);
        press(&mut rig.app, KeyCode::Char('l'));
        assert_eq!(
            rig.app.nav.tracks[track].fx_chain[slot].params[PARAM_SPEED],
            Speed::Fast.index() as f32,
            "l did not change the tape speed"
        );
        press(&mut rig.app, KeyCode::Char('h'));
        assert_eq!(
            rig.app.nav.tracks[track].fx_chain[slot].params[PARAM_SPEED],
            Speed::Studio.index() as f32
        );

        // Down to the drive, and a stride is ten points where a press is one.
        press(&mut rig.app, KeyCode::Char('j'));
        assert_eq!(rig.app.nav.clip_view.fx.band, PARAM_DRIVE);
        press(&mut rig.app, KeyCode::Char('l'));
        assert_eq!(rig.app.nav.tracks[track].fx_chain[slot].params[PARAM_DRIVE], 51.0);
        press(&mut rig.app, KeyCode::Char('L'));
        assert_eq!(rig.app.nav.tracks[track].fx_chain[slot].params[PARAM_DRIVE], 61.0);

        // The audio thread heard about both of them.
        let commands = rig.app.drain_mixer_commands();
        assert!(
            commands
                .iter()
                .filter(|c| matches!(
                    c,
                    MixerCommand::SetFxParam { param, .. } if *param == PARAM_DRIVE
                ))
                .count()
                >= 2,
            "the drive changes never reached the mixer"
        );

        // `enter` holds the control so that `j`/`k` stop walking off it.
        press(&mut rig.app, KeyCode::Enter);
        assert!(rig.app.nav.clip_view.fx.locked);
        press(&mut rig.app, KeyCode::Char('j'));
        assert_eq!(rig.app.nav.clip_view.fx.band, PARAM_DRIVE, "held, j walked the cursor");
        press(&mut rig.app, KeyCode::Esc);
        assert!(!rig.app.nav.clip_view.fx.locked);
    }

    /// **The trim is greyed while the makeup is automatic, and turning it
    /// takes the makeup back where the automatic had it** — so a player who
    /// wants the level never has to guess at it, and taking control never
    /// moves it.
    #[test]
    fn the_trim_is_greyed_until_you_turn_it_and_then_it_is_yours() {
        let mut rig = Rig::with_tone();
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_tape(track);
        rig.app.nav.clip_view.fx.open(slot);
        rig.app.nav.clip_view.clip_tab = crate::state::ClipTab::Fx;
        rig.app.nav.clip_view.focus = crate::state::ClipViewFocus::PianoRoll;
        rig.app.nav.focused_pane = crate::state::Pane::ClipView;

        let params = rig.app.nav.tracks[track].fx_chain[slot].params.clone();
        assert!(
            !phosphor_dsp::fx::tape::uses(&params, PARAM_TRIM_DB),
            "the trim is live while the automatic has it"
        );
        let automatic = phosphor_dsp::fx::tape::auto_makeup_db(&params) as f32;

        rig.app.nav.clip_view.fx.band = PARAM_TRIM_DB;
        let text = screen(&rig.app, 140, 44);
        assert!(
            text.contains("the makeup is automatic"),
            "the panel does not say why the trim is greyed:\n{text}"
        );

        press(&mut rig.app, KeyCode::Char('l'));
        let params = rig.app.nav.tracks[track].fx_chain[slot].params.clone();
        assert_eq!(params[PARAM_AUTO_MAKEUP], 0.0, "the automatic kept the makeup");
        assert!(
            (params[PARAM_TRIM_DB] - automatic).abs() < 0.05,
            "the trim came back at {} rather than at the automatic's {automatic}",
            params[PARAM_TRIM_DB]
        );
        assert!(phosphor_dsp::fx::tape::uses(&params, PARAM_TRIM_DB), "the trim stayed greyed");
        let status = rig.app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("output is yours"), "the status bar said {status:?}");

        // ...and now it turns, in half-decibel steps.
        press(&mut rig.app, KeyCode::Char('l'));
        let after = rig.app.nav.tracks[track].fx_chain[slot].params[PARAM_TRIM_DB];
        assert!((after - params[PARAM_TRIM_DB] - 0.5).abs() < 1.0e-4, "the trim moved to {after}");
    }
}
