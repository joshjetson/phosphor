//! Output headroom, checked by rendering the voicings people actually play.
//!
//! The defect this guards against: every instrument used to bound its output
//! with `clamp(-1.0, 1.0)` — a hard clip, which turns a chord into a square
//! wave and puts the odd harmonics that produces into the speakers. At
//! default parameters a three-note chord on the DX7 was already pinned at
//! full scale for one sample in two thousand, and an eight-note chord on the
//! loudest patch for one in nine.
//!
//! So the assertions here are, for every voicing:
//!
//! * no sample at or beyond full scale — not "few", none. This is the
//!   guarantee, and it holds for every preset at every velocity;
//! * peak at or under the master limiter's ceiling, which is the stronger
//!   statement that no single instrument can make the limiter act on its own.
//!   Gain reduction on the master bus should only ever mean "several loud
//!   tracks at once";
//! * for the presets the instruments load with, peak under the saturator's
//!   knee, which means the saturator did not engage at all and the render is
//!   the trimmed voice sum sample for sample.
//!
//! What is deliberately *not* asserted is that the loudest patch in a bank
//! also stays under the knee. Sizing the trims to that — an eight-note chord
//! at velocity 127 on the hottest preset — costs every note the user actually
//! plays 3.6 dB. The extremes are allowed to cross the knee; what is asserted
//! instead is how far, since the point of a soft knee is that a little of it
//! is inaudible and a lot of it is not.

use phosphor_dsp::level::{saturation_input, SATURATION_KNEE};
use phosphor_dsp::{drum_rack, dx7, juno, jupiter, odyssey, synth};
use phosphor_plugin::{MidiEvent, Plugin};

const SAMPLE_RATE: f64 = 44_100.0;
const BLOCK: usize = 256;
/// 1.16 s. Every peak measured across the four preset banks lands inside the
/// first 200 ms; this covers the attack, the decay and well into the sustain.
const BLOCKS: usize = 200;

/// Peak ceiling every instrument has to clear on its own, −1 dBFS.
///
/// The same value as `LIMITER_CEILING` in phosphor-core's mixer. It is
/// repeated rather than imported because phosphor-dsp does not depend on
/// phosphor-core and should not start to — the instruments have to be
/// testable without an engine around them. `mixer.rs` holds the two in step
/// with `limiter_idle_for_the_worst_single_track`, which drives the loudest
/// patch in the project through the real limiter and asserts its gain never
/// leaves unity.
const TARGET_PEAK: f32 = 0.891;

/// How much gain reduction the soft saturator is allowed to apply, in dB.
///
/// Above roughly this the curve stops being a rounded-off peak and starts
/// being a timbre change. The two voicings in the whole project that reach
/// the knee at all — the DX7's "Timpani" attack and a full drum kit on one
/// sample — measure 1.33 dB, so this leaves margin for a preset edit without
/// leaving room for a mis-sized trim to hide.
const MAX_SATURATION_DB: f32 = 2.0;

// The voicings. A chord in this application is quantised, so all the note-ons
// land on the same sample and the voices sum coherently — which is why the
// eight-note case is roughly eight times a single note rather than sqrt(8).
const SINGLE: &[u8] = &[60];
const TRIAD: &[u8] = &[60, 64, 67];
const SEVENTH: &[u8] = &[60, 64, 67, 71];
const WIDE_FIVE: &[u8] = &[36, 48, 60, 64, 67];
const TWO_HAND_EIGHT: &[u8] = &[36, 43, 48, 55, 60, 64, 67, 72];

const VOICINGS: &[(&str, &[u8])] = &[
    ("single", SINGLE),
    ("triad", TRIAD),
    ("7th", SEVENTH),
    ("wide 5-note", WIDE_FIVE),
    ("2-hand 8-note", TWO_HAND_EIGHT),
];

const VELOCITIES: &[u8] = &[100, 127];

struct Measured {
    peak: f32,
    at_full_scale: usize,
    non_finite: usize,
}

