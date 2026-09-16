//! How a value reads on a panel.
//!
//! One spelling per unit, shared by everything that prints it: the strip's
//! pan field, an effect's readout and a pad's knob all say `L50` for the
//! same number. Two panels formatting the same value differently is how one
//! of them ends up wrong, and a player learning two dialects for one unit is
//! worse than either.

/// A pan position: `C` in the middle, `L50` / `R50` either side, in whole
/// percent of the travel.
#[must_use]
pub fn pan_label(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round() as i32;
    if amount == 0 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("L{amount}")
    } else {
        format!("R{amount}")
    }
}

/// A linear gain in decibels — `0 dB` at unity, `+6 dB` above it, `-oo` at
/// silence.
///
/// The fader's spelling, because the fader is where a player learns it. The
/// value is rounded to the whole decibel it is stepped in: a readout with
/// more precision than the control has reads as a knob that cannot be put
/// back where it was.
#[must_use]
pub fn db_text(gain: f32) -> String {
    if gain <= 0.0 {
        return "-oo".to_string();
    }
    match (20.0 * gain.log10()).round() as i32 {
        0 => "0 dB".to_string(),
        db if db <= -100 => "-oo".to_string(),
        db => format!("{db:+} dB"),
    }
}

/// A time in the unit it is easiest to read: milliseconds up to a second,
/// seconds above it.
#[must_use]
pub fn ms_text(ms: f32) -> String {
    if ms < 1000.0 {
        format!("{} ms", ms.round() as i32)
    } else {
        format!("{:.2} s", ms / 1000.0)
    }
}

/// A note's name on a keyboard — `C3`, `A#1`. Octaves numbered so that MIDI
/// 60 is C3, matching the chord device's split labelling and the sampler's
/// pad names.
#[must_use]
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(note) / 12 - 2;
    format!("{}{}", NAMES[usize::from(note) % 12], octave)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_of_the_pan_control_has_a_name_of_its_own() {
        assert_eq!(pan_label(0.0), "C");
        // A position that rounds to nothing is still the centre, not `L0`.
        assert_eq!(pan_label(-0.001), "C");
        assert_eq!(pan_label(-0.5), "L50");
        assert_eq!(pan_label(1.0), "R100");
    }

    #[test]
    fn silence_reads_as_silence_and_unity_carries_no_sign() {
        assert_eq!(db_text(0.0), "-oo");
        assert_eq!(db_text(-1.0), "-oo", "a negative gain is not a level");
        assert_eq!(db_text(1.0), "0 dB");
        assert!(db_text(2.0).starts_with("+6"), "{}", db_text(2.0));
        assert!(db_text(0.5).starts_with("-6"), "{}", db_text(0.5));
        // Far enough down that the decibel is meaningless.
        assert_eq!(db_text(1.0e-9), "-oo");
    }

    #[test]
    fn a_time_changes_unit_at_the_second() {
        assert_eq!(ms_text(0.0), "0 ms");
        assert_eq!(ms_text(999.4), "999 ms");
        assert_eq!(ms_text(1000.0), "1.00 s");
        assert_eq!(ms_text(2500.0), "2.50 s");
    }

    #[test]
    fn note_names_read_like_a_keyboard() {
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(21), "A-1");
        assert_eq!(note_name(108), "C7");
        assert_eq!(note_name(127), "G8");
    }
}
