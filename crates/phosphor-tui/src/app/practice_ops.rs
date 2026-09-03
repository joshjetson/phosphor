//! App methods: the practice room — open, run, listen, click.
//!
//! The room's brain lives in `phosphor_app::practice`; this file is its
//! hands: the metronome commands, the MIDI ear, the progress file, the
//! keys. Sound never comes through here — the controller already reaches
//! the selected track's synth on the audio thread, which is the whole
//! point of practising inside the instrument you chose.

use super::*;
use phosphor_app::practice::{judge, ClickMode, Family, Hands, RoomEvent};

impl App {
    /// spc+f: open the room over the selected instrument track.
    pub(crate) fn open_practice(&mut self) {
        let has_instrument = self
            .nav
            .current_track()
            .is_some_and(|t| t.mixer_id.is_some() && t.sequencer.is_none());
        if !has_instrument {
            self.flash("fingers wants an instrument track \u{2014} pick a sound first");
            return;
        }
        let progress = phosphor_app::practice::progress::load();
        self.nav.practice.open_with(progress);
        self.flash("fingers \u{00b7} pick a drill, enter starts it");
    }

    pub(crate) fn close_practice(&mut self) {
        self.stop_practice_run();
        if self.nav.practice.progress_dirty {
            if phosphor_app::practice::progress::save(&self.nav.practice.progress).is_ok() {
                self.nav.practice.progress_dirty = false;
            }
        }
        self.nav.practice.close();
    }

    fn start_practice_run(&mut self) {
        let now = phosphor_midi::clock::now_micros();
        self.nav.practice.start(now);
        self.arm_practice_click();
    }

    fn stop_practice_run(&mut self) {
        self.nav.practice.stop();
        let _ = self
            .engine
            .shared
            .mixer_command_tx
            .send(MixerCommand::SetPracticeClick { bpm: 0.0, pattern: 0 });
    }

    fn arm_practice_click(&mut self) {
        let click = self.nav.practice.click;
        let bpm = self.nav.practice.start_bpm();
        let running = self.nav.practice.run.is_some();
        let cmd = if running && click != ClickMode::Off && self.nav.practice.mode == judge::Mode::Flow
        {
            MixerCommand::SetPracticeClick { bpm: f64::from(bpm), pattern: click.pattern() }
        } else {
            MixerCommand::SetPracticeClick { bpm: 0.0, pattern: 0 }
        };
        let _ = self.engine.shared.mixer_command_tx.send(cmd);
    }

    /// Every frame while the room is open.
    pub(crate) fn tick_practice(&mut self) {
        if !self.nav.practice.open || self.nav.practice.run.is_none() {
            return;
        }
        let now = phosphor_midi::clock::now_micros();
        let events = self.nav.practice.tick(now);
        for event in events {
            match event {
                RoomEvent::BpmUp => {
                    let bpm = self.nav.practice.start_bpm();
                    self.arm_practice_click();
                    self.flash(format!("three clean \u{2014} \u{2669}={bpm}"));
                }
                RoomEvent::RepDone => {}
            }
        }
    }

    /// The room's keys. Returns true when the key was taken.
    pub(crate) fn handle_practice_keys(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        if !self.nav.practice.open {
            return false;
        }
        let running = self.nav.practice.run.is_some();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if running {
                    self.stop_practice_run();
                } else {
                    self.close_practice();
                }
            }
            KeyCode::Enter => {
                if running {
                    self.stop_practice_run();
                } else {
                    self.start_practice_run();
                }
            }
            KeyCode::Char('j') | KeyCode::Down if !running => {
                let max = Family::ALL.len() - 1;
                self.nav.practice.cursor = (self.nav.practice.cursor + 1).min(max);
                self.nav.practice.bpm = None;
            }
            KeyCode::Char('k') | KeyCode::Up if !running => {
                self.nav.practice.cursor = self.nav.practice.cursor.saturating_sub(1);
                self.nav.practice.bpm = None;
            }
            KeyCode::Char('>') | KeyCode::Char('.') if self.nav.practice.family().keyed() => {
                self.nav.practice.key_pos = (self.nav.practice.key_pos + 1) % 12;
                self.nav.practice.bpm = None;
                if running {
                    self.start_practice_run();
                }
            }
            KeyCode::Char('<') | KeyCode::Char(',') if self.nav.practice.family().keyed() => {
                self.nav.practice.key_pos = (self.nav.practice.key_pos + 11) % 12;
                self.nav.practice.bpm = None;
                if running {
                    self.start_practice_run();
                }
            }
            KeyCode::Char('h') if self.nav.practice.family().handed() => {
                let next = match self.nav.practice.hands {
                    Hands::Right => Hands::Left,
                    Hands::Left => Hands::Together,
                    Hands::Together => Hands::Right,
                };
                self.nav.practice.hands = next;
                self.nav.practice.bpm = None;
                if running {
                    self.start_practice_run();
                }
            }
            KeyCode::Char('w') => {
                self.nav.practice.mode = match self.nav.practice.mode {
                    judge::Mode::Wait => judge::Mode::Flow,
                    judge::Mode::Flow => judge::Mode::Wait,
                };
                if running {
                    self.start_practice_run();
                }
            }
            KeyCode::Char('c') => {
                self.nav.practice.click = match self.nav.practice.click {
                    ClickMode::Off => ClickMode::AllBeats,
                    ClickMode::AllBeats => ClickMode::TwoAndFour,
                    ClickMode::TwoAndFour => ClickMode::Off,
                };
                self.arm_practice_click();
            }
            KeyCode::Char(']') => {
                let bpm = self.nav.practice.start_bpm().saturating_add(5).min(300);
                self.nav.practice.bpm = Some(bpm);
                self.arm_practice_click();
                if running {
                    self.start_practice_run();
                }
            }
            KeyCode::Char('[') => {
                let bpm = self.nav.practice.start_bpm().saturating_sub(5).max(30);
                self.nav.practice.bpm = Some(bpm);
                self.arm_practice_click();
                if running {
                    self.start_practice_run();
                }
            }
            _ => return false,
        }
        true
    }
}
