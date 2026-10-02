//! What the LEDs show.
//!
//! Each LED has a level, 0 to 127, and a colour that belongs to its control.
//! Phosphor sets levels by sending notes back on the deck's channel (a note
//! on at velocity 0-127 for the control that sends that note); until it
//! does, the deck lights its own pads as they are played, and lights the
//! last button picked in each set of choices (the function targets, the
//! track modes, the step halves).

use crate::table::{GROUPS, LIGHTS, LIGHT_COLOR, LIGHT_OF_NOTE};
use crate::Rgb;

/// The brightness cap, out of 255: a quarter, which keeps the whole chain
/// inside a USB port's 500 mA.
pub const CAP: u16 = 64;

pub struct Lights {
    level: [u8; LIGHTS],
}

impl Default for Lights {
    fn default() -> Self {
        Self { level: [0; LIGHTS] }
    }
}

impl Lights {
    pub fn set(&mut self, light: u8, level: u8) {
        if let Some(l) = self.level.get_mut(usize::from(light)) {
            *l = level.min(127);
        }
    }

    pub fn level(&self, light: u8) -> u8 {
        self.level.get(usize::from(light)).copied().unwrap_or(0)
    }

    /// The control sending `note` was told to show `level`.
    pub fn set_note(&mut self, note: u8, level: u8) {
        if let Some(&light) = LIGHT_OF_NOTE.get(usize::from(note)) {
            if light != 255 {
                self.set(light, level);
            }
        }
    }

    /// A button in one of the sets was picked: it lights, the rest go dark.
    pub fn pick(&mut self, light: u8) {
        if let Some(group) = GROUPS.iter().find(|g| g.contains(&light)) {
            for &member in group.iter() {
                self.set(member, if member == light { 127 } else { 0 });
            }
        }
    }

    /// The colours to send down the chain, capped.
    pub fn frame(&self) -> [Rgb; LIGHTS] {
        let mut out = [Rgb::default(); LIGHTS];
        for (i, rgb) in out.iter_mut().enumerate() {
            let level = u16::from(self.level[i]);
            let scale = |c: u8| (u16::from(c) * level / 127 * CAP / 255) as u8;
            let color = LIGHT_COLOR[i];
            *rgb = Rgb { r: scale(color.r), g: scale(color.g), b: scale(color.b) };
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_one_of_a_set_turns_the_others_off() {
        let mut l = Lights::default();
        let set = GROUPS[0];
        l.pick(set[0]);
        l.pick(set[1]);
        assert_eq!(l.level(set[0]), 0);
        assert_eq!(l.level(set[1]), 127);
    }

    #[test]
    fn nothing_ever_goes_out_brighter_than_the_cap() {
        let mut l = Lights::default();
        for i in 0..LIGHTS as u8 {
            l.set(i, 127);
        }
        for rgb in l.frame() {
            assert!(u16::from(rgb.r.max(rgb.g).max(rgb.b)) <= CAP);
        }
    }
}