fn render(plugin: &mut dyn Plugin, notes: &[u8], velocity: u8, blocks: usize) -> Measured {
    plugin.init(SAMPLE_RATE, BLOCK);
    plugin.reset();

    let events: Vec<MidiEvent> = notes
        .iter()
        .map(|&note| MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: velocity })
        .collect();

    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut m = Measured { peak: 0.0, at_full_scale: 0, non_finite: 0 };

    for block in 0..blocks {
        left.fill(0.0);
        right.fill(0.0);
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        if block == 0 {
            plugin.process(&[], &mut outs, &events);
        } else {
            plugin.process(&[], &mut outs, &[]);
        }
        for sample in left.iter().chain(right.iter()).copied() {
            if !sample.is_finite() {
                m.non_finite += 1;
                continue;
            }
            let mag = sample.abs();
            if mag > m.peak {
                m.peak = mag;
            }
            if mag >= 1.0 {
                m.at_full_scale += 1;
            }
        }
    }
    m
}

fn patch_value(index: usize, count: usize) -> f32 {
    index as f32 / (count as f32 - 0.01)
}

/// The instruments, each with the patch that measured loudest in its bank on
/// an eight-note chord at velocity 127.
///
/// The DX7 entry is 47 "Timpani" — its attack transient reached 5.91 before
/// the trim, above 49 "Horns" at 5.40. Horns is the loudest *sustained*
/// patch, so both are covered here.
fn loudest_patches() -> Vec<(&'static str, Box<dyn Plugin>)> {
    let mut dx7_timpani = dx7::Dx7Synth::new();
    dx7_timpani.set_parameter(dx7::P_PATCH, patch_value(47, dx7::PATCH_COUNT));
    let mut dx7_horns = dx7::Dx7Synth::new();
    dx7_horns.set_parameter(dx7::P_PATCH, patch_value(49, dx7::PATCH_COUNT));
    let mut jupiter_organ = jupiter::Jupiter8Synth::new();
    jupiter_organ.set_parameter(jupiter::P_PATCH, patch_value(9, jupiter::PATCH_COUNT));
    let mut odyssey_funk = odyssey::OdysseySynth::new();
    odyssey_funk.set_parameter(odyssey::P_PATCH, patch_value(1, odyssey::PATCH_COUNT));
    let mut juno_brass = juno::Juno60Synth::new();
    juno_brass.set_parameter(juno::P_PATCH, patch_value(3, juno::PATCH_COUNT));

    vec![
        ("dx7 47 Timpani", Box::new(dx7_timpani)),
        ("dx7 49 Horns", Box::new(dx7_horns)),
        ("jupiter 9 Organ", Box::new(jupiter_organ)),
        ("odyssey 1 Funk", Box::new(odyssey_funk)),
        ("juno 3 Brass", Box::new(juno_brass)),
        ("phosphor", Box::new(synth::PhosphorSynth::new())),
    ]
}

fn defaults() -> Vec<(&'static str, Box<dyn Plugin>)> {
    vec![
        ("dx7 default", Box::new(dx7::Dx7Synth::new())),
        ("jupiter default", Box::new(jupiter::Jupiter8Synth::new())),
        ("odyssey default", Box::new(odyssey::OdysseySynth::new())),
        ("juno default", Box::new(juno::Juno60Synth::new())),
        ("phosphor default", Box::new(synth::PhosphorSynth::new())),
    ]
}

/// How much the soft saturator took off a measured peak, in dB. Zero when
/// the peak never reached the knee.
fn saturation_db(peak: f32) -> f32 {
    let raw = saturation_input(peak);
    if raw <= SATURATION_KNEE {
        0.0
    } else {
        20.0 * (raw / peak).log10()
    }
}

