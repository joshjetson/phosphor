//! Journeys through copy and paste on the sampler: a pad whole, one sound
//! off it, across keys and across kits — and every paste undone and redone.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crossterm::event::KeyCode;
    use phosphor_app::sampler::SamplerState;
    use phosphor_core::mixer::MixerCommand;
    use phosphor_core::EngineConfig;
    use phosphor_plugin::sample::TrigMode;

    use crate::app::App;
    use crate::state::*;
    use crate::test_support::{press, press_ctrl, press_shift, screen, type_line};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phosphor-copy-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_wav(path: &Path) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..4_410 {
            let s = (std::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin();
            w.write_sample((s * 20_000.0) as i32).unwrap();
        }
        w.finalize().unwrap();
    }

    fn sampler_track(app: &mut App) {
        app.create_instrument_track(InstrumentType::Sampler);
        app.nav.focused_pane = Pane::ClipView;
    }

    fn load(app: &mut App, path: &Path) {
        press(app, KeyCode::Char('a'));
        press(app, KeyCode::Char('/'));
        type_line(app, &path.display().to_string());
        press(app, KeyCode::Enter);
    }

    /// A sampler with two sounds on C4 and its settings dressed.
    fn dressed(dir: &Path) -> App {
        let (kick, click) = (dir.join("kick.wav"), dir.join("click.wav"));
        write_wav(&kick);
        write_wav(&click);
        let mut app = App::new(EngineConfig { buffer_size: 64, sample_rate: 44100 }, false, false);
        app.browse_samples = Some(dir.to_path_buf());
        sampler_track(&mut app);
        app.sampler_follow_note(60);
        load(&mut app, &kick);
        load(&mut app, &click);
        let track = app.nav.track_cursor;
        let kit = app.nav.tracks[track].sampler.as_mut().unwrap();
        kit.pads[pad(60)].config.trig = TrigMode::Gate;
        kit.pads[pad(60)].config.choke = 2;
        let _ = app.drain_mixer_commands();
        app
    }

    fn pad(note: u8) -> usize {
        SamplerState::pad_of_note(note).unwrap()
    }

    fn kit(app: &App) -> &SamplerState {
        app.nav.tracks[app.nav.track_cursor].sampler.as_deref().unwrap()
    }

    fn flash(app: &App) -> String {
        app.status_message.as_ref().map(|(m, _)| m.clone()).unwrap_or_default()
    }

    fn synced(commands: &[MixerCommand], target: usize) -> bool {
        commands.iter().any(|c| match c {
            MixerCommand::SetSamplerPad { pad, layers, .. } => usize::from(*pad) == target && !layers.is_empty(),
            MixerCommand::SetSamplerRange { .. } => true,
            _ => false,
        })
    }

    /// The owner's question: one sound on a key, onto another key. `y`
    /// copies it with everything it is, `p` lands it, one `u` takes it off
    /// and `Ctrl+R` puts it back.
    #[test]
    fn y_then_p_copies_a_pad_to_another_key_and_undo_and_redo_follow() {
        let dir = scratch("pad");
        let mut app = dressed(&dir);
        press(&mut app, KeyCode::Char('y'));
        assert!(flash(&app).contains("pad C4 copied"), "{}", flash(&app));

        app.sampler_follow_note(64);
        press(&mut app, KeyCode::Char('p'));
        assert!(flash(&app).contains("pad C4 pasted on pad E4"), "{}", flash(&app));
        let (from, to) = (&kit(&app).pads[pad(60)], &kit(&app).pads[pad(64)]);
        assert_eq!(to, from, "E4 is not a copy of C4");
        assert!(Arc::ptr_eq(from.layers[0].pcm.as_ref().unwrap(), to.layers[0].pcm.as_ref().unwrap()));
        assert!(synced(&app.drain_mixer_commands(), pad(64)), "the engine was not told");

        press(&mut app, KeyCode::Char('u'));
        assert!(kit(&app).pads[pad(64)].layers.is_empty(), "u left the paste");
        assert_eq!(kit(&app).pads[pad(64)].config.choke, 0);
        press_ctrl(&mut app, 'r');
        assert_eq!(kit(&app).pads[pad(64)], kit(&app).pads[pad(60)], "redo lost the paste");
    }

    #[test]
    fn capital_y_copies_one_sound_and_p_stacks_it() {
        let dir = scratch("sound");
        let mut app = dressed(&dir);
        press(&mut app, KeyCode::Char(']')); // the second sound: click
        press_shift(&mut app, 'Y');
        assert!(flash(&app).contains("click from pad C4 copied"), "{}", flash(&app));
        app.sampler_follow_note(62);
        let snare = dir.join("snare.wav");
        write_wav(&snare);
        load(&mut app, &snare);
        press(&mut app, KeyCode::Char('p'));
        let names: Vec<_> = kit(&app).pads[pad(62)].layers.iter().map(|l| l.name.clone()).collect();
        assert_eq!(names, ["snare", "click"]);
        assert_eq!(app.nav.clip_view.sampler.layer, 1, "the cursor is not on what arrived");
        assert_eq!(kit(&app).pads[pad(62)].config.trig, TrigMode::OneShot, "a sound brought its pad's settings");
        press(&mut app, KeyCode::Char('u'));
        assert_eq!(kit(&app).pads[pad(62)].layers.len(), 1, "u did not take just the pasted sound");
    }

    /// The clipboard outlives the track: a pad copied on one sampler pastes
    /// onto another.
    #[test]
    fn a_pad_pastes_into_another_sampler_track() {
        let dir = scratch("across");
        let mut app = dressed(&dir);
        press(&mut app, KeyCode::Char('y'));
        let original = kit(&app).pads[pad(60)].clone();
        sampler_track(&mut app);
        app.sampler_follow_note(48);
        press(&mut app, KeyCode::Char('p'));
        assert_eq!(kit(&app).pads[pad(48)], original);
    }

    #[test]
    fn nothing_copied_and_nothing_to_copy_say_so_and_change_nothing() {
        let dir = scratch("nothing");
        let mut app = dressed(&dir);
        let before = kit(&app).clone();
        press(&mut app, KeyCode::Char('p'));
        assert!(flash(&app).contains("nothing copied"), "{}", flash(&app));
        assert_eq!(kit(&app), &before);
        app.sampler_follow_note(70);
        press(&mut app, KeyCode::Char('y'));
        assert!(flash(&app).contains("nothing on pad A#4 to copy"), "{}", flash(&app));
        assert!(app.sampler_clip.is_none(), "an empty pad filled the clipboard");
    }

    #[test]
    fn the_hint_bar_offers_copy() {
        let dir = scratch("hint");
        let mut app = dressed(&dir);
        // A flash covers the hints until it times out; this is after.
        app.status_message = None;
        let shown = screen(&app, 170, 44);
        assert!(shown.contains("y/p\u{00b7}copy"), "{}", shown.lines().last().unwrap_or(""));
    }
}
