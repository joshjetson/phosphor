//! The delay, end to end: menu to mixer to loudspeaker, and the send bus a
//! new session ships with.
//!
//! The line is tested where it lives, against the grid and against the
//! feedback bound, in `phosphor_dsp::fx::delay`; the adapter that puts it in a
//! slot is tested in `phosphor_app::fx::delay`. What is under test here is
//! everything between the two — that choosing "delay" in the menu builds one,
//! that the command reaches the audio thread, that a note played into a track
//! with one on it comes back *on the transport's grid*, that turning Send B up
//! on a new session is audible without any routing work, and that all of it
//! survives being written to a file and read back.
//!
//! **The tempo is the whole point of the rig being the real one.** A synced
//! delay reads the transport out of [`phosphor_core::fx::FxContext`], once a
//! block, inside the mixer's own `process` — so the only way to check that the
//! echo lands on the beat is to run the mixer with a transport at a tempo and
//! measure where the echo landed.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use phosphor_app::state::{FxType, InstrumentType};
    use phosphor_core::clip::ClipSnapshot;
    use phosphor_core::engine::VuLevels;
    use phosphor_core::fx::{FxTarget, SendSlot};
    use phosphor_core::mixer::{clip_snapshot_channel, Mixer, MixerCommand};
    use phosphor_core::project::TrackKind;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::fx::delay::{
        synced_seconds, Mode, Routing, HEAD_LABELS, SYNC_DEFAULT, SYNC_LABELS, PARAM_COUNT,
        PARAM_DIVISION, PARAM_FEEDBACK, PARAM_HEADS, PARAM_HIGH_CUT_HZ, PARAM_LOW_CUT_HZ,
        PARAM_MIX, PARAM_MODE, PARAM_ROUTING, PARAM_SYNC, PARAM_TIME_MS,
    };
    use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

    use crate::app::App;

    const SAMPLE_RATE: u32 = 44_100;
    const FS: f64 = SAMPLE_RATE as f64;
    const BLOCK: usize = 64;
    const MAX_BLOCK: usize = 256;

    /// How long the source plays for, in frames — about a twentieth of a
    /// second, short enough that an echo at any of the tested tempos lands in
    /// silence rather than on top of the source.
    const BURST_FRAMES: usize = 2_048;
    /// How loud it is. Low, so that nothing measured here is the master
    /// limiter's opinion rather than the delay's.
    const AMPLITUDE: f32 = 0.15;
    /// Long enough for the wet/dry smoother to arrive at exactly zero: it is a
    /// 15 ms one-pole walking down from 22%, and it snaps the last millionth,
    /// which takes about 190 ms.
    const SETTLE_BLOCKS: usize = 400;

    // ── A source that stops ──

    /// A burst of a 660 Hz sine, then silence, forever.
    ///
    /// The phase comes from a sample counter rather than an accumulator so
    /// that two renders of the same length are the same samples however the
    /// buffer is cut up.
    struct Burst {
        sample_rate: f64,
        n: usize,
        frames: usize,
    }

    impl Burst {
        fn new() -> Self {
            Self { sample_rate: FS, n: 0, frames: BURST_FRAMES }
        }

        fn continuous() -> Self {
            Self { sample_rate: FS, n: 0, frames: usize::MAX }
        }
    }

    impl Plugin for Burst {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "burst".into(),
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
                let value = if self.n < self.frames {
                    AMPLITUDE
                        * (std::f64::consts::TAU * 660.0 * self.n as f64 / self.sample_rate).sin()
                            as f32
                } else {
                    0.0
                };
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
            let rx = app
                .mixer_rx
                .clone()
                .expect("a headless app keeps the command receiver");
            let (clip_tx, clip_rx) = clip_snapshot_channel();
            let mixer = Mixer::new(rx, Arc::new(VuLevels::new()), clip_tx, SAMPLE_RATE, MAX_BLOCK);
            let transport = app.engine.transport.clone();
            Self { app, mixer, transport, _clip_rx: clip_rx }
        }

        fn with_burst() -> Self {
            Self::with_source(Burst::new())
        }

        fn with_tone() -> Self {
            Self::with_source(Burst::continuous())
        }

        fn with_source(source: Burst) -> Self {
            let mut rig = Self::new();
            rig.app.create_instrument_track(InstrumentType::Synth);
            let track_id = rig.app.nav.tracks[rig.app.nav.track_cursor]
                .mixer_id
                .expect("an instrument track has an id");
            let _ = rig
                .app
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::SetInstrument { track_id, instrument: Box::new(source) });
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

        /// Put a delay on a strip through the path a keypress takes, and
        /// answer with the slot it landed in.
        fn add_delay(&mut self, track_index: usize) -> usize {
            self.app.nav.track_cursor = track_index;
            let outcome = self.app.nav.add_fx(FxType::Delay);
            let slot = match &outcome {
                phosphor_app::state::FxAdd::Added { slot, .. } => *slot,
                phosphor_app::state::FxAdd::NotBuilt(_) => panic!("the delay is not registered"),
                phosphor_app::state::FxAdd::ChainFull => panic!("the chain was full"),
                phosphor_app::state::FxAdd::Nothing => panic!("no strip under the cursor"),
            };
            self.app.apply_fx_add(outcome);
            slot
        }

        fn settle_commands(&mut self) {
            for _ in 0..1000 {
                if self
                    .app
                    .mixer_rx
                    .as_ref()
                    .is_some_and(crossbeam_channel::Receiver::is_empty)
                {
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

        /// The burst and three seconds after it, interleaved.
        fn full_render(&mut self) -> Vec<f32> {
            self.settle_commands();
            self.render(2_200)
        }

        /// Empty the send bus a test is not about, so that what is measured is
        /// the effect under test.
        fn clear_bus(&mut self, kind: TrackKind) {
            let bus = self.track_index(kind);
            self.app.clear_chain(bus);
        }
    }

    // ── Measurement ──

    fn window_peak(render: &[f32], from: usize, to: usize) -> f64 {
        let frames = render.len() / 2;
        let to = to.min(frames);
        if to <= from {
            return 0.0;
        }
        (from..to)
            .map(|i| f64::from(render[i * 2].abs()))
            .fold(0.0, f64::max)
    }

    /// The frame the loudest sample of the left channel sits at, inside a
    /// window.
    fn argmax(render: &[f32], from: usize, to: usize) -> usize {
        let frames = render.len() / 2;
        let to = to.min(frames);
        let mut best = from.min(frames.saturating_sub(1));
        for i in from..to {
            if render[i * 2].abs() > render[best * 2].abs() {
                best = i;
            }
        }
        best
    }

    /// What is left after the source has stopped, and after the first echo has
    /// had time to arrive.
    fn tail_peak(render: &[f32]) -> f64 {
        window_peak(render, BURST_FRAMES + 512, BURST_FRAMES + 60_000)
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

    // ── The acceptance ──

    /// **A delay on a track repeats the note, on the transport's grid.**
    ///
    /// Through the whole path: the menu builds the effect, the command carries
    /// it to the mixer, `Effect::init` sizes the line for the device's rate,
    /// and the echo of a burst that stopped lands a dotted eighth later — at
    /// whatever tempo the transport happens to be running.
    #[test]
    fn a_delay_on_a_track_repeats_on_the_transports_grid() {
        for bpm in [90.0f64, 120.0, 174.0] {
            let mut dry = Rig::with_burst();
            dry.clear_bus(TrackKind::SendA);
            dry.clear_bus(TrackKind::SendB);
            dry.transport.set_tempo(bpm);
            let bare = dry.full_render();
            assert!(tail_peak(&bare) < 1.0e-6, "{bpm}: the dry render already repeated");

            let mut wet = Rig::with_burst();
            wet.clear_bus(TrackKind::SendA);
            wet.clear_bus(TrackKind::SendB);
            wet.transport.set_tempo(bpm);
            let track = wet.app.nav.track_cursor;
            let slot = wet.add_delay(track);
            assert_eq!(wet.app.nav.tracks[track].fx_target(), Some(FxTarget::Track(0)));
            assert_eq!(wet.app.nav.tracks[track].fx_chain[slot].params.len(), PARAM_COUNT);
            // Fully wet, so the echo is measured rather than the mix of it.
            wet.app.set_fx_param(track, slot, PARAM_MIX, 100.0);
            let rung = wet.full_render();

            let (seconds, _) = synced_seconds(SYNC_DEFAULT, bpm);
            let wanted = (seconds * FS) as usize;
            assert!(
                tail_peak(&rung) > 0.01,
                "{bpm}: a delay on the track left {:.3e}",
                tail_peak(&rung)
            );
            let landed = argmax(&rung, BURST_FRAMES + 512, wanted + BURST_FRAMES + 4_000);
            // The echo of a burst is the burst's own peak, so the arrival is
            // measured against where that peak was in the source.
            let source_peak = argmax(&rung, 0, BURST_FRAMES);
            let measured = landed as isize - source_peak as isize;
            assert!(
                (measured - wanted as isize).abs() < 64,
                "{bpm} bpm: the echo landed {measured} frames later, not {wanted}"
            );
        }
    }

    /// **The chain is the same feature on every strip.** A delay on the master
    /// repeats the whole mix; one on a bus repeats the return.
    #[test]
    fn a_delay_works_on_the_master_and_on_a_bus() {
        let mut rig = Rig::with_burst();
        rig.clear_bus(TrackKind::SendA);
        rig.clear_bus(TrackKind::SendB);
        let master = rig.track_index(TrackKind::Master);
        let slot = rig.add_delay(master);
        assert_eq!(rig.app.nav.tracks[master].fx_target(), Some(FxTarget::Master));
        rig.app.set_fx_param(master, slot, PARAM_MIX, 100.0);
        assert!(tail_peak(&rig.full_render()) > 0.01, "a delay on the master made no echo");

        // A bus, with the send open. This one keeps the shipped delay and just
        // turns the send up, which is the whole point of it being there.
        let mut rig = Rig::with_burst();
        rig.clear_bus(TrackKind::SendA);
        let track = rig.app.nav.track_cursor;
        rig.app.nav.tracks[track].set_send_db(SendSlot::B, 0.0);
        rig.app.sync_routing(track);
        let bus = rig.track_index(TrackKind::SendB);
        assert_eq!(rig.app.nav.tracks[bus].fx_target(), Some(FxTarget::BusB));
        assert!(
            tail_peak(&rig.full_render()) > 0.005,
            "the send bus produced no echo with the send open"
        );
    }

    /// **A new session is one keystroke from an audible delay send.**
    ///
    /// Send B ships with the synced delay at 100% wet and the strip says
    /// `dly`. With the send closed the mix is exactly what it would be with no
    /// bus at all; turning it up is a delay, in the render, through the real
    /// mixer — and the dry signal is untouched by it, which is what a fully wet
    /// bus is *for*.
    #[test]
    fn a_new_session_ships_with_a_delay_on_send_b() {
        let rig = Rig::with_burst();
        let bus = rig.track_index(TrackKind::SendB);
        let chain = rig.app.nav.tracks[bus].fx_chain.clone();
        assert_eq!(chain.len(), 1, "Send B did not ship with anything on it");
        assert_eq!(chain[0].fx_type, FxType::Delay);
        assert!(!chain[0].bypass, "the shipped delay arrived bypassed");
        assert_eq!(chain[0].params[PARAM_MIX], 100.0, "a send bus must be fully wet");
        assert_eq!(chain[0].params[PARAM_SYNC], 1.0, "the send delay is not synced");
        assert_eq!(chain[0].params[PARAM_DIVISION], SYNC_DEFAULT as f32);
        assert_eq!(SYNC_LABELS[SYNC_DEFAULT], "1/8D");
        assert_eq!(chain[0].params[PARAM_FEEDBACK], 30.0);
        assert_eq!(chain[0].params[PARAM_LOW_CUT_HZ], 200.0, "the loop filters ship on");
        assert_eq!(chain[0].params[PARAM_HIGH_CUT_HZ], 6_000.0);
        assert_eq!(chain[0].params[PARAM_ROUTING], Routing::Stereo.index() as f32);
        assert_eq!(chain[0].params[PARAM_MODE], Mode::Digital.index() as f32);
        assert_eq!(
            phosphor_app::fx::bus_label(&chain, SendSlot::B),
            "dly",
            "the strip does not name the bus after what is in it"
        );

        // The audio thread was given it at startup, without anyone asking.
        let installed = rig.app.drain_mixer_commands();
        assert!(
            installed.iter().any(|c| matches!(
                c,
                MixerCommand::AddFx { target: FxTarget::BusB, slot: 0, effect }
                    if effect.name() == "delay"
            )),
            "the bus delay never reached the mixer"
        );

        // Closed, the send contributes nothing at all.
        let mut closed = Rig::with_burst();
        closed.clear_bus(TrackKind::SendA);
        let quiet = closed.full_render();
        assert!(tail_peak(&quiet) < 1.0e-6, "a closed send was audible: {:.3e}", tail_peak(&quiet));

        // Open, it is a delay.
        let mut open = Rig::with_burst();
        open.clear_bus(TrackKind::SendA);
        let track = open.app.nav.track_cursor;
        open.app.nav.tracks[track].set_send_db(SendSlot::B, 0.0);
        open.app.sync_routing(track);
        let sounding = open.full_render();
        assert!(
            tail_peak(&sounding) > 0.005,
            "turning the send up produced {:.3e} of echo",
            tail_peak(&sounding)
        );

        // **And the source's own path is untouched by it.** The bus is 100%
        // wet, so none of the dry comes back a second time — which is the
        // phasey-send trap, and the reason the bus overrides the insert
        // default. Measured over the whole stretch before the first echo can
        // possibly have arrived: a dotted eighth at 120 BPM is 375 ms.
        let head = (0.370 * FS) as usize;
        for i in 0..head {
            assert_eq!(
                quiet[i * 2].to_bits(),
                sounding[i * 2].to_bits(),
                "opening a fully-wet send changed the dry signal at frame {i}"
            );
        }
    }

    /// **The wire promise survives integration.** A delay nobody has turned up
    /// is bit for bit the render without one — the line runs, so the echo is
    /// there when the knob comes back, and none of it is added.
    #[test]
    fn a_delay_at_wet_zero_is_bit_identical_to_no_chain() {
        let mut bare = Rig::with_tone();
        bare.clear_bus(TrackKind::SendA);
        bare.clear_bus(TrackKind::SendB);
        bare.settle_commands();
        let _ = bare.render(SETTLE_BLOCKS);
        let without = bare.render(400);

        let mut rig = Rig::with_tone();
        rig.clear_bus(TrackKind::SendA);
        rig.clear_bus(TrackKind::SendB);
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_delay(track);
        rig.app.set_fx_param(track, slot, PARAM_MIX, 0.0);
        rig.settle_commands();
        let _ = rig.render(SETTLE_BLOCKS);
        let with = rig.render(400);
        assert_bit_identical(&with, &without, "a delay at 0% wet");
    }

    /// Bypassing a delay mid-echo restores the dry signal exactly, once the
    /// crossfade has landed.
    #[test]
    fn bypassing_a_delay_restores_the_dry_signal_exactly() {
        let mut bare = Rig::with_tone();
        bare.clear_bus(TrackKind::SendA);
        bare.clear_bus(TrackKind::SendB);
        bare.settle_commands();
        let _ = bare.render(400);
        let without = bare.render(600);

        let mut rig = Rig::with_tone();
        rig.clear_bus(TrackKind::SendA);
        rig.clear_bus(TrackKind::SendB);
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_delay(track);
        rig.settle_commands();
        let _ = rig.render(200);
        rig.app.set_fx_bypass(track, slot, true);
        // Past the 8 ms crossfade by a wide margin.
        let _ = rig.render(200);
        let with = rig.render(600);
        assert_bit_identical(&with, &without, "a bypassed delay");
    }

    /// **The tempo reaches the audio thread every block.**
    ///
    /// Not through a parameter and not through the UI: the mixer reads the
    /// transport and hands it to the effect, so a tempo changed while the
    /// mixer is running moves the echo without anybody sending a command.
    #[test]
    fn a_tempo_change_moves_the_echo_without_a_command() {
        let mut rig = Rig::with_burst();
        rig.clear_bus(TrackKind::SendA);
        rig.clear_bus(TrackKind::SendB);
        rig.transport.set_tempo(120.0);
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_delay(track);
        rig.app.set_fx_param(track, slot, PARAM_MIX, 100.0);
        rig.app.set_fx_param(track, slot, PARAM_FEEDBACK, 0.0);
        rig.settle_commands();
        // Let the burst through and past its echo at 120.
        let _ = rig.render(1_400);

        // Now change the tempo and nothing else. No command is sent.
        rig.transport.set_tempo(80.0);
        let before = rig.app.drain_mixer_commands().len();
        assert_eq!(before, 0, "the tempo change sent {before} mixer commands");

        // Reset the instrument so the burst plays again, and measure.
        let track_id = rig.app.nav.tracks[track].mixer_id.expect("an instrument track has an id");
        let _ = rig
            .app
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetInstrument { track_id, instrument: Box::new(Burst::new()) });
        rig.settle_commands();
        let out = rig.render(2_200);

        let (seconds, _) = synced_seconds(SYNC_DEFAULT, 80.0);
        let wanted = (seconds * FS) as usize;
        let source_peak = argmax(&out, 0, BURST_FRAMES);
        let landed = argmax(&out, BURST_FRAMES + 512, wanted + BURST_FRAMES + 8_000);
        let measured = landed as isize - source_peak as isize;
        assert!(
            (measured - wanted as isize).abs() < 128,
            "at 80 bpm the echo landed {measured} frames later, not {wanted}"
        );
        // ...and it is a different place from where 120 bpm put it.
        let at_120 = (synced_seconds(SYNC_DEFAULT, 120.0).0 * FS) as isize;
        assert!(
            (measured - at_120).abs() > 2_000,
            "the echo did not move when the tempo did"
        );
    }

    /// **A session restores a delay on a track and on a bus**, control for
    /// control, and the audio it makes is the audio it made.
    #[test]
    fn a_session_restores_a_delay_on_a_track_and_on_a_bus() {
        let dir = std::env::temp_dir().join(format!("phosphor-dly-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("delay.phos");
        let path_str = path.to_string_lossy().to_string();

        let mut saving = Rig::with_burst();
        saving.clear_bus(TrackKind::SendA);
        let track = saving.app.nav.track_cursor;
        saving.app.nav.tracks[track].set_send_db(SendSlot::B, 0.0);
        saving.app.sync_routing(track);

        let slot = saving.add_delay(track);
        // Every control moved off its default, so that a vector which failed
        // to round trip could not pass by accident.
        let moved = [
            (PARAM_MODE, Mode::Tape.index() as f32),
            (PARAM_ROUTING, Routing::PingPong.index() as f32),
            (PARAM_SYNC, 0.0),
            (PARAM_DIVISION, 11.0),
            (PARAM_TIME_MS, 462.0),
            (5, -35.0),
            (6, 2.0),
            (PARAM_FEEDBACK, 145.0),
            (8, 1.0),
            (PARAM_LOW_CUT_HZ, 90.0),
            (PARAM_HIGH_CUT_HZ, 3_500.0),
            (11, 40.0),
            (12, 150.0),
            (PARAM_HEADS, 6.0),
            (14, 55.0),
            (PARAM_MIX, 45.0),
        ];
        assert_eq!(moved.len(), PARAM_COUNT, "a control was left out of the round trip");
        for (index, value) in moved {
            saving.app.set_fx_param(track, slot, index, value);
        }

        // And the bus keeps the delay it shipped with, tuned.
        let bus = saving.track_index(TrackKind::SendB);
        saving.app.set_fx_param(bus, 0, PARAM_DIVISION, 6.0);

        let track_params = saving.app.nav.tracks[track].fx_chain[slot].params.clone();
        let bus_params = saving.app.nav.tracks[bus].fx_chain[0].params.clone();
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
        let bus = reopened.track_index(TrackKind::SendB);

        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain.len(),
            1,
            "the track's chain did not come back"
        );
        assert_eq!(reopened.app.nav.tracks[track].fx_chain[0].fx_type, FxType::Delay);
        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain[0].params.len(),
            PARAM_COUNT,
            "a delay has sixteen controls and all of them are stored"
        );
        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain[0].params, track_params,
            "a control came back as a different number"
        );
        for (index, value) in moved {
            assert_eq!(
                reopened.app.nav.tracks[track].fx_chain[0].params[index], value,
                "index {index}"
            );
        }
        assert_eq!(reopened.app.nav.tracks[bus].fx_chain[0].params, bus_params);
        // The two counted controls are stored by position, and the position
        // has to mean what it meant.
        assert_eq!(
            HEAD_LABELS[reopened.app.nav.tracks[track].fx_chain[0].params[PARAM_HEADS] as usize],
            "1+2+3"
        );

        // **The load path is deterministic, bit for bit.** Two independent
        // loads of the same file render the same samples, which is the claim
        // that matters for "the session reopened": whatever a player hears
        // today they hear tomorrow.
        let with_source = |rig: &mut Rig| {
            let track_id =
                rig.app.nav.tracks[track].mixer_id.expect("an instrument track has an id");
            let _ = rig.app.engine.shared.mixer_command_tx.send(MixerCommand::SetInstrument {
                track_id,
                instrument: Box::new(Burst::new()),
            });
        };
        with_source(&mut reopened);
        let after = reopened.full_render();

        let mut again = Rig::new();
        again.app.do_load(&path_str);
        with_source(&mut again);
        assert_bit_identical(&again.full_render(), &after, "two loads of one session");
        assert!(tail_peak(&after) > 0.001, "the reloaded session made no echo");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The menu adds a working delay**, at its canonical place in the chain,
    /// and the audio thread is told about it.
    #[test]
    fn the_fx_menu_adds_a_working_delay() {
        let mut rig = Rig::with_burst();
        rig.clear_bus(TrackKind::SendA);
        rig.clear_bus(TrackKind::SendB);
        let track = rig.app.nav.track_cursor;
        let _ = rig.app.drain_mixer_commands();

        // A reverb first, so the delay has something to be placed *before*:
        // the canonical order is delay then reverb, and adding an effect
        // inserts at its place rather than appending.
        let outcome = rig.app.nav.add_fx(FxType::Reverb);
        rig.app.apply_fx_add(outcome);
        rig.app.nav.fx_menu.open = true;
        rig.app.nav.fx_menu.cursor = FxType::ALL
            .iter()
            .position(|f| *f == FxType::Delay)
            .expect("the delay is in the menu");
        rig.app.fx_menu_choose();

        assert!(!rig.app.nav.fx_menu.open, "the menu stayed open");
        let chain = &rig.app.nav.tracks[track].fx_chain;
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].fx_type, FxType::Delay, "the delay did not land before the reverb");
        assert!(chain[0].is_active(), "a new effect arrived bypassed");
        assert_eq!(chain[0].params.len(), PARAM_COUNT);
        let status = rig.app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("delay added at slot 1"), "the status bar said {status:?}");

        let commands = rig.app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::AddFx { target: FxTarget::Track(0), slot: 0, effect }
                    if effect.name() == "delay"
            )),
            "the delay never reached the mixer"
        );

        // And the same route, left alone, makes a sound.
        let mut sounding = Rig::with_burst();
        sounding.clear_bus(TrackKind::SendA);
        sounding.clear_bus(TrackKind::SendB);
        sounding.app.nav.fx_menu.open = true;
        sounding.app.nav.fx_menu.cursor =
            FxType::ALL.iter().position(|f| *f == FxType::Delay).unwrap();
        sounding.app.fx_menu_choose();
        assert!(
            tail_peak(&sounding.full_render()) > 1.0e-4,
            "the delay the menu added made no echo"
        );
    }

    /// **The panic key stops a screaming delay.**
    ///
    /// Feedback past unity is a bounded, deliberate self-oscillation and the
    /// one thing a player needs is a way out of it that does not involve
    /// finding the knob. The panic path already flushes every insert chain;
    /// this is the delay proving it, at the top of the feedback range, with
    /// the line full.
    #[test]
    fn the_panic_key_flushes_a_screaming_delay() {
        let mut rig = Rig::with_burst();
        rig.clear_bus(TrackKind::SendA);
        rig.clear_bus(TrackKind::SendB);
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_delay(track);
        rig.app.set_fx_param(track, slot, PARAM_MIX, 100.0);
        rig.app.set_fx_param(track, slot, PARAM_FEEDBACK, 200.0);
        rig.settle_commands();

        let singing = rig.render(1_200);
        let level = window_peak(&singing, 60_000, 76_000);
        assert!(level > 0.05, "the loop was not singing to begin with: {level:.4}");

        rig.mixer.reset_all();
        let after = rig.render(1_200);
        // The line is empty, so nothing at all comes out until whatever the
        // panic *left* running has had a delay time to come back. Measured
        // over 300 ms, inside the dotted eighth the grid is on: exactly zero,
        // not nearly.
        let quiet = (0.300 * FS) as usize;
        assert_eq!(
            window_peak(&after, 0, quiet),
            0.0,
            "the scream survived the panic"
        );
    }

    /// A mix full of delays, in every mode, never reaches the allocator.
    ///
    /// The property is about the code rather than the output, so no test that
    /// only reads the render can catch a breach of it. The counting allocator
    /// lives in `test_fx_eq`, installed once for the whole binary.
    #[test]
    fn a_mix_full_of_delays_never_reaches_the_allocator() {
        let mut rig = Rig::with_burst();
        for _ in 0..3 {
            rig.app.create_instrument_track(InstrumentType::Synth);
        }
        let mut mode = 0.0f32;
        for index in 0..rig.app.nav.tracks.len() {
            if rig.app.nav.tracks[index].fx_target().is_some()
                && rig.app.nav.tracks[index].fx_chain.is_empty()
            {
                let slot = rig.add_delay(index);
                rig.app.set_fx_param(index, slot, PARAM_MODE, mode);
                rig.app.set_fx_param(index, slot, PARAM_FEEDBACK, 60.0);
                mode = (mode + 1.0) % 3.0;
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
            "the audio callback allocated {allocations} times with delays in the chain"
        );
    }
}