/// `transparent` asks for the stronger property: the render is bit-identical
/// to the trimmed voice sum, because the peak never reached the knee. Only
/// the presets the instruments load with are held to it.
fn check_all_voicings(cases: Vec<(&'static str, Box<dyn Plugin>)>, transparent: bool) {
    for (name, mut plugin) in cases {
        for (voicing, notes) in VOICINGS {
            for &velocity in VELOCITIES {
                let m = render(plugin.as_mut(), notes, velocity, BLOCKS);
                assert_eq!(
                    m.non_finite, 0,
                    "{name} {voicing} @{velocity}: {} non-finite samples",
                    m.non_finite
                );
                assert_eq!(
                    m.at_full_scale, 0,
                    "{name} {voicing} @{velocity}: {} samples pinned at full scale (peak {:.4})",
                    m.at_full_scale, m.peak
                );
                assert!(
                    m.peak <= TARGET_PEAK,
                    "{name} {voicing} @{velocity}: peak {:.4} is above the -1 dBFS ceiling, \
                     so this one instrument on its own would engage the master limiter",
                    m.peak
                );
                let reduction = saturation_db(m.peak);
                assert!(
                    reduction <= MAX_SATURATION_DB,
                    "{name} {voicing} @{velocity}: peak {:.4} took {reduction:.2} dB of \
                     saturation, past the point where the soft knee is inaudible",
                    m.peak
                );
                if transparent {
                    assert!(
                        m.peak < SATURATION_KNEE,
                        "{name} {voicing} @{velocity}: peak {:.4} reached the saturator knee \
                         on the preset the instrument loads with, so ordinary playing is no \
                         longer bit-identical to the trimmed voice sum",
                        m.peak
                    );
                }
            }
        }
    }
}

#[test]
fn loudest_patches_have_headroom_on_every_voicing() {
    check_all_voicings(loudest_patches(), false);
}

#[test]
fn default_patches_have_headroom_on_every_voicing() {
    check_all_voicings(defaults(), true);
}

/// The loudest patch is only loudest until someone edits the bank. This walks
/// every preset of every instrument on the worst voicing, so a patch added or
/// revoiced above the target is caught here rather than in a speaker.
#[test]
fn no_patch_in_any_bank_exceeds_the_target() {
    // Shorter render than the per-voicing tests: this is a sweep over 155
    // presets, and the latest peak measured anywhere in the four banks
    // arrives at 192 ms, so 370 ms leaves the attack and decay well covered.
    const SWEEP_BLOCKS: usize = 64;

    let mut worst = (0.0f32, String::new());

    for index in 0..dx7::PATCH_COUNT {
        let mut s = dx7::Dx7Synth::new();
        s.set_parameter(dx7::P_PATCH, patch_value(index, dx7::PATCH_COUNT));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "dx7", dx7::PATCH_NAMES[index], &mut worst);
    }
    for index in 0..jupiter::PATCH_COUNT {
        let mut s = jupiter::Jupiter8Synth::new();
        s.set_parameter(jupiter::P_PATCH, patch_value(index, jupiter::PATCH_COUNT));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "jupiter", jupiter::PATCH_NAMES[index], &mut worst);
    }
    for index in 0..odyssey::PATCH_COUNT {
        // Duophonic: a single note is this instrument's worst case, not a
        // chord, so it gets checked on both.
        for notes in [SINGLE, TWO_HAND_EIGHT] {
            let mut s = odyssey::OdysseySynth::new();
            s.set_parameter(odyssey::P_PATCH, patch_value(index, odyssey::PATCH_COUNT));
            let m = render(&mut s, notes, 127, SWEEP_BLOCKS);
            check_patch(&m, "odyssey", odyssey::PATCH_NAMES[index], &mut worst);
        }
    }
    for index in 0..juno::PATCH_COUNT {
        let mut s = juno::Juno60Synth::new();
        s.set_parameter(juno::P_PATCH, patch_value(index, juno::PATCH_COUNT));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "juno", juno::PATCH_NAMES[index], &mut worst);
    }
    {
        let mut s = synth::PhosphorSynth::new();
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "phosphor", "default", &mut worst);
    }

    // The banks should not all be so quiet that the ceiling is meaningless.
    // This is the assertion that fails if someone "fixes" a level problem by
    // trimming everything down again: the loudest thing the project can
    // produce has to actually use the headroom it is given.
    assert!(
        worst.0 > 0.5,
        "nothing in any bank reaches half of full scale ({}, {:.4}) — \
         the trims are too deep to be useful",
        worst.1, worst.0
    );
}

fn check_patch(m: &Measured, synth: &str, patch: &str, worst: &mut (f32, String)) {
    assert_eq!(m.non_finite, 0, "{synth} {patch}: {} non-finite samples", m.non_finite);
    assert_eq!(
        m.at_full_scale, 0,
        "{synth} {patch}: {} samples pinned at full scale",
        m.at_full_scale
    );
    assert!(
        m.peak <= TARGET_PEAK,
        "{synth} {patch}: peak {:.4} is above the -1 dBFS ceiling, so this one \
         instrument on its own would engage the master limiter",
        m.peak
    );
    let reduction = saturation_db(m.peak);
    assert!(
        reduction <= MAX_SATURATION_DB,
        "{synth} {patch}: peak {:.4} took {reduction:.2} dB of saturation, past the \
         point where the soft knee is inaudible",
        m.peak
    );
    if m.peak > worst.0 {
        *worst = (m.peak, format!("{synth} {patch}"));
    }
}

