//! The reverb, end to end: menu to mixer to loudspeaker, and the send bus a
//! new session ships with.
//!
//! The tank is tested where it lives, against measured decay times, in
//! `phosphor_dsp::fx::reverb`; the adapter that puts it in a slot is tested in
//! `phosphor_app::fx::reverb`. What is under test here is everything between
//! the two — that choosing "reverb" in the menu builds one, that the command
//! reaches the audio thread, that a note played into a track with one on it
//! rings after the note stops, that turning a send up on a *new* session is
//! audible without any routing work, and that all of it survives being
//! written to a file and read back.
//!
//! **The rig is the real one.** A headless [`App`] keeps the receiving end of
//! the mixer command channel, so these tests build a real [`Mixer`] on that
//! same channel: the UI sends what it always sends, the mixer applies what it
//! always applies, and what is measured is the interleaved stereo the device
//! would have been handed. The instrument is a short burst rather than a
//! synth, because "did the tail outlast the note" is a statement about what
//! happens *after* the source stops and a synth's own release would be a
//! second variable in it.

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
    use phosphor_dsp::fx::reverb::{
        Algorithm, PARAM_ALGORITHM, PARAM_COUNT, PARAM_DECAY_S, PARAM_EARLY, PARAM_MIX,
        PARAM_PREDELAY_MS, PARAM_SIZE,
    };
    use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

    use crate::app::App;

    const SAMPLE_RATE: u32 = 44_100;
    const FS: f64 = SAMPLE_RATE as f64;
    const BLOCK: usize = 64;
    const MAX_BLOCK: usize = 256;

    /// How long the source plays for, in frames — about a fifth of a second.
    const BURST_FRAMES: usize = 8_192;
    /// How loud it is. Low, so that nothing measured here is the master
    /// limiter's opinion rather than the reverb's.
    const AMPLITUDE: f32 = 0.1;
    /// Long enough for the wet/dry smoother to arrive at exactly zero: it is
    /// a 15 ms one-pole walking down from 25%, and it snaps the last
    /// millionth, which takes about 190 ms.
    const SETTLE_BLOCKS: usize = 400;

    // ── A source that stops ──

    /// A burst of a 220 Hz sine, then silence, forever.
    ///
    /// The phase comes from a sample counter rather than an accumulator so
    /// that two renders of the same length are the same samples however the
    /// buffer is cut up.
    struct Burst {
        sample_rate: f64,
        n: usize,
        /// How many frames the tone lasts. [`usize::MAX`] never stops, which
        /// is what the null tests want: identity between two renders proves
        /// nothing about a stretch where both are silent.
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
                        * (std::f64::consts::TAU * 220.0 * self.n as f64 / self.sample_rate).sin()
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
                EngineConfig {
                    buffer_size: BLOCK as u32,
                    sample_rate: SAMPLE_RATE,
                },
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
            Self {
                app,
                mixer,
                transport,
                _clip_rx: clip_rx,
            }
        }

        /// A rig with one track playing the burst.
        fn with_burst() -> Self {
            Self::with_source(Burst::new())
        }

        /// The same, with a tone that never stops.
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
                .send(MixerCommand::SetInstrument {
                    track_id,
                    instrument: Box::new(source),
                });
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

        /// Put a reverb on a strip through the path a keypress takes, and
        /// answer with the slot it landed in.
        fn add_reverb(&mut self, track_index: usize) -> usize {
            self.app.nav.track_cursor = track_index;
            let outcome = self.app.nav.add_fx(FxType::Reverb);
            let slot = match &outcome {
                phosphor_app::state::FxAdd::Added { slot, .. } => *slot,
                phosphor_app::state::FxAdd::NotBuilt(_) => panic!("the reverb is not registered"),
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

        /// The burst and everything after it, interleaved: 3.7 s at 44.1 kHz.
        fn full_render(&mut self) -> Vec<f32> {
            self.settle_commands();
            self.render(2_560)
        }
    }

    // ── Measurement ──

    /// RMS of the left channel over a window of *frames*, in the
    /// interleaved render.
    fn window_rms(render: &[f32], from: usize, to: usize) -> f64 {
        let frames = render.len() / 2;
        let to = to.min(frames);
        if to <= from {
            return 0.0;
        }
        let sum: f64 = (from..to)
            .map(|i| f64::from(render[i * 2]) * f64::from(render[i * 2]))
            .sum();
        (sum / (to - from) as f64).sqrt()
    }

    /// What is left a fifth of a second after the source stopped.
    fn tail_rms(render: &[f32]) -> f64 {
        window_rms(render, BURST_FRAMES + 8_192, BURST_FRAMES + 40_000)
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

    /// A rig whose send bus has been emptied, for the tests that are about a
    /// reverb somewhere else.
    fn without_the_bus_plate(rig: &mut Rig) {
        let bus = rig.track_index(TrackKind::SendA);
        rig.app.clear_chain(bus);
    }

    // ── The acceptance ──

    /// **A reverb on a track rings after the note stops.**
    ///
    /// Through the whole path: the menu builds the effect, the command
    /// carries it to the mixer, `Effect::init` sizes its delay lines for the
    /// device's rate, and a fifth of a second after the source went silent
    /// there is still sound coming out of the master where without the effect
    /// there is none.
    #[test]
    fn a_reverb_on_a_track_outlasts_the_note() {
        let mut dry = Rig::with_burst();
        without_the_bus_plate(&mut dry);
        let bare = dry.full_render();
        assert!(
            tail_rms(&bare) < 1.0e-6,
            "the dry render was already ringing: {:.3e}",
            tail_rms(&bare)
        );

        let mut wet = Rig::with_burst();
        without_the_bus_plate(&mut wet);
        let track = wet.app.nav.track_cursor;
        let slot = wet.add_reverb(track);
        assert_eq!(wet.app.nav.tracks[track].fx_target(), Some(FxTarget::Track(0)));
        assert_eq!(wet.app.nav.tracks[track].fx_chain[slot].params.len(), PARAM_COUNT);
        let rung = wet.full_render();

        let tail = tail_rms(&rung);
        assert!(
            tail > 1.0e-4,
            "a reverb on the track left {tail:.3e} where the dry render left {:.3e}",
            tail_rms(&bare)
        );

        // And the longer setting rings longer, which is the difference
        // between an effect and a constant.
        let mut longer = Rig::with_burst();
        without_the_bus_plate(&mut longer);
        let track = longer.app.nav.track_cursor;
        let slot = longer.add_reverb(track);
        longer.app.set_fx_param(track, slot, PARAM_DECAY_S, 8.0);
        let long = longer.full_render();
        assert!(
            tail_rms(&long) > tail * 1.5,
            "an 8 s decay left {:.3e} against the 1.8 s setting's {tail:.3e}",
            tail_rms(&long)
        );
    }

    /// **The chain is the same feature on every strip.** A reverb on the
    /// master rings the whole mix; one on a bus rings the return.
    #[test]
    fn a_reverb_works_on_the_master_and_on_a_bus() {
        // The master.
        let mut rig = Rig::with_burst();
        without_the_bus_plate(&mut rig);
        let master = rig.track_index(TrackKind::Master);
        let slot = rig.add_reverb(master);
        assert_eq!(rig.app.nav.tracks[master].fx_target(), Some(FxTarget::Master));
        rig.app.set_fx_param(master, slot, PARAM_MIX, 100.0);
        assert!(
            tail_rms(&rig.full_render()) > 1.0e-4,
            "a reverb on the master left no tail"
        );

        // A bus, with the send open. This one keeps the shipped plate and
        // just turns the send up, which is the whole point of it being there.
        let mut rig = Rig::with_burst();
        let track = rig.app.nav.track_cursor;
        rig.app.nav.tracks[track].set_send_db(SendSlot::A, 0.0);
        rig.app.sync_routing(track);
        let bus = rig.track_index(TrackKind::SendA);
        assert_eq!(rig.app.nav.tracks[bus].fx_target(), Some(FxTarget::BusA));
        assert!(
            tail_rms(&rig.full_render()) > 1.0e-4,
            "the send bus produced no tail with the send open"
        );
    }

    /// **A new session is one keystroke from an audible send.**
    ///
    /// Send A ships with the plate at 100% wet and the strip says `rvb`. With
    /// the send closed the mix is exactly what it would be with no bus at
    /// all; turning it up is a reverb, in the render, through the real mixer.
    #[test]
    fn a_new_session_ships_with_a_plate_on_send_a() {
        let rig = Rig::with_burst();
        let bus = rig.track_index(TrackKind::SendA);
        let chain = rig.app.nav.tracks[bus].fx_chain.clone();
        assert_eq!(chain.len(), 1, "Send A did not ship with anything on it");
        assert_eq!(chain[0].fx_type, FxType::Reverb);
        assert!(!chain[0].bypass, "the shipped plate arrived bypassed");
        assert_eq!(
            chain[0].params[PARAM_ALGORITHM],
            Algorithm::Plate.index() as f32,
            "the send bus is not a plate"
        );
        assert_eq!(chain[0].params[PARAM_MIX], 100.0, "a send bus must be fully wet");
        assert_eq!(
            phosphor_app::fx::bus_label(&chain, SendSlot::A),
            "rvb",
            "the strip does not name the bus after what is in it"
        );

        // The audio thread was given it at startup, without anyone asking.
        let installed = rig.app.drain_mixer_commands();
        assert!(
            installed.iter().any(|c| matches!(
                c,
                MixerCommand::AddFx { target: FxTarget::BusA, slot: 0, .. }
            )),
            "the bus reverb never reached the mixer"
        );

        // Closed, the send contributes nothing at all.
        let mut closed = Rig::with_burst();
        let quiet = closed.full_render();
        assert!(
            tail_rms(&quiet) < 1.0e-6,
            "a closed send was audible: {:.3e}",
            tail_rms(&quiet)
        );

        // Open, it is a reverb.
        let mut open = Rig::with_burst();
        let track = open.app.nav.track_cursor;
        open.app.nav.tracks[track].set_send_db(SendSlot::A, 0.0);
        open.app.sync_routing(track);
        let sounding = open.full_render();
        assert!(
            tail_rms(&sounding) > 1.0e-4,
            "turning the send up produced {:.3e} of tail",
            tail_rms(&sounding)
        );

        // ...and the source's own path is untouched by it. The bus is 100%
        // wet, so none of the dry signal comes back a second time — which is
        // the phasey-send trap, and the reason the bus overrides the insert
        // default. Measured over the first 20 ms, which is inside the
        // predelay and therefore before the plate has said anything at all.
        let head = (0.020 * FS) as usize;
        for i in 0..head {
            assert_eq!(
                quiet[i * 2].to_bits(),
                sounding[i * 2].to_bits(),
                "opening a fully-wet send changed the dry signal at frame {i}"
            );
        }
    }

    /// **The wire promise survives integration.** A reverb nobody has turned
    /// up is bit for bit the render without one — the tank runs, so the tail
    /// is there when the knob comes back, and none of it is added.
    #[test]
    fn a_reverb_at_wet_zero_is_bit_identical_to_no_chain() {
        // Steady state, as `FX.md` defines the null: the wet/dry is a
        // *smoothed* control, so a knob turned to zero glides there over
        // about 190 ms and only then does the dry path stop being added to.
        // What is asserted is that it arrives at exactly zero rather than at
        // a millionth, which is the difference between a null and a nearly.
        let mut bare = Rig::with_tone();
        without_the_bus_plate(&mut bare);
        bare.settle_commands();
        let _ = bare.render(SETTLE_BLOCKS);
        let without = bare.render(400);

        let mut rig = Rig::with_tone();
        without_the_bus_plate(&mut rig);
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_reverb(track);
        rig.app.set_fx_param(track, slot, PARAM_MIX, 0.0);
        rig.settle_commands();
        let _ = rig.render(SETTLE_BLOCKS);
        let with = rig.render(400);
        assert_bit_identical(&with, &without, "a reverb at 0% wet");
    }

    /// Bypassing a reverb mid-tail restores the dry signal exactly, once the
    /// crossfade has landed.
    #[test]
    fn bypassing_a_reverb_restores_the_dry_signal_exactly() {
        let mut bare = Rig::with_tone();
        without_the_bus_plate(&mut bare);
        bare.settle_commands();
        let _ = bare.render(400);
        let without = bare.render(600);

        let mut rig = Rig::with_tone();
        without_the_bus_plate(&mut rig);
        let track = rig.app.nav.track_cursor;
        let slot = rig.add_reverb(track);
        rig.settle_commands();
        let _ = rig.render(200);
        rig.app.set_fx_bypass(track, slot, true);
        // Past the 8 ms crossfade by a wide margin.
        let _ = rig.render(200);
        let with = rig.render(600);
        assert_bit_identical(&with, &without, "a bypassed reverb");
    }

    /// **A session restores a reverb on a track and on a bus**, control for
    /// control, and the audio it makes is the audio it made.
    #[test]
    fn a_session_restores_a_reverb_on_a_track_and_on_a_bus() {
        let dir = std::env::temp_dir().join(format!("phosphor-rvb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("reverb.phos");
        let path_str = path.to_string_lossy().to_string();

        let mut saving = Rig::with_burst();
        let track = saving.app.nav.track_cursor;
        saving.app.nav.tracks[track].set_send_db(SendSlot::A, 0.0);
        saving.app.sync_routing(track);

        let slot = saving.add_reverb(track);
        // Every control moved off its default, so that a vector that failed
        // to round trip could not pass by accident.
        let moved = [
            (PARAM_ALGORITHM, Algorithm::Hall.index() as f32),
            (PARAM_PREDELAY_MS, 63.0),
            (PARAM_DECAY_S, 5.5),
            (PARAM_SIZE, 1.4),
            (4, 9_000.0),
            (5, 120.0),
            (PARAM_EARLY, 55.0),
            (7, 70.0),
            (8, 0.4),
            (9, 20.0),
            (10, 85.0),
            (PARAM_MIX, 40.0),
        ];
        for (index, value) in moved {
            saving.app.set_fx_param(track, slot, index, value);
        }

        // And the bus keeps the plate it shipped with, tuned.
        let bus = saving.track_index(TrackKind::SendA);
        saving.app.set_fx_param(bus, 0, PARAM_DECAY_S, 3.2);

        let track_params = saving.app.nav.tracks[track].fx_chain[slot].params.clone();
        let bus_params = saving.app.nav.tracks[bus].fx_chain[0].params.clone();
        let before = saving.full_render();
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
        let bus = reopened.track_index(TrackKind::SendA);

        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain.len(),
            1,
            "the track's chain did not come back"
        );
        assert_eq!(reopened.app.nav.tracks[track].fx_chain[0].fx_type, FxType::Reverb);
        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain[0].params.len(),
            PARAM_COUNT,
            "a reverb has twelve controls and all of them are stored"
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

        // The audio, not just the numbers.
        let track_id = reopened.app.nav.tracks[track]
            .mixer_id
            .expect("an instrument track has an id");
        let _ = reopened
            .app
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetInstrument {
                track_id,
                instrument: Box::new(Burst::new()),
            });
        let after = reopened.full_render();

        // **The load path is deterministic, bit for bit.** Two independent
        // loads of the same file render the same samples, which is the claim
        // that matters for "the session reopened": whatever a player hears
        // today they hear tomorrow.
        let mut again = Rig::new();
        again.app.do_load(&path_str);
        let track_id = again.app.nav.tracks[track]
            .mixer_id
            .expect("an instrument track has an id");
        let _ = again
            .app
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetInstrument {
                track_id,
                instrument: Box::new(Burst::new()),
            });
        assert_bit_identical(&again.full_render(), &after, "two loads of one session");

        // **Against the session that saved it, the comparison is the sound
        // rather than the samples, and the reason is the reverb's own
        // memory.** The saving instance reached these settings through the
        // knob path — a geometry morph crossfades over 30 ms, the wet/dry
        // glides over 190 ms — while a load snaps to them before the effect
        // is in the signal path. Both are correct, and they differ in what
        // went into the tank during the first fifth of a second. A reverb is
        // linear, so that difference then decays at exactly the rate the
        // tail does and never becomes small *relative* to it: what is left
        // is a few percent, forever, and asking for bit-exactness here would
        // be asking the knob path not to glide.
        let (from, to) = (BURST_FRAMES + 8_192, BURST_FRAMES + 40_000);
        let a = window_rms(&before, from, to);
        let b = window_rms(&after, from, to);
        assert!(
            a > 1.0e-5 && (b - a).abs() / a < 0.10,
            "the tail: the reloaded session rendered {b:.6} against {a:.6}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The menu adds a working reverb**, at its canonical place in the
    /// chain, and the audio thread is told about it.
    #[test]
    fn the_fx_menu_adds_a_working_reverb() {
        // What the keystroke does, and what it tells the audio thread. The
        // commands are read off this rig, which means they are taken out of
        // the channel — so the render is measured on a second one.
        let mut rig = Rig::with_burst();
        without_the_bus_plate(&mut rig);
        let track = rig.app.nav.track_cursor;
        let _ = rig.app.drain_mixer_commands();

        // An EQ first, so the reverb has something to be placed after: the
        // canonical order is tone before time, and adding an effect inserts
        // at its place rather than appending.
        let outcome = rig.app.nav.add_fx(FxType::Eq);
        rig.app.apply_fx_add(outcome);
        rig.app.nav.fx_menu.open = true;
        rig.app.nav.fx_menu.cursor = FxType::ALL
            .iter()
            .position(|f| *f == FxType::Reverb)
            .expect("the reverb is in the menu");
        rig.app.fx_menu_choose();

        assert!(!rig.app.nav.fx_menu.open, "the menu stayed open");
        let chain = &rig.app.nav.tracks[track].fx_chain;
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1].fx_type, FxType::Reverb, "the reverb did not land after the EQ");
        assert!(chain[1].is_active(), "a new effect arrived bypassed");
        assert_eq!(chain[1].params.len(), PARAM_COUNT);
        let status = rig.app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("reverb added at slot 2"), "the status bar said {status:?}");

        let commands = rig.app.drain_mixer_commands();
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MixerCommand::AddFx { target: FxTarget::Track(0), slot: 1, effect }
                    if effect.name() == "reverb"
            )),
            "the reverb never reached the mixer"
        );

        // And the same route, left alone, makes a sound.
        let mut sounding = Rig::with_burst();
        without_the_bus_plate(&mut sounding);
        sounding.app.nav.fx_menu.open = true;
        sounding.app.nav.fx_menu.cursor =
            FxType::ALL.iter().position(|f| *f == FxType::Reverb).unwrap();
        sounding.app.fx_menu_choose();
        assert!(
            tail_rms(&sounding.full_render()) > 1.0e-4,
            "the reverb the menu added made no tail"
        );
    }

    /// A mix full of reverbs never reaches the allocator.
    ///
    /// The property is about the code rather than the output, so no test that
    /// only reads the render can catch a breach of it. The counting allocator
    /// lives in `test_fx_eq`, installed once for the whole binary.
    #[test]
    fn a_mix_full_of_reverbs_never_reaches_the_allocator() {
        let mut rig = Rig::with_burst();
        for _ in 0..3 {
            rig.app.create_instrument_track(InstrumentType::Synth);
        }
        for index in 0..rig.app.nav.tracks.len() {
            if rig.app.nav.tracks[index].fx_target().is_some()
                && rig.app.nav.tracks[index].fx_chain.is_empty()
            {
                rig.add_reverb(index);
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
            "the audio callback allocated {allocations} times with reverbs in the chain"
        );
    }
}
