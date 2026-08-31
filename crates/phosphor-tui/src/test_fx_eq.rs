//! The EQ, end to end: menu to mixer to loudspeaker.
//!
//! The filter is tested where it lives, against golden coefficients, in
//! `phosphor_dsp::fx::eq`; the adapter that puts it in a slot is tested in
//! `phosphor_app::fx::eq`. What is under test here is everything between the
//! two — that choosing "eq" in the menu builds one, that the command reaches
//! the audio thread, that a parameter set in decibels is decibels of measured
//! level in the render, and that all of it survives being written to a file
//! and read back.
//!
//! **The rig is the real one.** A headless [`App`] keeps the receiving end of
//! the mixer command channel, so these tests build a real [`Mixer`] on that
//! same channel: the UI sends what it always sends, the mixer applies what it
//! always applies, and what is measured is the interleaved stereo the device
//! would have been handed. The only thing that is not real is the
//! instrument — a synth's spectrum is not something a filter's gain can be
//! measured against, so the track's plugin is replaced with a steady sine of
//! a known frequency.

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use phosphor_app::fx::eq_response_db;
    use phosphor_app::state::{FxType, InstrumentType};
    use phosphor_core::clip::ClipSnapshot;
    use phosphor_core::engine::VuLevels;
    use phosphor_core::fx::{FxTarget, SendSlot};
    use phosphor_core::mixer::{clip_snapshot_channel, Mixer, MixerCommand};
    use phosphor_core::project::TrackKind;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::fx::eq::{BandParam, PARAM_COUNT};
    use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

    use crate::app::App;

    // ── An allocation counter for the audio path ──
    //
    // The third copy of this device in the project, after `phosphor_core` and
    // `phosphor_dsp`, and for the reason those two say: a global allocator is
    // installed once per test binary, and "the callback never calls the
    // allocator" is a property of the code rather than of its output, so no
    // test that only reads the output can catch a breach of it. This binary
    // is the only one where the mixer and a real effect meet.
    //
    // Counted per thread, because cargo runs tests in parallel and a global
    // count would see every other test's work.
    pub(crate) mod alloc_count {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
        }

        struct Counting;

        fn note_allocation() {
            let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
        }

        // SAFETY: every method forwards to the system allocator with the same
        // pointer and layout it was given, so the allocator's contract is the
        // system allocator's contract. The counter is a thread-local `Cell`
        // of a plain integer, which allocates nothing and cannot re-enter.
        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                note_allocation();
                System.alloc(layout)
            }
            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                System.dealloc(ptr, layout);
            }
            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                note_allocation();
                System.alloc_zeroed(layout)
            }
            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                note_allocation();
                System.realloc(ptr, layout, new_size)
            }
        }

        #[global_allocator]
        static COUNTING: Counting = Counting;

        /// How many times the allocator was reached on this thread while
        /// `body` ran.
        pub(crate) fn allocations_during(body: impl FnOnce()) -> u64 {
            let before = ALLOCATIONS.with(Cell::get);
            body();
            ALLOCATIONS.with(Cell::get) - before
        }
    }

    const SAMPLE_RATE: u32 = 44_100;
    const FS: f64 = SAMPLE_RATE as f64;
    /// Frames per callback, the same 64 the engine defaults to.
    const BLOCK: usize = 64;
    /// The largest block the mixer is built for.
    const MAX_BLOCK: usize = 256;

    /// How loud the test tone is.
    ///
    /// Low on purpose. A +12 dB boost on top of this is −14 dBFS, and the
    /// master limiter never engages: a test that measured a boost through a
    /// limiter would be measuring the limiter.
    const AMPLITUDE: f32 = 0.05;

    /// The band the acceptance boosts, and where it sits by default.
    const PROBE_BAND: usize = 4;
    const PROBE_HZ: f64 = 2500.0;
    /// A decade below the band, where nothing much may happen.
    const CONTROL_HZ: f64 = 250.0;
    /// What "nothing much" is, exactly: a Q 1 bell boosted 12 dB is still
    /// worth this much 3.3 octaves below its centre. The control is not zero
    /// and asserting that it were would be asserting a different filter.
    const SKIRT_DB: f64 = 0.1618;

    /// How far a measured level may sit from the level that was asked for.
    ///
    /// Not a fudge: every measurement below lands within 0.001 dB of its
    /// prediction, and this is two orders of magnitude above the spread that
    /// three platforms' `sin` and `exp` put into a coefficient chain. Loose
    /// enough to be portable, tight enough that a band an octave out or a
    /// gain law off by a factor of two could not squeeze through.
    const TOLERANCE_DB: f64 = 0.02;

    // ── A signal with a frequency ──

    /// A steady sine, as an instrument.
    ///
    /// The phase comes from a sample counter rather than from an accumulator
    /// so that two renders of the same length are the same samples, bit for
    /// bit, however the buffer happens to be cut up.
    struct Tone {
        freq: f64,
        amplitude: f32,
        sample_rate: f64,
        n: u64,
    }

    impl Tone {
        fn new(freq: f64) -> Self {
            Self { freq, amplitude: AMPLITUDE, sample_rate: FS, n: 0 }
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
            let w = 2.0 * std::f64::consts::PI * self.freq / self.sample_rate;
            let frames = outputs.iter().map(|o| o.len()).min().unwrap_or(0);
            for i in 0..frames {
                let s = (w * (self.n + i as u64) as f64).sin() as f32 * self.amplitude;
                for out in outputs.iter_mut() {
                    out[i] = s;
                }
            }
            self.n += frames as u64;
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

    /// A headless application with a real mixer on the other end of its
    /// command channel.
    struct Rig {
        app: App,
        mixer: Mixer,
        transport: Arc<Transport>,
        /// Held open so the mixer's clip channel is not disconnected.
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

        /// A rig with one track playing a sine at `freq`.
        fn with_tone(freq: f64) -> Self {
            let mut rig = Self::new();
            rig.app.create_instrument_track(InstrumentType::Synth);
            rig.point_track_at_tone(rig.app.nav.track_cursor, freq);
            rig
        }

        /// Replace a track's instrument with the test tone.
        ///
        /// A filter's gain is a statement about one frequency, and a synth
        /// does not have one.
        fn point_track_at_tone(&mut self, track_index: usize, freq: f64) {
            let track_id = self.app.nav.tracks[track_index]
                .mixer_id
                .expect("an instrument track has an id");
            let _ = self
                .app
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::SetInstrument {
                    track_id,
                    instrument: Box::new(Tone::new(freq)),
                });
        }

        fn track_index(&self, kind: TrackKind) -> usize {
            self.app
                .nav
                .tracks
                .iter()
                .position(|t| t.kind == kind)
                .expect("the strip exists")
        }

        /// Take the plate off Send A, on both sides.
        ///
        /// A new session ships with one there so that a send is audible
        /// without any routing work — which is the right default and the
        /// wrong thing to measure an EQ's gain through. Every level in this
        /// file is a statement about the EQ, and a reverb on the return is a
        /// second variable in it.
        fn strip_the_send_bus(&mut self) {
            let bus = self.track_index(TrackKind::SendA);
            self.app.clear_chain(bus);
        }

        /// Put an EQ on a strip through the path a keypress takes, and answer
        /// with the slot it landed in.
        fn add_eq(&mut self, track_index: usize) -> usize {
            self.app.nav.track_cursor = track_index;
            let outcome = self.app.nav.add_fx(FxType::Eq);
            let slot = match &outcome {
                phosphor_app::state::FxAdd::Added { slot, .. } => *slot,
                phosphor_app::state::FxAdd::NotBuilt(_) => panic!("the EQ is not registered"),
                phosphor_app::state::FxAdd::ChainFull => panic!("the chain was full"),
                phosphor_app::state::FxAdd::Nothing => panic!("no strip under the cursor"),
            };
            self.app.apply_fx_add(outcome);
            slot
        }

        /// Apply every queued command without rendering a sample.
        ///
        /// The command budget spreads a burst over several callbacks, so a
        /// rig that started rendering immediately would have its tone at a
        /// different phase depending on how the burst happened to fall. Every
        /// comparison below is between two renders, so they have to start
        /// from the same place: no commands outstanding, and no audio yet.
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

        /// Render `blocks` callbacks and keep every sample, interleaved.
        fn render(&mut self, blocks: usize) -> Vec<f32> {
            let mut out = vec![0.0f32; BLOCK * 2];
            let mut all = Vec::with_capacity(BLOCK * 2 * blocks);
            for _ in 0..blocks {
                self.mixer.process(&mut out, &[], &self.transport);
                all.extend_from_slice(&out);
            }
            all
        }

        /// Everything the master produced after the filters have reached
        /// steady state.
        fn steady_render(&mut self) -> Vec<f32> {
            self.settle_commands();
            let _warmup = self.render(WARMUP_BLOCKS);
            self.render(ANALYSIS_BLOCKS)
        }
    }

    /// Long enough for an IIR to settle, for the 15 ms parameter smoother to
    /// arrive, for an 8 ms bypass crossfade to land — and for the difference
    /// between two filters that reached the same coefficients by different
    /// routes to fall below the last bit of an `f32`.
    ///
    /// That last one is what sets the number, and it is 743 ms rather than
    /// the 30 ms the others need. A parameter set on a running EQ glides to
    /// its target over 15 ms while a session load snaps to it, so the two
    /// instances have different state histories; an IIR's memory of its own
    /// history is infinite in exact arithmetic and about sixteen thousand
    /// samples in `f32`. Anything shorter and the bit-exact comparisons below
    /// find one ulp of it and are right to.
    const WARMUP_BLOCKS: usize = 512;
    /// 32 768 frames of analysis, 743 ms — 1857 periods of the control tone,
    /// which is what makes a single-bin measurement of it meaningful.
    const ANALYSIS_BLOCKS: usize = 512;

    // ── Measurement ──

    /// The level of one frequency in an interleaved render, in dB relative to
    /// full scale, measured on the left channel.
    ///
    /// A Hann-windowed single-bin DFT. Not peak: the master limiter and the
    /// summing bus mean the signal is not guaranteed to be one clean sine, and
    /// a peak is not monotonic in the level of a component. Not RMS either —
    /// that measures everything at once, and the whole point is what happened
    /// at *this* frequency.
    fn level_db(render: &[f32], freq: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq / FS;
        let frames = render.len() / 2;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for i in 0..frames {
            let win = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / frames as f64).cos();
            let phase = w * i as f64;
            re += f64::from(render[i * 2]) * phase.cos() * win;
            im -= f64::from(render[i * 2]) * phase.sin() * win;
        }
        10.0 * re.mul_add(re, im * im).log10()
    }

    /// What one render did to a frequency that the other did not.
    fn difference_db(with: &[f32], without: &[f32], freq: f64) -> f64 {
        assert_eq!(with.len(), without.len(), "renders of different lengths");
        level_db(with, freq) - level_db(without, freq)
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

    /// **A band asked for +12 dB gives +12 dB, in the mix.**
    ///
    /// Through the whole path: the UI builds the effect, the command carries
    /// it to the mixer, `Effect::init` designs it for the device's rate, a
    /// `SetFxParam` in decibels arrives, and the master output has twelve
    /// more decibels at 2.5 kHz than the same render without the chain. The
    /// control frequency a decade down moves by the bell's own skirt and
    /// nothing else.
    #[test]
    fn an_eq_on_a_track_boosts_the_band_it_was_pointed_at() {
        for (freq, expected) in [(PROBE_HZ, 12.0), (CONTROL_HZ, SKIRT_DB)] {
            let mut plain = Rig::with_tone(freq);
            let bare = plain.steady_render();

            let mut eq = Rig::with_tone(freq);
            let index = eq.app.nav.track_cursor;
            let slot = eq.add_eq(index);
            eq.app
                .set_fx_param(index, slot, BandParam::Gain.index(PROBE_BAND), 12.0);
            let boosted = eq.steady_render();

            let measured = difference_db(&boosted, &bare, freq);
            assert!(
                (measured - expected).abs() < TOLERANCE_DB,
                "at {freq} Hz the chain moved the level by {measured:+.4} dB, not {expected:+.4}"
            );

            // The mirror the UI holds says the same thing the render did,
            // which is what makes the drawn curve trustworthy.
            let mirror = &eq.app.nav.tracks[index].fx_chain[slot].params;
            let drawn = eq_response_db(mirror, FS, freq);
            assert!(
                (drawn - measured).abs() < TOLERANCE_DB,
                "the curve says {drawn:+.3} dB at {freq} Hz and the render is {measured:+.3}"
            );
        }
    }

    /// The same EQ on the master does the same thing. The master's chain runs
    /// ahead of the safety limiter, so a boost that reaches it is a boost in
    /// the file.
    #[test]
    fn an_eq_on_the_master_boosts_the_whole_mix() {
        for (freq, expected) in [(PROBE_HZ, 12.0), (CONTROL_HZ, SKIRT_DB)] {
            let mut plain = Rig::with_tone(freq);
            let bare = plain.steady_render();

            let mut eq = Rig::with_tone(freq);
            let master = eq.track_index(TrackKind::Master);
            let slot = eq.add_eq(master);
            assert_eq!(
                eq.app.nav.tracks[master].fx_target(),
                Some(FxTarget::Master),
                "the master strip is not addressed as the master"
            );
            eq.app
                .set_fx_param(master, slot, BandParam::Gain.index(PROBE_BAND), 12.0);
            let boosted = eq.steady_render();

            let measured = difference_db(&boosted, &bare, freq);
            assert!(
                (measured - expected).abs() < TOLERANCE_DB,
                "at {freq} Hz the master EQ moved the mix by {measured:+.4} dB, not {expected:+.4}"
            );
        }
    }

    /// And on a send bus.
    ///
    /// A bus is a parallel path, so the number to expect is not +12: the
    /// master hears the dry track *and* the boosted return, and the sum of
    /// the two is what a mix engineer would hear. The assertion is against
    /// that sum computed from the EQ's own curve — `(1 + G·H) / (1 + G)` for
    /// a send at unity into a return at unity — rather than against a
    /// tolerance wide enough to swallow the difference, because the dilution
    /// is the thing that makes this test about the *bus*.
    ///
    /// The measurement is at the band's centre and only there. Summing two
    /// paths adds *complex* responses, and the arithmetic above treats `H` as
    /// real, which it is at a bell's centre frequency and nowhere else: on
    /// the skirt the phase term is worth a couple of hundredths of a decibel,
    /// which is larger than this assertion's tolerance and would be a test
    /// measuring its own approximation.
    #[test]
    fn an_eq_on_a_send_bus_colours_the_return() {
        fn rig_with_open_send() -> Rig {
            let mut rig = Rig::with_tone(PROBE_HZ);
            rig.strip_the_send_bus();
            let track = rig.app.nav.track_cursor;
            rig.app.nav.tracks[track].set_send_db(SendSlot::A, 0.0);
            rig.app.sync_routing(track);
            rig
        }

        /// The reference for "the send is open": the same rig with it closed.
        fn render_with_the_send_closed() -> Vec<f32> {
            let mut rig = Rig::with_tone(PROBE_HZ);
            rig.strip_the_send_bus();
            rig.steady_render()
        }

        let mut plain = rig_with_open_send();
        let bare = plain.steady_render();

        let mut eq = rig_with_open_send();
        let bus = eq.track_index(TrackKind::SendA);
        let slot = eq.add_eq(bus);
        assert_eq!(eq.app.nav.tracks[bus].fx_target(), Some(FxTarget::BusA));
        eq.app
            .set_fx_param(bus, slot, BandParam::Gain.index(PROBE_BAND), 12.0);
        let boosted = eq.steady_render();

        // What the send being open is worth at all: the bus has to be in the
        // mix before an EQ on it can be heard.
        let opened = difference_db(&bare, &render_with_the_send_closed(), PROBE_HZ);
        assert!(
            (opened - 6.0206).abs() < TOLERANCE_DB,
            "opening the send at unity should double the level, and moved it {opened:+.4} dB"
        );

        let mirror = &eq.app.nav.tracks[bus].fx_chain[slot].params;
        let h = 10.0f64.powf(eq_response_db(mirror, FS, PROBE_HZ) / 20.0);
        let expected = 20.0 * ((1.0 + h) / 2.0).log10();
        let measured = difference_db(&boosted, &bare, PROBE_HZ);
        assert!(
            (measured - expected).abs() < TOLERANCE_DB,
            "the bus EQ moved the mix by {measured:+.4} dB against the {expected:+.4} its \
             curve summed with the dry path predicts"
        );
    }

    /// **The wire promise survives integration.** An EQ nobody has touched,
    /// in a chain, on a track that is playing, is bit for bit the render that
    /// had no chain at all. This is what makes inserting an EQ mid-take safe.
    #[test]
    fn a_fresh_eq_in_a_chain_is_bit_identical_to_no_chain() {
        let mut plain = Rig::with_tone(PROBE_HZ);
        let bare = plain.steady_render();

        let mut eq = Rig::with_tone(PROBE_HZ);
        let index = eq.app.nav.track_cursor;
        eq.add_eq(index);
        let flat = eq.steady_render();

        assert_bit_identical(&flat, &bare, "a flat EQ");
    }

    /// A bypassed slot is not in the signal path — not "inaudible", not in
    /// it — and unbypassing puts it back.
    #[test]
    fn bypassing_an_eq_mid_note_restores_the_dry_signal_exactly() {
        // The reference is given the same history as the rig under test,
        // down to the block: the tone's phase is a function of how many
        // samples have been rendered, and a bit-exact comparison between two
        // renders that started at different points in the waveform would fail
        // for a reason that has nothing to do with bypass.
        let mut plain = Rig::with_tone(PROBE_HZ);
        plain.settle_commands();
        let _before_the_switch = plain.render(WARMUP_BLOCKS);
        let bare = plain.steady_render();

        let mut eq = Rig::with_tone(PROBE_HZ);
        let index = eq.app.nav.track_cursor;
        let slot = eq.add_eq(index);
        eq.app
            .set_fx_param(index, slot, BandParam::Gain.index(PROBE_BAND), 12.0);

        // Playing, boosted, and then the switch is thrown mid-note.
        eq.settle_commands();
        let _running = eq.render(WARMUP_BLOCKS);
        eq.app.set_fx_bypass(index, slot, true);
        assert!(eq.app.nav.tracks[index].fx_chain[slot].bypass, "the mirror missed the switch");
        let settled = eq.steady_render();
        assert_bit_identical(&settled, &bare, "a settled bypass");

        // ...and back on, to the boost it had before.
        eq.app.set_fx_bypass(index, slot, false);
        let restored = eq.steady_render();
        let measured = difference_db(&restored, &bare, PROBE_HZ);
        assert!(
            (measured - 12.0).abs() < TOLERANCE_DB,
            "unbypassing came back at {measured:+.4} dB, not +12"
        );
    }

    /// **The session round trip.** Two bands moved on a track and one on a
    /// bus, written to a file and read back: the chain is where it was, every
    /// parameter is the number it was saved as, and the render is the same
    /// audio sample for sample.
    #[test]
    fn a_session_restores_an_eq_on_a_track_and_on_a_bus() {
        let dir = std::env::temp_dir().join(format!("phosphor-eq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("eq.phos");
        let path_str = path.to_string_lossy().to_string();

        let mut saving = Rig::with_tone(PROBE_HZ);
        saving.strip_the_send_bus();
        let track = saving.app.nav.track_cursor;
        saving.app.nav.tracks[track].set_send_db(SendSlot::A, 0.0);
        saving.app.sync_routing(track);

        let track_slot = saving.add_eq(track);
        saving
            .app
            .set_fx_param(track, track_slot, BandParam::Gain.index(PROBE_BAND), 12.0);
        saving
            .app
            .set_fx_param(track, track_slot, BandParam::Freq.index(2), 400.0);
        saving
            .app
            .set_fx_param(track, track_slot, BandParam::Gain.index(2), -6.0);

        let bus = saving.track_index(TrackKind::SendA);
        let bus_slot = saving.add_eq(bus);
        saving
            .app
            .set_fx_param(bus, bus_slot, BandParam::Gain.index(6), 4.5);

        let before = saving.steady_render();
        let track_params = saving.app.nav.tracks[track].fx_chain[track_slot].params.clone();
        let bus_params = saving.app.nav.tracks[bus].fx_chain[bus_slot].params.clone();
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
        assert_eq!(reopened.app.nav.tracks[bus].fx_chain.len(), 1, "the bus chain did not come back");
        assert_eq!(reopened.app.nav.tracks[track].fx_chain[0].fx_type, FxType::Eq);
        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain[0].params.len(),
            PARAM_COUNT,
            "an EQ has forty-nine controls and all of them are stored"
        );
        assert_eq!(
            reopened.app.nav.tracks[track].fx_chain[0].params, track_params,
            "a control came back as a different number"
        );
        assert_eq!(reopened.app.nav.tracks[bus].fx_chain[0].params, bus_params);

        // The audio, not just the numbers. Same instrument, same chain, same
        // samples — which is the only definition of "the session reopened"
        // that a listener would accept.
        reopened.point_track_at_tone(track, PROBE_HZ);
        let after = reopened.steady_render();
        assert_bit_identical(&after, &before, "a reloaded session");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Seven EQs — one on each of four tracks, one on each bus, one on the
    /// master — and the callback still never reaches the allocator.
    ///
    /// Fifty-six biquads and their coefficient interpolation, on the audio
    /// thread, with no `Vec` growing behind them. The EQ's own zero-alloc
    /// test covers `process`; this one covers the whole callback with the
    /// effects in it, which is where a `Vec<FxSlot>` or a scratch buffer
    /// would be the thing that grew.
    #[test]
    fn a_mix_full_of_eqs_never_reaches_the_allocator() {
        let mut rig = Rig::new();
        for i in 0..4 {
            rig.app.create_instrument_track(InstrumentType::Synth);
            let index = rig.app.nav.track_cursor;
            rig.point_track_at_tone(index, 300.0 * (i + 1) as f64);
            rig.app.nav.tracks[index].set_send_db(SendSlot::A, -6.0);
            rig.app.nav.tracks[index].set_send_db(SendSlot::B, -6.0);
            rig.app.sync_routing(index);
            let slot = rig.add_eq(index);
            rig.app
                .set_fx_param(index, slot, BandParam::Gain.index(PROBE_BAND), 6.0);
        }
        for kind in [TrackKind::SendA, TrackKind::SendB, TrackKind::Master] {
            let index = rig.track_index(kind);
            let slot = rig.add_eq(index);
            rig.app.set_fx_param(index, slot, BandParam::Gain.index(1), -3.0);
        }

        rig.settle_commands();
        let _warm = rig.render(8);
        // The output buffer is built outside the measurement; only the
        // callback is inside it.
        let mut out = vec![0.0f32; BLOCK * 2];
        let allocations = alloc_count::allocations_during(|| {
            for _ in 0..64 {
                rig.mixer.process(&mut out, &[], &rig.transport);
            }
        });
        assert_eq!(allocations, 0, "the callback allocated {allocations} times");

        // ...and it was actually doing the work: seven chains, and a mix that
        // is not silence.
        let render = rig.render(8);
        assert!(render.iter().any(|s| s.abs() > 0.01), "the rig rendered silence");
    }

    /// The menu path, from the keystroke's point of view: choosing the first
    /// entry adds an EQ, says where it landed, tells the audio thread, and
    /// leaves the strip's list showing a real effect rather than a
    /// placeholder.
    #[test]
    fn the_fx_menu_adds_a_working_eq() {
        let mut app = App::new(
            EngineConfig { buffer_size: BLOCK as u32, sample_rate: SAMPLE_RATE },
            false,
            false,
        );
        app.create_instrument_track(InstrumentType::Synth);
        let index = app.nav.track_cursor;
        let _ = app.drain_mixer_commands();

        app.nav.fx_menu.open = true;
        app.nav.fx_menu.cursor = 0;
        app.fx_menu_choose();

        assert!(!app.nav.fx_menu.open, "the menu stayed open");
        let chain = &app.nav.tracks[index].fx_chain;
        assert_eq!(chain.len(), 1, "the mirror has no slot");
        assert_eq!(chain[0].fx_type, FxType::Eq);
        assert!(chain[0].is_active(), "a new effect arrived bypassed");
        assert_eq!(chain[0].params.len(), PARAM_COUNT);

        let status = app.live_status().unwrap_or_default().to_string();
        assert!(status.contains("eq added at slot 1"), "the status bar said {status:?}");

        let commands = app.drain_mixer_commands();
        let added: Vec<&MixerCommand> = commands
            .iter()
            .filter(|c| matches!(c, MixerCommand::AddFx { .. }))
            .collect();
        assert_eq!(added.len(), 1, "the audio thread was told once, and only once");
        match added[0] {
            MixerCommand::AddFx { target, slot, effect } => {
                assert_eq!(*target, FxTarget::Track(0));
                assert_eq!(*slot, 0);
                assert_eq!(effect.name(), "eq", "the slot holds something else");
                assert_eq!(effect.parameter_count(), PARAM_COUNT);
            }
            _ => unreachable!(),
        }
    }

    /// A strip that is not a track gets one too. The bus and master strips
    /// address their chains by identity rather than by a track id, and this
    /// is the test that would catch that addressing coming undone.
    #[test]
    fn every_strip_can_hold_an_eq() {
        let mut rig = Rig::new();
        rig.app.create_instrument_track(InstrumentType::Synth);
        let targets = [
            (rig.app.nav.track_cursor, FxTarget::Track(0)),
            (rig.track_index(TrackKind::SendA), FxTarget::BusA),
            (rig.track_index(TrackKind::SendB), FxTarget::BusB),
            (rig.track_index(TrackKind::Master), FxTarget::Master),
        ];
        let _ = rig.app.drain_mixer_commands();
        for (index, want) in targets {
            let slot = rig.add_eq(index);
            assert_eq!(slot, 0, "an EQ is the first thing in a canonical chain");
            let sent = rig
                .app
                .drain_mixer_commands()
                .into_iter()
                .find_map(|c| match c {
                    MixerCommand::AddFx { target, .. } => Some(target),
                    _ => None,
                })
                .expect("nothing reached the audio thread");
            assert_eq!(sent, want, "the wrong chain was addressed");
        }
    }
}