/// The drum rack is the sixth instrument and had the same defect as the
/// phosphor synth: no output bound at all. Eight pads struck on one sample is
/// its worst case, and it has to clear the same target as the keyboards.
#[test]
fn no_drum_kit_exceeds_the_target() {
    // Kick, snare, two toms, closed and open hat, crash, ride — a full kit
    // hit on the same sample, which is what a quantised fill lands as.
    const FULL_KIT: &[u8] = &[36, 38, 41, 42, 45, 46, 49, 51];
    const KIT_COUNT: usize = 10;

    let mut worst = (0.0f32, String::new());
    for kit in 0..KIT_COUNT {
        for notes in [&[36u8][..], FULL_KIT] {
            let mut rack = drum_rack::DrumRack::new();
            rack.set_parameter(drum_rack::P_KIT, kit as f32 / (KIT_COUNT as f32 - 0.01));
            let m = render(&mut rack, notes, 127, BLOCKS);
            check_patch(&m, "drum rack", &format!("kit {kit}"), &mut worst);
        }
    }
    assert!(worst.0 > 0.5, "no kit reaches half of full scale: {worst:?}");
}

/// The other half of the guarantee, and the one that was missing.
///
/// Bounding the output is easy to get right by making everything quiet, and
/// that is what the first pass at this did: the trims were sized around an
/// eight-note chord at velocity 127 on the hottest preset in each bank, so
/// the notes a user actually plays came out 15 dB below where they belonged
/// and the only fader left in the signal path was the operating system's.
///
/// Two assertions, on the preset each instrument loads with and a triad at
/// velocity 100:
///
/// * a floor on RMS, which is what the ear tracks and the only number
///   comparable across instruments whose crest factors differ by 10 dB;
/// * a ceiling on peak, so "loud enough" cannot be met by driving everything
///   into the saturator.
///
/// The floor is a regression guard, not a derived quantity — the measured
/// values are 0.0143 (DX7, the tightest), 0.0149, 0.0166, 0.0191, 0.0298 and
/// 0.0448 — so it sits about 2 dB under the quietest. Enough margin that a
/// preset edit does not trip it, not enough that another 3.6 dB of trim could
/// hide beneath it.
#[test]
fn ordinary_playing_is_at_a_usable_level() {
    /// −38.8 dBFS RMS.
    const RMS_FLOOR: f32 = 0.0115;
    /// −6 dBFS peak. Above this one instrument is using half the headroom of
    /// the master bus before a second track has been added.
    const PEAK_CEILING: f32 = 0.5;

    /// The drum rack has no C major triad; a downbeat is its equivalent.
    const DOWNBEAT: &[u8] = &[36, 38, 42];

    let cases: Vec<(&str, &[u8], Box<dyn Plugin>)> = vec![
        ("dx7", TRIAD, Box::new(dx7::Dx7Synth::new())),
        ("jupiter", TRIAD, Box::new(jupiter::Jupiter8Synth::new())),
        ("odyssey", TRIAD, Box::new(odyssey::OdysseySynth::new())),
        ("juno", TRIAD, Box::new(juno::Juno60Synth::new())),
        ("phosphor", TRIAD, Box::new(synth::PhosphorSynth::new())),
        ("drum rack", DOWNBEAT, Box::new(drum_rack::DrumRack::new())),
    ];

    for (name, notes, mut plugin) in cases {
        let (peak, rms) = peak_and_rms(plugin.as_mut(), notes, 100);
        assert!(
            rms >= RMS_FLOOR,
            "{name}: ordinary playing sits at {rms:.5} RMS ({:.1} dBFS), too quiet \
             to use without makeup gain",
            20.0 * rms.log10()
        );
        assert!(
            peak <= PEAK_CEILING,
            "{name}: ordinary playing peaks at {peak:.4} ({:.1} dBFS), which leaves \
             no room for a second track",
            20.0 * peak.log10()
        );
    }
}

