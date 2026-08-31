//! The compressor, end to end: menu to mixer to loudspeaker, and the
//! sidechain that is the only reason the mixer renders in two passes.
//!
//! The detector, the ballistics and the static curve are tested where they
//! live, in `phosphor_dsp::fx::compressor`; the adapter that puts one in a
//! slot is tested in `phosphor_app::fx::compressor`. What is under test here
//! is everything between the two — that choosing "comp" in the menu builds
//! one, that the command reaches the audio thread, that **a kick on one track
//! ducks a pad on another**, that it does so whichever order the two tracks
//! sit in, that a deleted key track falls back to internal without a gap, that
//! key listen swaps the output and clears itself, and that all of it survives
//! being written to a file and read back.
//!
//! **The rig is the real mixer for a reason.** A sidechain is not a property
//! of the compressor; it is a property of the two-pass render. The only way to
//! check that a key is the *same block* and not the one before it, whichever
//! order the tracks happen to be in, is to run the mixer.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use phosphor_app::state::{FxType, InstrumentType, TrackState};
    use phosphor_core::clip::ClipSnapshot;
    use phosphor_core::engine::VuLevels;
    use phosphor_core::mixer::{clip_snapshot_channel, Mixer, MixerCommand};
    use phosphor_core::project::TrackKind;
    use phosphor_core::transport::Transport;
    use phosphor_core::EngineConfig;
    use phosphor_dsp::fx::compressor::{
        ratio_to_percent, PARAM_ATTACK_MS, PARAM_AUTO_MAKEUP, PARAM_COUNT, PARAM_KNEE_DB,
        PARAM_MIX, PARAM_RATIO, PARAM_RELEASE_MS, PARAM_SC_HPF_HZ, PARAM_THRESHOLD_DB,
    };
    use phosphor_plugin::{MidiEvent, ParameterInfo, Plugin, PluginCategory, PluginInfo};

    use crate::app::App;

    const SAMPLE_RATE: u32 = 44_100;
    const FS: f64 = SAMPLE_RATE as f64;
    const BLOCK: usize = 64;
    const MAX_BLOCK: usize = 256;

    /// How often the kick lands: 120 BPM, four on the floor, so every half
    /// second.
    const KICK_PERIOD: usize = (FS as usize) / 2;
    /// How long each kick is audible for — 40 ms, a short percussive blip.
    const KICK_FRAMES: usize = (FS as usize) / 25;

    // ── Two sources ──

    /// A four-on-the-floor kick: a decaying 60 Hz blip every half second.
    ///
    /// A *source* rather than a recording, so the test can say exactly where
    /// every onset is and measure the gain reduction against it.
    struct Kick {
        sample_rate: f64,
        n: usize,
        amplitude: f32,
    }

    impl Kick {
        fn new(amplitude: f32) -> Self {
            Self { sample_rate: FS, n: 0, amplitude }
        }
    }

    impl Plugin for Kick {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "kick".into(),
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
                let into = self.n % KICK_PERIOD;
                let value = if into < KICK_FRAMES {
                    let t = into as f64 / self.sample_rate;
                    // A rise as well as a decay. A sine switched on at a
                    // sample boundary is a step, and a step is broadband —
                    // which would make the detector's high-pass look useless
                    // when what it is really failing to remove is a click the
                    // test invented.
                    let envelope = (1.0 - (-t / 0.002).exp()) * (-t * 60.0).exp();
                    (f64::from(self.amplitude)
                        * envelope
                        * (std::f64::consts::TAU * 60.0 * t).sin()) as f32
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

    /// A pad: an unwavering 220 Hz sine, so that anything moving in the render
    /// is the compressor and not the source.
    struct Pad {
        sample_rate: f64,
        n: usize,
        amplitude: f32,
    }

    impl Pad {
        fn new(amplitude: f32) -> Self {
            Self { sample_rate: FS, n: 0, amplitude }
        }
    }

    impl Plugin for Pad {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "pad".into(),
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
                let value = (f64::from(self.amplitude)
                    * (std::f64::consts::TAU * 220.0 * self.n as f64 / self.sample_rate).sin())
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
        /// How many frames the sources have produced so far, so a test can say
        /// where in the render a kick lands rather than guessing.
        frames: usize,
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
            let mut rig = Self { app, mixer, transport, _clip_rx: clip_rx, frames: 0 };
            // The send buses ship loaded; nothing here is about them.
            rig.clear_bus(TrackKind::SendA);
            rig.clear_bus(TrackKind::SendB);
            rig
        }

        fn clear_bus(&mut self, kind: TrackKind) {
            let bus = self
                .app
                .nav
                .tracks
                .iter()
                .position(|t| t.kind == kind)
                .expect("the strip exists");
            self.app.clear_chain(bus);
        }

        /// Add an instrument track carrying `source`, and answer its index in
        /// the strip.
        fn add_source(&mut self, source: Box<dyn Plugin>, name: &str) -> usize {
            self.app.create_instrument_track(InstrumentType::Synth);
            let index = self.app.nav.track_cursor;
            self.app.nav.tracks[index].name = name.to_string();
            let track_id = self.app.nav.tracks[index]
                .mixer_id
                .expect("an instrument track has an id");
            let _ = self
                .app
                .engine
                .shared
                .mixer_command_tx
                .send(MixerCommand::SetInstrument { track_id, instrument: source });
            index
        }

        /// Put a compressor on a strip through the path a keypress takes, and
        /// answer the slot it landed in.
        fn add_comp(&mut self, track_index: usize) -> usize {
            self.app.nav.track_cursor = track_index;
            let outcome = self.app.nav.add_fx(FxType::Compressor);
            let slot = match &outcome {
                phosphor_app::state::FxAdd::Added { slot, .. } => *slot,
                phosphor_app::state::FxAdd::NotBuilt(_) => panic!("the compressor is not built"),
                phosphor_app::state::FxAdd::ChainFull => panic!("the chain was full"),
                phosphor_app::state::FxAdd::Nothing => panic!("no strip under the cursor"),
            };
            self.app.apply_fx_add(outcome);
            slot
        }

        fn settle_commands(&mut self) {
            for _ in 0..2_000 {
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

        /// Drain the commands, then render a second and a half and throw it
        /// away.
        ///
        /// Two things need it. An effect is installed at its factory settings
        /// and *then* told what the test wants, so the first block after a
        /// setup carries the makeup ramp gliding from one to the other. And
        /// the gain-reduction meter has a 300 ms visual release on it, so a
        /// measurement taken straight after a change would find the previous
        /// setting's reduction still on its way down and report it as the new
        /// setting's. A second and a half is five of those time constants.
        fn warm_up(&mut self) {
            self.settle_commands();
            let _ = self.render(3 * (FS as usize) / 2 / BLOCK);
        }

        /// Render `blocks` blocks and answer the interleaved master output.
        fn render(&mut self, blocks: usize) -> Vec<f32> {
            let mut out = vec![0.0f32; BLOCK * 2];
            let mut all = Vec::with_capacity(BLOCK * 2 * blocks);
            for _ in 0..blocks {
                self.mixer.process(&mut out, &[], &self.transport);
                all.extend_from_slice(&out);
                self.frames += BLOCK;
            }
            all
        }

        /// Render, and answer the gain reduction the compressor published,
        /// one entry per block.
        fn render_with_gr(&mut self, blocks: usize, meter_of: usize) -> (Vec<f32>, Vec<f32>) {
            let meter = self.app.nav.tracks[meter_of].fx_chain[0]
                .gr
                .clone()
                .expect("the compressor published a meter");
            let mut out = vec![0.0f32; BLOCK * 2];
            let mut all = Vec::with_capacity(BLOCK * 2 * blocks);
            let mut gr = Vec::with_capacity(blocks);
            for _ in 0..blocks {
                self.mixer.process(&mut out, &[], &self.transport);
                all.extend_from_slice(&out);
                gr.push(meter.current_db());
                self.frames += BLOCK;
            }
            (all, gr)
        }
    }

    // ── Measurement ──

    /// The terminal, as text.
    fn screen(app: &App, width: u16, height: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let snapshot = app.engine.transport.snapshot();
        let status = app.live_status();
        terminal
            .draw(|frame| crate::ui::render(frame, &snapshot, &app.nav, status))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn peak(render: &[f32], from: usize, to: usize) -> f64 {
        let frames = render.len() / 2;
        let to = to.min(frames);
        if to <= from {
            return 0.0;
        }
        (from..to).map(|i| f64::from(render[i * 2].abs())).fold(0.0, f64::max)
    }

    /// Pearson correlation, for "does the gain reduction follow the kick".
    fn correlation(a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len());
        if n == 0 {
            return 0.0;
        }
        let mean_a: f64 = a[..n].iter().map(|v| f64::from(*v)).sum::<f64>() / n as f64;
        let mean_b: f64 = b[..n].iter().map(|v| f64::from(*v)).sum::<f64>() / n as f64;
        let mut num = 0.0;
        let mut da = 0.0;
        let mut db = 0.0;
        for i in 0..n {
            let x = f64::from(a[i]) - mean_a;
            let y = f64::from(b[i]) - mean_b;
            num += x * y;
            da += x * x;
            db += y * y;
        }
        if da <= 0.0 || db <= 0.0 {
            return 0.0;
        }
        num / (da * db).sqrt()
    }

    /// How loud the pad is: −30 dBFS, which is under the threshold the pump is
    /// dialled to. That is the point — the pad asks for nothing on its own, so
    /// every decibel of reduction in the render came off the key.
    const PAD_AMPLITUDE: f32 = 0.03;

    /// A compressor dialled to duck, hard and audibly, and to be *back* before
    /// the next beat.
    ///
    /// The release is the whole effect: a kick at 120 BPM is every 500 ms, and
    /// 120 ms of release means the 17 dB the kick asks for is under a decibel
    /// again 460 ms later. Dial it longer and the pump degenerates into a
    /// level drop, which is what test V7's "less than a decibel between kicks"
    /// is really asserting.
    fn dial_the_pump(rig: &mut Rig, track: usize, slot: usize) {
        rig.app.set_fx_param(track, slot, PARAM_THRESHOLD_DB, -24.0);
        rig.app.set_fx_param(track, slot, PARAM_RATIO, ratio_to_percent(4.0));
        rig.app.set_fx_param(track, slot, PARAM_KNEE_DB, 0.0);
        rig.app.set_fx_param(track, slot, PARAM_ATTACK_MS, 0.5);
        rig.app.set_fx_param(track, slot, PARAM_RELEASE_MS, 120.0);
        rig.app.set_fx_param(track, slot, PARAM_AUTO_MAKEUP, 0.0);
        rig.app.set_fx_param(track, slot, PARAM_MIX, 100.0);
    }

    /// The pad's amplitude, window by window, from the render.
    ///
    /// RMS times root two rather than a peak: a 220 Hz sine is four and a half
    /// milliseconds long, so a peak taken over a ten-millisecond window is
    /// two samples' worth of luck. The RMS is the level.
    fn pad_envelope(render: &[f32], window: usize) -> Vec<f64> {
        let frames = render.len() / 2;
        (0..frames / window)
            .map(|w| {
                let sum: f64 = (w * window..(w + 1) * window)
                    .map(|i| {
                        let s = f64::from(render[i * 2]);
                        s * s
                    })
                    .sum();
                (sum / window as f64).sqrt() * std::f64::consts::SQRT_2
            })
            .collect()
    }

    // ── The acceptance ──

    /// **A kick on one track ducks a pad on another, and the reduction follows
    /// the kick's envelope.**
    ///
    /// The whole feature, through the whole path: the menu builds the
    /// compressor, the command carries it to the mixer, the key names the kick
    /// track *by identity*, and the mixer resolves it to a same-block buffer
    /// every callback.
    ///
    /// Four assertions, and each one catches a different way of getting this
    /// wrong: the reduction has to be deep at the onsets (the key reached the
    /// detector), shallow between them (the release is real and not a stuck
    /// gain), correlated with the kick (it is following *that* signal and not
    /// something else), and uncorrelated when the key is switched back to
    /// internal (the pad's own steady level asks for nothing).
    #[test]
    fn a_kick_on_the_key_input_ducks_a_pad() {
        /// Ten milliseconds, which is two cycles of the pad and a quarter of a
        /// kick blip.
        const WINDOW: usize = 441;

        let mut rig = Rig::new();
        let kick_track = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
        let pad_track = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
        let slot = rig.add_comp(pad_track);
        dial_the_pump(&mut rig, pad_track, slot);

        // The kick is heard through its key only: muted, it still keys, and
        // that is what makes the measurement the pad alone.
        rig.app.nav.tracks[kick_track].muted = true;
        rig.app.nav.tracks[kick_track].sync_to_audio();
        rig.app.set_key_source(pad_track, Some(kick_id));

        rig.transport.play();
        rig.warm_up();
        let start = rig.frames;
        let blocks = 4 * (FS as usize) / BLOCK;
        let (out, gr) = rig.render_with_gr(blocks, pad_track);

        // What the audio actually did, window by window. The meter is a
        // *display*, with its own 300 ms visual release on top of the
        // compressor's, so the audio is what the ducking is measured on.
        let envelope = pad_envelope(&out, WINDOW);
        let quiet: f64 = envelope
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(quiet > 0.01, "the pad never reached its own level: {quiet:.4}");

        // Where the kicks are, taken from how many frames the sources have
        // produced rather than assumed — the warm-up is not a whole number of
        // measurement windows, and a test that guesses this measures the
        // recovery and calls it the attack.
        let onsets: Vec<usize> = (1..)
            .map(|k| k * KICK_PERIOD)
            .take_while(|frame| *frame < start + out.len() / 2)
            .filter(|frame| *frame >= start)
            .map(|frame| (frame - start) / WINDOW)
            .filter(|w| *w >= 3 && w + 2 < envelope.len())
            .collect();
        assert!(onsets.len() >= 4, "the render was too short to hold four kicks");

        for &onset in &onsets {
            // Deep at the onset: the detector is looking at the kick, which is
            // 22 dB above the threshold, so 4:1 asks for about 17.
            let at = envelope[onset..onset + 2].iter().copied().fold(f64::MAX, f64::min);
            let taken = 20.0 * (at / quiet).log10();
            assert!(
                taken < -6.0,
                "window {onset}: only {taken:.2} dB came off the pad on a kick"
            );

            // ...and back up before it. A hundred and twenty milliseconds of
            // release inside a five-hundred-millisecond beat is the whole of
            // what makes a pump a pump rather than a level drop: the previous
            // kick's reduction is gone by the time this one arrives.
            let just_before = onset - 2;
            let back = 20.0 * (envelope[just_before] / quiet).log10();
            assert!(
                back > -1.0,
                "window {just_before}: {back:.2} dB was still coming off when the next kick \
                 arrived"
            );
        }

        // **The reduction has the shape the settings predict**, not merely the
        // same period as the kick.
        //
        // The reference is built from the kick's own envelope run through the
        // static curve and the release — in decibels, because that is the
        // domain the detector works in and a linear-envelope reference would
        // be comparing two different shapes and calling the difference an
        // error. What this catches is a detector fed the wrong signal: any
        // other source in the session has a different envelope and would not
        // correlate.
        let mut held_db = 0.0f64;
        let release = (-(WINDOW as f64 / FS) / 0.120).exp();
        let predicted: Vec<f32> = (0..envelope.len())
            .map(|w| {
                let into = (start + w * WINDOW) % KICK_PERIOD;
                let level = if into < KICK_FRAMES {
                    let t = into as f64 / FS;
                    0.8 * (1.0 - (-t / 0.002).exp()) * (-t * 60.0).exp()
                } else {
                    0.0
                };
                let over = 20.0 * level.max(1.0e-7).log10() - (-24.0);
                let asked = if over > 0.0 { 0.75 * over } else { 0.0 };
                held_db = asked.max(held_db * release);
                held_db as f32
            })
            .collect();
        let ducking: Vec<f32> = envelope
            .iter()
            .map(|a| (-20.0 * (a / quiet).log10()) as f32)
            .collect();
        let r = correlation(&ducking, &predicted);
        assert!(r > 0.8, "the reduction does not follow the kick: r = {r:.3}");

        // ...and the meter saw it too, which is what the panel draws.
        assert!(
            gr.iter().fold(0.0f32, |w, g| w.min(*g)) < -6.0,
            "the meter never registered the ducking"
        );

        // With the key back on internal, the pad's own level is under the
        // threshold and the gain stops moving at all.
        rig.app.set_key_source(pad_track, None);
        rig.warm_up();
        let (_, internal) = rig.render_with_gr(blocks, pad_track);
        let moved = internal.iter().fold(0.0f32, |worst, g| worst.min(*g));
        assert!(
            moved > -0.5,
            "the internal key still moved the gain by {moved:.2} dB \u{2014} the key never \
             switched"
        );
    }

    /// **Track order does not matter, bit for bit.**
    ///
    /// The justification for the mixer rendering in two passes, and it deserves
    /// a test that fails loudly if somebody reintroduces a single pass: with
    /// the kick *before* the pad and with it *after*, the gain-reduction
    /// envelope has to be identical sample for sample. A single-pass render
    /// would hand the compressor last block's kick in one of the two orders.
    #[test]
    fn the_key_is_the_same_block_whichever_order_the_tracks_are_in() {
        fn take(kick_first: bool) -> Vec<f32> {
            let mut rig = Rig::new();
            let (kick_track, pad_track) = if kick_first {
                let k = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
                let p = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
                (k, p)
            } else {
                let p = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
                let k = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
                (k, p)
            };
            let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
            let slot = rig.add_comp(pad_track);
            dial_the_pump(&mut rig, pad_track, slot);
            rig.app.nav.tracks[kick_track].muted = true;
            rig.app.nav.tracks[kick_track].sync_to_audio();
            rig.app.set_key_source(pad_track, Some(kick_id));
            rig.transport.play();
            rig.warm_up();
            let (out, _) = rig.render_with_gr(2 * (FS as usize) / BLOCK, pad_track);
            out
        }

        let kick_before = take(true);
        let kick_after = take(false);
        assert!(
            peak(&kick_before, 0, usize::MAX) > 0.001,
            "the reference render was silent, so identity proves nothing"
        );
        for (i, (a, b)) in kick_before.iter().zip(&kick_after).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "sample {i} depends on which order the tracks sit in: {a} vs {b}"
            );
        }
    }

    /// **A muted key track still keys.**
    ///
    /// The tap is post-instrument and pre-fader, so the mute — which lives at
    /// the fader — cannot reach it. That is deliberate and it is the useful
    /// behaviour: a trigger track that has been silenced still triggers, which
    /// is how a ghost kick is used. Asserted here rather than left as an
    /// accident of where the tap happens to be.
    #[test]
    fn a_muted_key_track_still_keys() {
        let mut rig = Rig::new();
        let kick_track = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
        let pad_track = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
        let slot = rig.add_comp(pad_track);
        dial_the_pump(&mut rig, pad_track, slot);
        rig.app.nav.tracks[kick_track].muted = true;
        rig.app.nav.tracks[kick_track].sync_to_audio();
        rig.app.set_key_source(pad_track, Some(kick_id));
        rig.transport.play();
        rig.warm_up();

        let (out, gr) = rig.render_with_gr(2 * (FS as usize) / BLOCK, pad_track);
        let worst = gr.iter().fold(0.0f32, |w, g| w.min(*g));
        assert!(worst < -6.0, "a muted key track stopped keying: {worst:.2} dB");
        // ...and nothing of the kick itself is in the mix: the pad on its own
        // never reaches a tenth of full scale.
        assert!(
            peak(&out, 0, usize::MAX) < 0.1,
            "the muted kick was audible: {}",
            peak(&out, 0, usize::MAX)
        );
    }

    /// **A deleted key track falls back to internal, and the panel says
    /// which one went.**
    ///
    /// Never silence, never a frozen gain, never whatever track happens to
    /// have moved into that position — and the name is kept so the message is
    /// something a player can act on.
    #[test]
    fn a_deleted_key_track_falls_back_and_says_so() {
        let mut rig = Rig::new();
        let kick_track = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
        let pad_track = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
        let slot = rig.add_comp(pad_track);
        dial_the_pump(&mut rig, pad_track, slot);
        rig.app.set_key_source(pad_track, Some(kick_id));
        rig.transport.play();
        rig.warm_up();

        let (_, keyed) = rig.render_with_gr((FS as usize) / BLOCK, pad_track);
        assert!(
            keyed.iter().fold(0.0f32, |w, g| w.min(*g)) < -6.0,
            "the key never arrived to begin with"
        );
        assert_eq!(crate::ui::fx::comp_key_label(&rig.app.nav), "Kick");

        // The kick goes. The mirror still names it, the panel marks it, and
        // the audio falls back this block rather than reading a stale buffer.
        rig.app.nav.track_cursor = kick_track;
        rig.app.execute_delete(crate::state::ConfirmKind::DeleteTrack);
        rig.app.nav.track_cursor = rig
            .app
            .nav
            .tracks
            .iter()
            .position(|t| t.name == "Pad")
            .expect("the pad is still there");
        rig.settle_commands();
        assert_eq!(crate::ui::fx::comp_key_label(&rig.app.nav), "Kick (missing)");

        let pad_now = rig.app.nav.track_cursor;
        // Two seconds first: the meter's own visual release is 300 ms, and
        // what is under test is where the gain settles rather than how long
        // the display takes to admit it.
        let _ = rig.render(2 * (FS as usize) / BLOCK);
        let (out, fallen_back) = rig.render_with_gr((FS as usize) / BLOCK, pad_now);
        assert!(
            fallen_back.iter().fold(0.0f32, |w, g| w.min(*g)) > -0.5,
            "the deleted key kept compressing \u{2014} something stale was still being read"
        );
        assert!(
            peak(&out, 0, usize::MAX) > f64::from(PAD_AMPLITUDE) * 0.5,
            "the pad went silent when the key went"
        );
    }

    /// **Undo puts a deleted key track back, and the key finds it again.**
    ///
    /// A restored track is a *new* track as far as the mixer is concerned, so
    /// the key that named the old one is dangling — and undo is exactly the
    /// moment that should stop being true.
    #[test]
    fn undo_re_resolves_a_key_that_lost_its_track() {
        let mut rig = Rig::new();
        let kick_track = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
        let pad_track = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
        let slot = rig.add_comp(pad_track);
        dial_the_pump(&mut rig, pad_track, slot);
        rig.app.set_key_source(pad_track, Some(kick_id));
        assert_eq!(crate::ui::fx::comp_key_label(&rig.app.nav), "Kick");

        rig.app.nav.track_cursor = kick_track;
        rig.app.execute_delete(crate::state::ConfirmKind::DeleteTrack);
        let pad_now = rig
            .app
            .nav
            .tracks
            .iter()
            .position(|t| t.name == "Pad")
            .expect("the pad is still there");
        rig.app.nav.track_cursor = pad_now;
        assert_eq!(crate::ui::fx::comp_key_label(&rig.app.nav), "Kick (missing)");

        rig.app.perform_undo();
        let pad_now = rig
            .app
            .nav
            .tracks
            .iter()
            .position(|t| t.name == "Pad")
            .expect("the pad survived the undo");
        rig.app.nav.track_cursor = pad_now;
        let restored = rig
            .app
            .nav
            .tracks
            .iter()
            .find(|t| t.name == "Kick")
            .and_then(|t| t.mixer_id)
            .expect("the kick came back");
        assert_ne!(restored, kick_id, "the restored track kept its old identity");
        assert_eq!(
            rig.app.nav.tracks[pad_now].key_source,
            Some(restored),
            "the key did not find the restored track"
        );
        assert_eq!(crate::ui::fx::comp_key_label(&rig.app.nav), "Kick");

        // ...and the audio thread was told, so the sound follows the screen.
        //
        // The restored track comes back with the *instrument type* it had,
        // which in this rig is the default synth rather than the test kick,
        // so the kick is put back on it by hand — what is under test is that
        // the mixer is keying off the restored track's new identity.
        let _ = rig
            .app
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetInstrument {
                track_id: restored,
                instrument: Box::new(Kick::new(0.8)),
            });
        rig.transport.play();
        rig.warm_up();
        let (_, gr) = rig.render_with_gr((FS as usize) / BLOCK, pad_now);
        assert!(
            gr.iter().fold(0.0f32, |w, g| w.min(*g)) < -6.0,
            "the key came back on screen and not in the signal path"
        );
    }

    /// **Key listen swaps the output, and puts it back.**
    ///
    /// What comes out of the pad's strip is the kick — audibly, measurably,
    /// and only while the panel that armed it is open.
    #[test]
    fn key_listen_swaps_the_output_and_clears_itself() {
        let mut rig = Rig::new();
        let kick_track = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
        let pad_track = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
        let pad_id = rig.app.nav.tracks[pad_track].mixer_id.unwrap();
        let slot = rig.add_comp(pad_track);
        // A compressor doing nothing at all, so what is measured is the swap.
        rig.app.set_fx_param(pad_track, slot, PARAM_THRESHOLD_DB, 0.0);
        rig.app.set_fx_param(pad_track, slot, PARAM_AUTO_MAKEUP, 0.0);
        rig.app.set_key_source(pad_track, Some(kick_id));
        rig.app.nav.tracks[kick_track].muted = true;
        rig.app.nav.tracks[kick_track].sync_to_audio();
        rig.transport.play();
        rig.warm_up();

        // Off: a steady pad, the same level everywhere — because the pad is
        // steady and the compressor is doing nothing.
        let plain = rig.render((FS as usize) / BLOCK);
        let quiet_before = peak(&plain, KICK_FRAMES * 2, KICK_PERIOD - 1);
        let loud_before = peak(&plain, 0, KICK_FRAMES);
        assert!(
            (quiet_before - loud_before).abs() < 0.001 && quiet_before > 0.01,
            "the pad is not steady: {loud_before} against {quiet_before}"
        );

        // On: the strip plays the kick, which is silent between beats and
        // much louder on them. Both halves are the measurement.
        rig.app.set_key_listen(Some(pad_id));
        rig.warm_up();
        let listened = rig.render((FS as usize) / BLOCK);
        // The key is silent between beats and loud on them, and both halves
        // are the measurement: the pad has gone and the kick has arrived.
        let whole = peak(&listened, 0, usize::MAX);
        let quiet_window = listened
            .chunks_exact(2)
            .skip(KICK_FRAMES * 3)
            .take(KICK_PERIOD / 4)
            .map(|f| f64::from(f[0].abs()))
            .fold(0.0, f64::max);
        assert!(
            quiet_window < 0.001,
            "the pad was still audible under the key listen: {quiet_window}"
        );
        assert!(whole > 0.4, "the kick did not come through: {whole}");

        // A transport stop clears it on the audio thread's own initiative,
        // whatever the front end does — the safety net for a UI that has
        // stopped answering.
        rig.transport.pause();
        rig.render(4);
        assert_eq!(rig.mixer.key_listen(), None, "the stop did not clear the key listen");

        // ...and leaving the panel clears the mirror, which is the rule the
        // front end enforces every frame.
        rig.app.nav.clip_view.fx.close();
        rig.app.enforce_key_listen();
        assert_eq!(rig.app.nav.key_listen, None, "leaving the panel left it armed");
    }

    /// **Only one key listen at a time, and the type is what says so.**
    #[test]
    fn only_one_key_listen_is_ever_armed() {
        let mut rig = Rig::new();
        let a = rig.add_source(Box::new(Pad::new(0.2)), "A");
        let b = rig.add_source(Box::new(Pad::new(0.2)), "B");
        let (id_a, id_b) = (
            rig.app.nav.tracks[a].mixer_id.unwrap(),
            rig.app.nav.tracks[b].mixer_id.unwrap(),
        );
        rig.app.set_key_listen(Some(id_a));
        assert_eq!(rig.app.nav.key_listen, Some(id_a));
        assert_eq!(rig.app.nav.key_listen_track_name(), Some("A"));
        rig.app.set_key_listen(Some(id_b));
        assert_eq!(rig.app.nav.key_listen, Some(id_b), "the second arming did not take");
        assert!(rig.app.nav.is_key_listening(b));
        assert!(!rig.app.nav.is_key_listening(a), "two strips were armed at once");

        rig.settle_commands();
        assert_eq!(rig.mixer.key_listen(), Some(id_b), "the audio thread has the other one");
    }

    /// **The compressor arrives at the settings `../FX.md` settled.**
    ///
    /// Dropped on a track whose instrument is gain-staged to −12 dBFS peaks,
    /// the factory setting is two or three decibels of reduction on the loud
    /// notes and nothing on the quiet ones — which is what a compressor is
    /// supposed to sound like the first time somebody adds one.
    #[test]
    fn the_factory_setting_compresses_without_being_asked() {
        let mut rig = Rig::new();
        // −12 dBFS peaks, the house's gain-staging target.
        let pad_track = rig.add_source(Box::new(Pad::new(0.251)), "Pad");
        let slot = rig.add_comp(pad_track);
        assert_eq!(slot, 0);
        assert_eq!(rig.app.nav.tracks[pad_track].fx_chain[0].params.len(), PARAM_COUNT);
        assert!(
            rig.app.nav.tracks[pad_track].fx_chain[0].gr.is_some(),
            "the mirror never got the meter, so the panel would draw a dead bar"
        );
        rig.transport.play();
        rig.settle_commands();

        let (_, gr) = rig.render_with_gr(2 * (FS as usize) / BLOCK, pad_track);
        let settled = gr.last().copied().unwrap_or(0.0);
        assert!(
            settled < -1.0 && settled > -6.0,
            "the factory setting took {settled:.2} dB off a -12 dBFS source"
        );
    }

    /// **The detector high-pass keeps a kick out of the sidechain.**
    ///
    /// End to end, through the mixer, on a key that has both a 60 Hz kick and
    /// the pad's own 220 Hz in it: with the filter at 200 Hz the reduction
    /// stops following the kick.
    #[test]
    fn the_detector_highpass_reaches_the_key_path() {
        let mut rig = Rig::new();
        let kick_track = rig.add_source(Box::new(Kick::new(0.8)), "Kick");
        let pad_track = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let kick_id = rig.app.nav.tracks[kick_track].mixer_id.unwrap();
        let slot = rig.add_comp(pad_track);
        dial_the_pump(&mut rig, pad_track, slot);
        rig.app.nav.tracks[kick_track].muted = true;
        rig.app.nav.tracks[kick_track].sync_to_audio();
        rig.app.set_key_source(pad_track, Some(kick_id));
        rig.transport.play();
        rig.warm_up();

        let blocks = 2 * (FS as usize) / BLOCK;
        let (_, open) = rig.render_with_gr(blocks, pad_track);
        let with_kick = open.iter().fold(0.0f32, |w, g| w.min(*g));
        assert!(with_kick < -6.0, "the kick was not keying to begin with: {with_kick:.2} dB");

        // Two poles at 200 Hz put a 60 Hz fundamental 21 dB down and at 300 Hz
        // 28 dB down, and what is left of the kick's grip on the detector is
        // only what its own attack puts above the corner.
        //
        // Measured end to end through the mixer: 14.04 dB of reduction with
        // the filter out, 7.41 dB at 200 Hz, and 0.02 dB at 300 Hz — which is
        // the kick no longer keying at all. That is the whole reason the
        // control exists: a compressor keyed on a full mix is, in practice,
        // keyed on the kick drum.
        let mut measured = Vec::new();
        for corner in [200.0f32, 300.0] {
            rig.app.set_fx_param(pad_track, slot, PARAM_SC_HPF_HZ, corner);
            rig.warm_up();
            let (_, filtered) = rig.render_with_gr(blocks, pad_track);
            measured.push(filtered.iter().fold(0.0f32, |w, g| w.min(*g)));
        }
        assert!(
            measured[0] > with_kick + 5.0,
            "200 Hz took only {:.2} dB off the kick's grip: {with_kick:.2} -> {:.2}",
            measured[0] - with_kick,
            measured[0]
        );
        assert!(
            measured[1] > measured[0],
            "300 Hz was not tighter than 200: {:.2} against {:.2}",
            measured[1],
            measured[0]
        );
        assert!(
            measured[1] > -1.0,
            "at 300 Hz the kick was still taking {:.2} dB off the pad",
            measured[1]
        );
    }

    // ── The session ──

    /// **A compressor and its key survive a save and a load.**
    ///
    /// Twelve controls in natural units, plus a key stored by *position* in
    /// the file and resolved back to a track identity on the way in — because
    /// track ids are handed out per run and mean nothing between sessions.
    #[test]
    fn a_compressor_and_its_key_survive_a_round_trip() {
        let path = {
            let dir = std::env::temp_dir()
                .join(format!("phosphor-comp-{}-roundtrip", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            dir.join("test.phos")
        };

        let mut saving = App::new(
            EngineConfig { buffer_size: BLOCK as u32, sample_rate: SAMPLE_RATE },
            false,
            false,
        );
        saving.create_instrument_track(InstrumentType::Synth);
        let source_index = saving.nav.track_cursor;
        saving.nav.tracks[source_index].name = "Kick".to_string();
        saving.create_instrument_track(InstrumentType::DX7);
        let keyed_index = saving.nav.track_cursor;
        let source_id = saving.nav.tracks[source_index].mixer_id.unwrap();

        saving.nav.track_cursor = keyed_index;
        let outcome = saving.nav.add_fx(FxType::Compressor);
        let slot = match &outcome {
            phosphor_app::state::FxAdd::Added { slot, .. } => *slot,
            _ => panic!("the compressor did not go in"),
        };
        saving.apply_fx_add(outcome);
        let written = [
            (PARAM_THRESHOLD_DB, -27.0f32),
            (PARAM_RATIO, ratio_to_percent(10.0)),
            (PARAM_KNEE_DB, 3.0),
            (PARAM_ATTACK_MS, 0.4),
            (PARAM_RELEASE_MS, 640.0),
            (PARAM_SC_HPF_HZ, 120.0),
            (PARAM_MIX, 35.0),
        ];
        for (index, value) in written {
            saving.set_fx_param(keyed_index, slot, index, value);
        }
        saving.set_key_source(keyed_index, Some(source_id));
        saving.do_save(&path.to_string_lossy());

        let mut reopened = App::new(
            EngineConfig { buffer_size: BLOCK as u32, sample_rate: SAMPLE_RATE },
            false,
            false,
        );
        reopened.do_load(&path.to_string_lossy());
        let tracks: Vec<&TrackState> = reopened
            .nav
            .tracks
            .iter()
            .filter(|t| t.instrument_type.is_some())
            .collect();
        assert_eq!(tracks.len(), 2);
        let restored = tracks[1];
        assert_eq!(restored.fx_chain.len(), 1);
        assert_eq!(restored.fx_chain[0].fx_type, FxType::Compressor);
        for (index, value) in written {
            assert_eq!(restored.fx_chain[0].params[index], value, "control {index}");
        }
        assert_eq!(
            restored.key_source,
            tracks[0].mixer_id,
            "the key did not come back pointing at the first track"
        );
        // ...and the reinstalled chain is watching the compressor that is
        // actually in the signal path, not a twin of it.
        assert!(
            restored.fx_chain[0].gr.is_some(),
            "a loaded compressor's meter was never attached"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// **A session written before the compressor existed still opens**, and
    /// its tracks are byte for byte what they were.
    #[test]
    fn an_older_session_still_opens() {
        let path = {
            let dir =
                std::env::temp_dir().join(format!("phosphor-comp-{}-old", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            dir.join("old.phos")
        };
        // A file with no `fx` block and no `key_track` at all — the shape
        // every session written before the insert layer has.
        std::fs::write(
            &path,
            r#"{"version":1,"transport":{"tempo_bpm":128.0,"loop_enabled":false,
               "loop_start_bar":1,"loop_end_bar":5,"metronome":false},
               "tracks":[{"name":"Old","instrument_type":"synth","synth_params":[],
               "muted":false,"soloed":false,"armed":false,"volume":0.8,
               "color_index":0,"clips":[]}]}"#,
        )
        .unwrap();

        let mut app = App::new(
            EngineConfig { buffer_size: BLOCK as u32, sample_rate: SAMPLE_RATE },
            false,
            false,
        );
        app.do_load(&path.to_string_lossy());
        let track = app
            .nav
            .tracks
            .iter()
            .find(|t| t.name == "Old")
            .expect("the old session's track did not load");
        assert!(track.fx_chain.is_empty(), "a chain appeared out of nowhere");
        assert_eq!(track.key_source, None);
        assert!((track.volume - 0.8).abs() < 1.0e-6);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // ── The panel ──

    /// The panel names itself, lists its fourteen rows, and draws the meter.
    #[test]
    fn the_compressor_panel_draws_its_rows_and_its_meter() {
        let mut rig = Rig::new();
        let pad = rig.add_source(Box::new(Pad::new(PAD_AMPLITUDE)), "Pad");
        let slot = rig.add_comp(pad);
        rig.app.nav.clip_view.fx.open(slot);
        rig.app.nav.clip_view.clip_tab = crate::state::ClipTab::Fx;
        rig.app.nav.clip_view.focus = crate::state::ClipViewFocus::PianoRoll;

        let text = screen(&rig.app, 140, 44);
        assert!(text.contains("cmp"), "the panel does not name itself:\n{text}");
        assert!(text.contains("latency 0"), "the panel does not say it adds no delay:\n{text}");
        for name in [
            "char", "thresh", "ratio", "knee", "attack", "releas", "arel", "makeup", "mkauto",
            "mix", "sense", "schpf", "key", "klistn",
        ] {
            assert!(text.contains(name), "the panel is missing `{name}`:\n{text}");
        }
        assert!(text.contains("3.0:1"), "the ratio does not read as a ratio:\n{text}");
        assert!(text.contains("basic"), "the character does not read:\n{text}");
        assert!(text.contains("gr "), "the gain-reduction meter is not on the panel:\n{text}");
        assert!(text.contains("internal"), "the key row does not read:\n{text}");
        // The makeup is greyed because the automatic has it, and says so.
        assert!(text.contains("auto"), "the automatic makeup does not read:\n{text}");
    }
}