/// The DX7 is the reference instrument for the gain structure — the trims of
/// the other five are set relative to it — so the one number the whole
/// structure hangs from gets pinned here rather than left to drift.
///
/// A ±2 dB window around −12 dBFS: wide enough that a preset edit or a
/// rounded trim constant does not trip it, narrow enough that the 3.6 dB
/// re-target this test was written for could not happen silently.
#[test]
fn dx7_default_triad_hits_the_minus_twelve_dbfs_target() {
    let (peak, _) = peak_and_rms(&mut dx7::Dx7Synth::new(), TRIAD, 100);
    let dbfs = 20.0 * peak.log10();
    assert!(
        (-14.0..=-10.0).contains(&dbfs),
        "DX7 default preset, triad @100, peaks at {peak:.4} ({dbfs:.1} dBFS); \
         the target for ordinary playing is -12 dBFS"
    );
}

/// Peak and RMS of one render. RMS over the left channel only, matching
/// `instruments_are_level_matched` and the `levels` example, so the three
/// report the same number for the same input.
fn peak_and_rms(plugin: &mut dyn Plugin, notes: &[u8], velocity: u8) -> (f32, f32) {
    plugin.init(SAMPLE_RATE, BLOCK);
    plugin.reset();
    let events: Vec<MidiEvent> = notes
        .iter()
        .map(|&note| MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: velocity })
        .collect();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut peak = 0.0f32;
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for block in 0..BLOCKS {
        left.fill(0.0);
        right.fill(0.0);
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        if block == 0 {
            plugin.process(&[], &mut outs, &events);
        } else {
            plugin.process(&[], &mut outs, &[]);
        }
        for s in &left {
            peak = peak.max(s.abs());
            sum += f64::from(*s) * f64::from(*s);
            n += 1;
        }
    }
    (peak, (sum / n as f64).sqrt() as f32)
}

/// Every instrument at the same typical loudness, so switching between them
/// does not step the level. Measured as RMS of a triad, which is what the ear
/// tracks, rather than peak, which a single transient can dominate.
#[test]
fn instruments_are_level_matched() {
    fn triad_rms(plugin: &mut dyn Plugin) -> f32 {
        plugin.init(SAMPLE_RATE, BLOCK);
        plugin.reset();
        let events: Vec<MidiEvent> = TRIAD
            .iter()
            .map(|&note| MidiEvent { sample_offset: 0, status: 0x90, data1: note, data2: 100 })
            .collect();
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let mut sum = 0.0f64;
        let mut n = 0u64;
        for block in 0..BLOCKS {
            left.fill(0.0);
            right.fill(0.0);
            let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
            if block == 0 {
                plugin.process(&[], &mut outs, &events);
            } else {
                plugin.process(&[], &mut outs, &[]);
            }
            for s in &left {
                sum += f64::from(*s) * f64::from(*s);
                n += 1;
            }
        }
        (sum / n as f64).sqrt() as f32
    }

    // The median preset of each bank, so one unusual patch cannot skew the
    // comparison. Indices picked by measuring the triad RMS of every preset.
    let mut dx7_mid = dx7::Dx7Synth::new();
    dx7_mid.set_parameter(dx7::P_PATCH, patch_value(2, dx7::PATCH_COUNT));
    let mut jupiter_mid = jupiter::Jupiter8Synth::new();
    jupiter_mid.set_parameter(jupiter::P_PATCH, patch_value(1, jupiter::PATCH_COUNT));
    let mut odyssey_mid = odyssey::OdysseySynth::new();
    odyssey_mid.set_parameter(odyssey::P_PATCH, patch_value(4, odyssey::PATCH_COUNT));
    let mut juno_mid = juno::Juno60Synth::new();
    juno_mid.set_parameter(juno::P_PATCH, patch_value(1, juno::PATCH_COUNT));

    let levels = [
        ("dx7", triad_rms(&mut dx7_mid)),
        ("jupiter", triad_rms(&mut jupiter_mid)),
        ("odyssey", triad_rms(&mut odyssey_mid)),
        ("juno", triad_rms(&mut juno_mid)),
        ("phosphor", triad_rms(&mut synth::PhosphorSynth::new())),
    ];

    let quietest = levels.iter().map(|(_, v)| *v).fold(f32::MAX, f32::min);
    let loudest = levels.iter().map(|(_, v)| *v).fold(0.0f32, f32::max);
    assert!(quietest > 0.0, "an instrument rendered silence: {levels:?}");
    let spread_db = 20.0 * (loudest / quietest).log10();
    assert!(
        spread_db < 9.0,
        "instruments are {spread_db:.1} dB apart, which is an audible jump \
         when switching: {levels:?}"
    );
}
