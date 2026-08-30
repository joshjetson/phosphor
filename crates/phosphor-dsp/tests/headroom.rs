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
//!
//! The DX7 is the exception, and it is the factory bank that made it one: its
//! loudest ROM voice sits 10 dB above the bank's median where the hand-voiced
//! bank it replaced managed 6, so its trim *is* sized by its hottest voice —
//! see `OUTPUT_TRIM` in dx7.rs. Sixteen voices would otherwise cross the
//! ceiling by up to 0.5 dB, and a peak past the ceiling on one track ducks
//! every other track with it.

use phosphor_dsp::level::{saturation_input, SATURATION_KNEE};
use phosphor_dsp::{
    drum_rack, dx7, juno, jupiter, odyssey, phatty, prophet6, rhodes, synth, teo5,
};
use phosphor_plugin::{MidiEvent, Plugin};

const SAMPLE_RATE: f64 = 44_100.0;
const BLOCK: usize = 256;
/// The factory voice whose triad RMS is the median of the DX7's 256: ROM1A's
/// PIANO 2. Re-derive with `cargo run --example levels -- stage` after the
/// bank or the trim moves.
const DX7_MEDIAN_VOICE: usize = 8;
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
/// the knee at all — ROM3A's TIMPANI on the DX7 and a full drum kit on one
/// sample — measure 1.44 and 1.33 dB, so this leaves margin for a preset edit
/// without leaving room for a mis-sized trim to hide.
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
    /// Left channel only, matching `peak_and_rms` and the `levels` example so
    /// that the three report the same number for the same input.
    rms: f32,
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
    let mut m = Measured { peak: 0.0, rms: 0.0, at_full_scale: 0, non_finite: 0 };
    let mut square_sum = 0.0f64;

    for block in 0..blocks {
        left.fill(0.0);
        right.fill(0.0);
        let mut outs: [&mut [f32]; 2] = [&mut left, &mut right];
        if block == 0 {
            plugin.process(&[], &mut outs, &events);
        } else {
            plugin.process(&[], &mut outs, &[]);
        }
        for sample in left.iter().copied() {
            if sample.is_finite() {
                square_sum += f64::from(sample) * f64::from(sample);
            }
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
    m.rms = (square_sum / (blocks * BLOCK) as f64).sqrt() as f32;
    m
}

/// Point a DX7 at one of the 256 factory voices by number. The instrument has
/// two selectors — a cartridge and a voice — so a sweep index is split.
fn dx7_voice(index: usize) -> dx7::Dx7Synth {
    let mut synth = dx7::Dx7Synth::new();
    let (bank, patch) = dx7::voice_knobs(index);
    synth.set_parameter(dx7::P_BANK, bank);
    synth.set_parameter(dx7::P_PATCH, patch);
    synth
}

/// The instruments, each with the patch that measured loudest in its bank on
/// an eight-note chord at velocity 127.
///
/// The DX7 entries are the two loudest of the 256 factory voices: ROM3A's
/// TIMPANI (voice 147) and ROM1B's HARP 1 (voice 60), whose attack transients
/// reach 1.65 and 1.62 before the bounding stage. ROM4A's CLARINET (voice 195)
/// is the loudest *sustained* voice, so it is here too.
///
/// The Juno's is 44 TUBA, the loudest of its 56 factory patches: 16' saw with
/// the filter wide open, which is as much of the DCO as that panel can put
/// through at once.
///
/// The Jupiter's is 61 STARTING UP, the loudest of its 64 factory patches: a
/// square LFO chopping a resonant filter over a sawtooth and noise together.
/// It is a sound effect, and it is the one patch in that bank whose peak
/// arrives after the render window the per-voicing tests use — see
/// `the_jupiters_effects_peak_after_the_other_banks_have_finished`, which is
/// where its real number is. 45 PIPE ORGAN is here too, because it is the
/// loudest thing in the bank that a player would hold a chord on.
///
/// The Rhodes' is Hard Bark, the loudest of its 26: the tine voiced onto the
/// pickup axis and struck at the top of the STRIKE knob, which is as far into
/// the pickup's nonlinearity as that panel goes.
/// Point a Prophet-6 at one of the 500 factory programs by number. Two
/// selectors, a bank and a program, so a sweep index is split.
fn p6_program(index: usize) -> prophet6::Prophet6 {
    let mut synth = prophet6::Prophet6::new();
    let (bank, program) = prophet6::program_knobs(index);
    synth.set_parameter(prophet6::P_BANK, bank);
    synth.set_parameter(prophet6::P_PROGRAM, program);
    synth
}

/// Point a TEO-5 at one of the 256 factory programs by number. Two
/// selectors, a bank and a program, so a sweep index is split.
fn teo5_program(index: usize) -> teo5::Teo5 {
    let mut synth = teo5::Teo5::new();
    let (bank, program) = teo5::program_knobs(index);
    synth.set_parameter(teo5::P_BANK, bank);
    synth.set_parameter(teo5::P_PROGRAM, program);
    synth
}

fn loudest_patches() -> Vec<(&'static str, Box<dyn Plugin>)> {
    let dx7_timpani = dx7_voice(147);
    let dx7_harp = dx7_voice(60);
    let dx7_clarinet = dx7_voice(195);
    let mut jupiter_startup = jupiter::Jupiter8Synth::new();
    jupiter_startup.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(40));
    let mut jupiter_organ = jupiter::Jupiter8Synth::new();
    jupiter_organ.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(28));
    let mut odyssey_funk = odyssey::OdysseySynth::new();
    odyssey_funk.set_parameter(odyssey::P_PATCH, odyssey::patch_knob(1));
    let mut juno_tuba = juno::Juno60Synth::new();
    juno_tuba.set_parameter(juno::P_PATCH, juno::patch_knob(27));
    let mut rhodes_bark = rhodes::RhodesPiano::new();
    rhodes_bark.set_parameter(rhodes::P_PATCH, rhodes::patch_knob(22));
    let mut phatty_blip = phatty::LittlePhatty::new();
    phatty_blip.set_parameter(phatty::P_PATCH, phatty::patch_knob(85));
    // The Prophet-6's is 285 Genesis 2, the loudest of its 500 factory
    // programs on an eight-note chord: a tremolo at 0.09 Hz over both filters
    // at maximum resonance, so its peak arrives eleven seconds into a held
    // chord rather than in its attack. 031 Hi Hat is the loudest whose peak
    // is in the first tenth of a second — white noise at full through a
    // wide-open four-pole and a high-pass at maximum resonance.
    let p6_genesis = p6_program(285);
    let p6_hat = p6_program(31);
    // The TEO-5's is 123 Trancendance, the loudest of its 256 factory
    // programs on an eight-note chord: both oscillators on all three
    // waveshapes with the sub underneath and the filter wide open.
    let teo5_loudest = teo5_program(123);

    vec![
        ("dx7 147 TIMPANI", Box::new(dx7_timpani)),
        ("dx7 60 HARP 1", Box::new(dx7_harp)),
        ("dx7 195 CLARINET", Box::new(dx7_clarinet)),
        ("jupiter 61 START UP", Box::new(jupiter_startup)),
        ("jupiter 45 PIPE ORGN", Box::new(jupiter_organ)),
        ("odyssey 1 Funk", Box::new(odyssey_funk)),
        ("juno 44 TUBA", Box::new(juno_tuba)),
        ("rhodes Hard Bark", Box::new(rhodes_bark)),
        ("phatty Blip", Box::new(phatty_blip)),
        ("prophet6 285 Genesis 2", Box::new(p6_genesis)),
        ("prophet6 031 Hi Hat", Box::new(p6_hat)),
        ("teo5 123 Trancendance", Box::new(teo5_loudest)),
        ("phosphor", Box::new(synth::PhosphorSynth::new())),
    ]
}

fn defaults() -> Vec<(&'static str, Box<dyn Plugin>)> {
    vec![
        ("dx7 default", Box::new(dx7::Dx7Synth::new())),
        ("jupiter default", Box::new(jupiter::Jupiter8Synth::new())),
        ("odyssey default", Box::new(odyssey::OdysseySynth::new())),
        ("juno default", Box::new(juno::Juno60Synth::new())),
        ("rhodes default", Box::new(rhodes::RhodesPiano::new())),
        ("phatty default", Box::new(phatty::LittlePhatty::new())),
        ("prophet6 default", Box::new(prophet6::Prophet6::new())),
        ("teo5 default", Box::new(teo5::Teo5::new())),
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
    // Shorter render than the per-voicing tests: this is a sweep over 626
    // presets — 256 of them the DX7's factory voices, 229 the phosphor
    // synth's four sets and 56 the Juno's — and the latest peak measured
    // anywhere in the banks arrives at 192 ms, so 370 ms leaves the attack
    // and decay well covered.
    //
    // The phosphor synth's own sweep, in `synth.rs`, is the one that covers
    // the slow half of its bank properly: it derives a render length per
    // patch from that patch's attack, and adds a case with every clock in the
    // patch sped up so that an LFO or a sequence slower than the render
    // cannot hide a peak. This one is the project-wide check that no bank has
    // drifted above the target, at one length for all six instruments.
    const SWEEP_BLOCKS: usize = 64;

    let mut worst = (0.0f32, String::new());

    for index in 0..dx7::VOICE_COUNT {
        let mut s = dx7_voice(index);
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "dx7", dx7::voice_name(index), &mut worst);
    }
    for index in 0..jupiter::PATCH_COUNT {
        let mut s = jupiter::Jupiter8Synth::new();
        s.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(index));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "jupiter", jupiter::PATCH_LABELS[index], &mut worst);
    }
    for index in 0..odyssey::PATCH_COUNT {
        // Duophonic: a single note is this instrument's worst case, not a
        // chord, so it gets checked on both.
        for notes in [SINGLE, TWO_HAND_EIGHT] {
            let mut s = odyssey::OdysseySynth::new();
            s.set_parameter(odyssey::P_PATCH, odyssey::patch_knob(index));
            let m = render(&mut s, notes, 127, SWEEP_BLOCKS);
            check_patch(&m, "odyssey", odyssey::PATCH_NAMES[index], &mut worst);
        }
    }
    for index in 0..juno::PATCH_COUNT {
        let mut s = juno::Juno60Synth::new();
        s.set_parameter(juno::P_PATCH, juno::patch_knob(index));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "juno", juno::PATCH_LABELS[index], &mut worst);
    }
    for index in 0..rhodes::PATCH_COUNT {
        let mut s = rhodes::RhodesPiano::new();
        s.set_parameter(rhodes::P_PATCH, rhodes::patch_knob(index));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "rhodes", rhodes::PATCH_NAMES[index], &mut worst);
    }
    for index in 0..phatty::PATCH_COUNT {
        // Monophonic: a chord is one note here, and a low one is the worst
        // case rather than a high one, so it gets checked on both.
        for notes in [SINGLE, TWO_HAND_EIGHT] {
            let mut s = phatty::LittlePhatty::new();
            s.set_parameter(phatty::P_PATCH, phatty::patch_knob(index));
            let m = render(&mut s, notes, 127, SWEEP_BLOCKS);
            check_patch(&m, "phatty", phatty::PATCH_NAMES[index], &mut worst);
        }
    }
    for index in 0..prophet6::PROGRAM_COUNT {
        let mut s = p6_program(index);
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "prophet6", prophet6::program_name(index), &mut worst);
    }
    for index in 0..teo5::PROGRAM_COUNT {
        let mut s = teo5_program(index);
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "teo5", teo5::program_name(index), &mut worst);
    }
    for index in 0..synth::PATCH_COUNT {
        let mut s = synth::PhosphorSynth::new();
        s.set_parameter(synth::P_PATCH, synth::patch_knob(index));
        let m = render(&mut s, TWO_HAND_EIGHT, 127, SWEEP_BLOCKS);
        check_patch(&m, "phosphor", synth::PATCH_NAMES[index], &mut worst);
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

/// The peak of a slow sweep arrives long after the other banks have finished.
///
/// Everything else here renders 0.37 s or 1.16 s, because that is where every
/// attack transient in the project lands. The Juno's factory bank breaks that
/// assumption: 77 SURF is a noise wash swept by an LFO at 0.31 Hz, so its
/// loudest sample is 4.7 s into a held chord — four times later than anything
/// else measured anywhere in this file, and 2.3 times the peak the 0.37 s
/// sweep sees. Held for thirty seconds it reaches 0.687 and stops there, so
/// the number below is the whole of it, but a short render would have called
/// this patch quiet.
#[test]
fn the_junos_slow_sweeps_peak_after_the_other_banks_have_finished() {
    // 11.6 s, which is three cycles of the slowest LFO in the bank.
    const HOLD: usize = 2000;
    // 77 SURF, 57 REED 3 and 51 FUNNY CAT: the three patches whose peak
    // arrives after the 1.16 s the per-voicing tests render.
    const LATE: [(usize, &str); 3] = [(54, "77 SURF"), (38, "57 REED 3"), (32, "51 FUNNY CAT")];

    let mut worst = (0.0f32, String::new());
    for (index, name) in LATE {
        let mut s = juno::Juno60Synth::new();
        s.set_parameter(juno::P_PATCH, juno::patch_knob(index));
        assert_eq!(juno::PATCH_LABELS[index], name, "the bank moved under this test");
        let m = render(&mut s, TWO_HAND_EIGHT, 127, HOLD);
        check_patch(&m, "juno held", name, &mut worst);
    }
    assert!(worst.0 > 0.5, "the slow sweeps no longer reach the level they were pinned at");
}

/// The same defect on the Jupiter, and worse: its factory bank ends in two
/// banks of Roland's own sound effects, and an effect is by definition
/// something that takes its time.
///
/// 61 STARTING UP is the loudest patch in that bank and the clearest case —
/// 2.5 s of filter attack behind 1.6 s of LFO delay, so its peak arrives 5.5 s
/// into a held chord and measures 33 times what the 0.37 s sweep sees. Four
/// others peak past the window: 66 SOLAR WINDS is a quarter-hertz LFO on
/// noise, 51 TRAIN CHUG holds its effect off for over three seconds by
/// design, and 54 BIRDS and 64 MUSIC OF THE SPHERES wander under cross
/// modulation.
#[test]
fn the_jupiters_effects_peak_after_the_other_banks_have_finished() {
    /// 9.3 s, which reaches past the 9.05 s peak of the slowest of the five.
    const HOLD: usize = 1600;
    const LATE: [(usize, &str); 5] = [
        (40, "61 START UP"),
        (45, "66 SOLAR WND"),
        (32, "51 TRAIN CHG"),
        (35, "54 BIRDS"),
        (43, "64 SPHERES"),
    ];

    let mut worst = (0.0f32, String::new());
    for (index, name) in LATE {
        let mut s = jupiter::Jupiter8Synth::new();
        s.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(index));
        assert_eq!(jupiter::PATCH_LABELS[index], name, "the bank moved under this test");
        let m = render(&mut s, TWO_HAND_EIGHT, 127, HOLD);
        check_patch(&m, "jupiter held", name, &mut worst);
    }
    // Measured: 61 STARTING UP at 0.552, which is 2.7 dB under the saturator
    // knee and 4.2 dB under the ceiling. The floor is here so that a patch
    // quietened into safety fails rather than passes.
    assert!(
        worst.0 > 0.4,
        "the Jupiter's effects no longer reach the level they were voiced at: {worst:?}"
    );
}

/// The same defect again on the Prophet-6, whose 500 programs include slow
/// tremolos over resonant filters.
///
/// 285 Genesis 2 is the loudest program in the bank and the clearest case: an
/// LFO at 0.09 Hz on the amplifier over both filters at maximum resonance, so
/// its peak arrives eleven seconds into a held chord — thirty times later
/// than the 0.37 s sweep looks. Four others peak past the short window for
/// the same reason.
#[test]
fn the_prophet_sixs_slow_tremolos_peak_after_the_other_banks_have_finished() {
    /// 14 s, which reaches past the slowest of the five.
    const HOLD: usize = 2_400;
    const LATE: [(usize, &str); 5] = [
        (285, "Genesis 2"),
        (259, "Amiga Chop"),
        (162, "Synth on Wheels"),
        (261, "Emotive Pad"),
        (109, "Chalice"),
    ];

    let mut worst = (0.0f32, String::new());
    for (index, name) in LATE {
        let mut s = p6_program(index);
        assert_eq!(prophet6::program_name(index), name, "the bank moved under this test");
        let m = render(&mut s, TWO_HAND_EIGHT, 127, HOLD);
        check_patch(&m, "prophet6 held", name, &mut worst);
    }
    // Measured: 285 Genesis 2 at 0.57, which is 2.4 dB under the saturator
    // knee and 3.9 dB under the ceiling. The floor is here so that a program
    // quietened into safety fails rather than passes.
    assert!(
        worst.0 > 0.4,
        "the Prophet-6's slow programs no longer reach the level they were voiced at: {worst:?}"
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
    let mut worst = (0.0f32, String::new());
    for kit in 0..drum_rack::KIT_COUNT {
        for notes in [&[36u8][..], FULL_KIT] {
            let m = render(&mut rack_at(kit, None), notes, 127, BLOCKS);
            check_patch(&m, "drum rack", &format!("kit {kit}"), &mut worst);
        }
    }
    assert!(worst.0 > 0.5, "no kit reaches half of full scale: {worst:?}");
}

/// Kick, snare, two toms, closed and open hat, crash, ride — a full kit hit on
/// the same sample, which is what a quantised fill lands as, and the drum
/// rack's worst case.
const FULL_KIT: &[u8] = &[36, 38, 41, 42, 45, 46, 49, 51];

/// Every drum-rack knob that shapes a voice, which is the panel minus the kit
/// selector, the twelve level knobs and the two rack controls.
///
/// The level knobs are left out because they sit at the top of their travel by
/// default and only cut, so the defaults already are their loudest setting.
/// DRIVE and GAIN are left out because they get their own sweep — see
/// `no_drive_setting_exceeds_the_target` and `the_gain_knob_can_only_cut`.
fn shaping_controls() -> Vec<usize> {
    const LEVELS: [usize; 12] = [
        drum_rack::P_BD_LEVEL,
        drum_rack::P_SD_LEVEL,
        drum_rack::P_LT_LEVEL,
        drum_rack::P_MT_LEVEL,
        drum_rack::P_HT_LEVEL,
        drum_rack::P_RS_LEVEL,
        drum_rack::P_CP_LEVEL,
        drum_rack::P_CB_LEVEL,
        drum_rack::P_CY_LEVEL,
        drum_rack::P_RD_LEVEL,
        drum_rack::P_OH_LEVEL,
        drum_rack::P_CH_LEVEL,
    ];
    (0..drum_rack::PARAM_COUNT)
        .filter(|i| {
            !LEVELS.contains(i)
                && *i != drum_rack::P_KIT
                && *i != drum_rack::P_DRIVE
                && *i != drum_rack::P_GAIN
        })
        .collect()
}

/// A rack on `kit`, with every shaping control at `setting` when one is given.
fn rack_at(kit: usize, setting: Option<f32>) -> drum_rack::DrumRack {
    let mut rack = drum_rack::DrumRack::new();
    rack.set_parameter(drum_rack::P_KIT, drum_rack::kit_knob(kit));
    if let Some(value) = setting {
        for i in shaping_controls() {
            rack.set_parameter(i, value);
        }
    }
    rack
}

/// The drum rack's panel is per-instrument, which is thirty-three new ways to
/// change the level of a hit. This drives all of them to both ends.
#[test]
fn no_drum_panel_setting_exceeds_the_target() {
    let mut worst = (0.0f32, String::new());
    for kit in 0..drum_rack::KIT_COUNT {
        for (setting, value) in [("min", 0.0f32), ("max", 1.0)] {
            for notes in [&[36u8][..], FULL_KIT] {
                let m = render(&mut rack_at(kit, Some(value)), notes, 127, BLOCKS);
                check_patch(&m, "drum panel", &format!("kit {kit} {setting}"), &mut worst);
            }
        }
    }
    // The panel's extremes should not be *quieter* than its defaults either:
    // a control that only ever takes level away is one that has been trimmed
    // into safety rather than voiced.
    assert!(worst.0 > 0.5, "no panel setting reaches half of full scale: {worst:?}");
}

/// The two rack controls the panel sweep leaves out, and the reason this test
/// exists: neither was ever swept, and both could put a kit past the ceiling
/// on their own.
///
/// DRIVE was the worse of the two. It multiplied by up to 17 before its own
/// denominator, so the knob was a level control as much as a tone control: a
/// solo 808 kick went from 0.16 to 0.66 across its travel, +12 dB, and a full
/// kit from 0.64 to 0.89. Nine of the fifteen kits ended up over the master
/// limiter's ceiling at the top of the knob and the SDS-V was 0.9 dB over at a
/// quarter turn. It is now gain-compensated — see `drive_stage` in the rack —
/// and what is asserted is both halves of that: the ceiling holds at every
/// setting, and the peak stays near where the knob at zero left it.
///
/// The two bounds are on different measurements, and deliberately:
///
/// * **Peak may not rise** by more than 3 dB. Peak is what the ceiling is
///   about, and a rising peak is the defect. The measured worst is the SDS-V's
///   2.5 dB on a full kit — that machine's voices are filtered noise that sits
///   close to its peak for the whole hit, so lifting their bodies lifts the
///   sum with them. Everything else moves under 0.3 dB.
/// * **RMS may not fall.** RMS is what the ear tracks, and a compressor takes
///   peaks down while bringing loudness up: a solo 909 kick loses 4 dB of peak
///   across the knob and gains 4.5 dB of RMS. Asserting a band on peak in both
///   directions would be asserting that the knob does not compress, which is
///   the one thing it is for.
#[test]
fn no_drive_setting_exceeds_the_target() {
    const DRIVE_SETTINGS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    /// How much level the knob is allowed to add to a peak, in dB.
    const PEAK_RISE_DB: f32 = 3.0;
    /// How much loudness it is allowed to take away, in dB. Not zero: the
    /// 777's kit is 0.4 dB down at the top of the knob.
    const RMS_DROP_DB: f32 = 1.0;

    let mut worst = (0.0f32, String::new());
    for kit in 0..drum_rack::KIT_COUNT {
        // Both at the panel's defaults and with everything that shapes a voice
        // at the top, which is where the kits sit closest to the ceiling.
        for shaping in [None, Some(1.0f32)] {
            for notes in [&[36u8][..], FULL_KIT] {
                let mut undriven = None;
                for drive in DRIVE_SETTINGS {
                    let mut rack = rack_at(kit, shaping);
                    rack.set_parameter(drum_rack::P_DRIVE, drive);
                    let m = render(&mut rack, notes, 127, BLOCKS);
                    let name = format!(
                        "kit {kit} drive {drive}{}",
                        if shaping.is_some() { " panel max" } else { "" }
                    );
                    check_patch(&m, "drum drive", &name, &mut worst);

                    let Some((peak, rms)) = undriven else {
                        undriven = Some((m.peak, m.rms));
                        continue;
                    };
                    let peak_moved = 20.0 * (m.peak / peak).log10();
                    assert!(
                        peak_moved <= PEAK_RISE_DB,
                        "drum drive {name}: the knob added {peak_moved:+.1} dB of peak \
                         ({peak:.4} -> {:.4}); it is a tone control, not a fader",
                        m.peak
                    );
                    let rms_moved = 20.0 * (m.rms / rms).log10();
                    assert!(
                        rms_moved >= -RMS_DROP_DB,
                        "drum drive {name}: the knob took {rms_moved:.1} dB of loudness \
                         away ({rms:.5} -> {:.5} RMS); driving a kit should not make it \
                         quieter",
                        m.rms
                    );
                }
            }
        }
    }
    assert!(worst.0 > 0.5, "no drive setting reaches half of full scale: {worst:?}");
}

/// GAIN is the rack's output trim, and it sits at the top of its travel by
/// default so that it can only cut — the same reasoning as the twelve level
/// knobs, and for the same reason: the headroom trim was measured at one knob
/// position, so any travel above that position is headroom nothing measured.
/// It used to have a quarter of a turn up there, which put a full 777 kit at
/// 0.928, past the ceiling.
#[test]
fn the_gain_knob_can_only_cut() {
    assert_eq!(
        drum_rack::PARAM_DEFAULTS[drum_rack::P_GAIN], 1.0,
        "GAIN has travel above its default again"
    );

    let mut worst = (0.0f32, String::new());
    for kit in 0..drum_rack::KIT_COUNT {
        let mut louder_setting = 0.0f32;
        for gain in [1.0f32, 0.75, 0.5, 0.0] {
            let mut rack = rack_at(kit, Some(1.0));
            rack.set_parameter(drum_rack::P_GAIN, gain);
            let m = render(&mut rack, FULL_KIT, 127, BLOCKS);
            check_patch(&m, "drum gain", &format!("kit {kit} gain {gain}"), &mut worst);
            if gain < 1.0 {
                assert!(
                    m.peak < louder_setting,
                    "drum gain kit {kit}: the knob at {gain} is not quieter than the \
                     one above it ({:.4} against {louder_setting:.4})",
                    m.peak
                );
            }
            louder_setting = m.peak;
        }
    }
    assert!(worst.0 > 0.5, "no gain setting reaches half of full scale: {worst:?}");
}

/// One instrument pointed at the loudest thing in its bank, with the knob its
/// headroom trim was measured at one position of.
///
/// The DX7 and the phosphor synth call theirs GAIN and the other three call
/// theirs LEVEL; all five are the same stage — a multiplier on the voice sum,
/// applied just before the headroom trim and the bounding saturator.
struct KnobCase {
    name: &'static str,
    /// Index of the output knob on that instrument's panel.
    param: usize,
    notes: &'static [u8],
    /// How long this patch takes to reach its peak.
    blocks: usize,
    plugin: Box<dyn Plugin>,
}

fn output_knob_cases() -> Vec<KnobCase> {
    let mut jupiter_organ = jupiter::Jupiter8Synth::new();
    jupiter_organ.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(28));
    let mut juno_tuba = juno::Juno60Synth::new();
    juno_tuba.set_parameter(juno::P_PATCH, juno::patch_knob(27));
    let mut odyssey_funk = odyssey::OdysseySynth::new();
    odyssey_funk.set_parameter(odyssey::P_PATCH, odyssey::patch_knob(1));
    let mut rhodes_bark = rhodes::RhodesPiano::new();
    rhodes_bark.set_parameter(rhodes::P_PATCH, rhodes::patch_knob(22));

    let case = |name, param, notes, plugin| KnobCase { name, param, notes, blocks: BLOCKS, plugin };
    vec![
        case("dx7 default", dx7::P_GAIN, TWO_HAND_EIGHT, Box::new(dx7::Dx7Synth::new())),
        case("dx7 147 TIMPANI", dx7::P_GAIN, TWO_HAND_EIGHT, Box::new(dx7_voice(147))),
        case("dx7 60 HARP 1", dx7::P_GAIN, TWO_HAND_EIGHT, Box::new(dx7_voice(60))),
        case("dx7 195 CLARINET", dx7::P_GAIN, TWO_HAND_EIGHT, Box::new(dx7_voice(195))),
        case("phosphor", synth::P_GAIN, TWO_HAND_EIGHT, Box::new(synth::PhosphorSynth::new())),
        case("jupiter 45 PIPE ORGN", jupiter::P_LEVEL, TWO_HAND_EIGHT, Box::new(jupiter_organ)),
        case("juno 44 TUBA", juno::P_LEVEL, TWO_HAND_EIGHT, Box::new(juno_tuba)),
        case("rhodes Hard Bark", rhodes::P_LEVEL, TWO_HAND_EIGHT, Box::new(rhodes_bark)),
        // Duophonic: a single note is the Odyssey's worst case, not a chord.
        case("odyssey 1 Funk", odyssey::P_LEVEL, SINGLE, Box::new(odyssey_funk)),
    ]
}

/// The two patches in the project whose peak arrives after the render window
/// the sweeps use, at the top of their instrument's output knob.
///
/// They are checked at the top of the travel and nowhere else, and that is
/// enough: the knob is a plain multiplier applied before the bounding stage,
/// so peak is monotonic in it and the top is the only setting that can be the
/// worst one. `no_output_knob_setting_exceeds_the_target` is what holds that
/// monotonicity, on renders short enough to sweep.
fn late_peaking_at_the_top_of_the_knob() -> Vec<KnobCase> {
    let mut jupiter_startup = jupiter::Jupiter8Synth::new();
    jupiter_startup.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(40));
    jupiter_startup.set_parameter(jupiter::P_LEVEL, 1.0);
    let mut juno_surf = juno::Juno60Synth::new();
    juno_surf.set_parameter(juno::P_PATCH, juno::patch_knob(54));
    juno_surf.set_parameter(juno::P_LEVEL, 1.0);

    vec![
        KnobCase {
            name: "jupiter 61 START UP",
            param: jupiter::P_LEVEL,
            notes: TWO_HAND_EIGHT,
            // 9.3 s, which reaches past the 9.05 s peak of that patch.
            blocks: 1600,
            plugin: Box::new(jupiter_startup),
        },
        KnobCase {
            name: "juno 77 SURF",
            param: juno::P_LEVEL,
            notes: TWO_HAND_EIGHT,
            // 11.6 s, three cycles of the slowest LFO in that bank.
            blocks: 2000,
            plugin: Box::new(juno_surf),
        },
    ]
}

/// The defect the drum rack's GAIN had, on the other five instruments: a
/// headroom trim measured at one knob position leaves everything above that
/// position unmeasured, and nothing swept these knobs at all.
///
/// Two of them were over the master limiter's ceiling at the top of their
/// travel. The DX7's GAIN defaulted to 0.75, so ROM3A's TIMPANI reached 0.9298
/// with the knob up and ROM1B's HARP 1 0.9282; the phosphor synth's reached
/// 0.9379 with DRIVE up as well. Both defaults are now folded into the
/// instrument's `OUTPUT_TRIM` — see `the_folded_gain_knobs_can_only_cut`. The
/// Jupiter, the Juno and the Odyssey were never over it; they are here so that
/// they cannot become so quietly.
#[test]
fn no_output_knob_setting_exceeds_the_target() {
    const SETTINGS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

    let mut worst = (0.0f32, String::new());
    for mut case in output_knob_cases() {
        let mut louder = f32::INFINITY;
        for setting in SETTINGS.iter().rev().copied() {
            case.plugin.set_parameter(case.param, setting);
            let m = render(case.plugin.as_mut(), case.notes, 127, case.blocks);
            let name = case.name;
            check_patch(&m, "output knob", &format!("{name} at {setting}"), &mut worst);
            // A multiplier before the bounding stage, so the knob can only
            // take peak off as it comes down. Anything else means it is not
            // the stage this test thinks it is.
            assert!(
                m.peak < louder,
                "{name}: the knob at {setting} is not quieter than the setting above it \
                 ({:.4} against {louder:.4})",
                m.peak
            );
            louder = m.peak;
        }
    }

    for mut case in late_peaking_at_the_top_of_the_knob() {
        assert_eq!(
            case.plugin.get_parameter(case.param),
            1.0,
            "{}: the knob is not at the top",
            case.name
        );
        let m = render(case.plugin.as_mut(), case.notes, 127, case.blocks);
        check_patch(&m, "output knob held", case.name, &mut worst);
    }

    assert!(worst.0 > 0.5, "no output knob setting reaches half of full scale: {worst:?}");
}

/// The three instruments whose trim was measured at a GAIN of 0.75 had that
/// 0.75 folded into the trim, which leaves the knob at the top of its travel
/// and able only to cut. The fold is exact — `1.0 * (t * 0.75)` and `0.75 * t`
/// are the same f32 — so every render at the default is what it was, sample
/// for sample; each instrument holds that itself, with the old constant pinned
/// beside the new one.
#[test]
fn the_folded_gain_knobs_can_only_cut() {
    assert_eq!(
        dx7::PARAM_DEFAULTS[dx7::P_GAIN], 1.0,
        "the DX7's GAIN has travel above its default again"
    );
    assert_eq!(
        synth::PARAM_DEFAULTS[synth::P_GAIN], 1.0,
        "the phosphor synth's GAIN has travel above its default again"
    );
    assert_eq!(
        drum_rack::PARAM_DEFAULTS[drum_rack::P_GAIN], 1.0,
        "the drum rack's GAIN has travel above its default again"
    );
}

/// DRIVE is the other knob nothing swept, and on the phosphor synth it had the
/// same defect the drum rack's did: the knob multiplied by up to 11 before its
/// own denominator, so it was a level control as much as a tone control. An
/// eight-note chord went from 0.346 to 0.901 across its travel, +8.3 dB and
/// past the master limiter's ceiling, and the curve was not even monotonic in
/// the knob — the panel at its extremes measured 0.947 at drive 0 and 0.888 at
/// drive 0.1. It is now gain-compensated, the same curve the rack uses; see
/// `drive_stage` in synth.rs.
///
/// The Odyssey's DRIVE is here because it is the only other one, and it is
/// unchanged: nothing in its bank reaches 0.29 at any setting of it.
///
/// The two bounds are on different measurements, and deliberately — the same
/// reasoning as `no_drive_setting_exceeds_the_target` for the rack. Peak may
/// not rise, because peak is what the ceiling is about; RMS may not fall,
/// because a compressor takes peaks down while bringing loudness up, and
/// asserting a band on peak in both directions would assert that the knob does
/// not compress, which is the one thing it is for.
#[test]
fn no_keyboard_drive_setting_exceeds_the_target() {
    const DRIVE_SETTINGS: [f32; 6] = [0.0, 0.1, 0.25, 0.5, 0.75, 1.0];
    /// How much level the knob is allowed to add to a peak, in dB. The
    /// measured worst is the phosphor synth's +0.5 dB on an eight-note chord.
    const PEAK_RISE_DB: f32 = 3.0;

    let mut worst = (0.0f32, String::new());
    for notes in [SINGLE, TWO_HAND_EIGHT] {
        let mut undriven = None;
        for drive in DRIVE_SETTINGS {
            let mut s = synth::PhosphorSynth::new();
            s.set_parameter(synth::P_DRIVE, drive);
            let m = render(&mut s, notes, 127, BLOCKS);
            let name = format!("phosphor drive {drive}");
            check_patch(&m, "keyboard drive", &name, &mut worst);

            let Some((peak, rms)) = undriven else {
                undriven = Some((m.peak, m.rms));
                continue;
            };
            let peak_moved = 20.0 * (m.peak / peak).log10();
            assert!(
                peak_moved <= PEAK_RISE_DB,
                "{name}: the knob added {peak_moved:+.1} dB of peak ({peak:.4} -> {:.4}); \
                 it is a tone control, not a fader",
                m.peak
            );
            assert!(
                m.rms >= rms,
                "{name}: the knob took loudness away ({rms:.5} -> {:.5} RMS); driving a \
                 patch should not make it quieter",
                m.rms
            );
        }
    }

    // The Little Phatty's OVERLOAD is the one knob in the project that is
    // *meant* to raise the level: "When set to 100%, Overload adds a volume
    // boost of about +6dB" (Stage II manual, page 14). So the flat-peak bound
    // above does not apply to it.
    //
    // Neither does the loudness bound, and the reason is worth stating because
    // it is not a licence: the knob itself is held to it exhaustively in
    // phatty.rs, where `the_overload_knob_never_takes_loudness_away` sweeps
    // every input the mixer can produce against every setting of the knob, and
    // is what sets the shaper's knee. What that cannot cover is the one patch
    // in the bank whose sound source is the *filter*: Sine Floor is a ladder
    // self-oscillating at resonance 0.99, and driving a self-oscillating
    // ladder harder suppresses it, because the nonlinearity in the feedback
    // path steals the loop gain that was producing the tone. That is the
    // filter behaving like a filter, not the knob behaving like a fader.
    //
    // So what is asserted here is what this suite is for: the whole bank stays
    // under the ceiling at every setting of the knob.
    for index in 0..phatty::PATCH_COUNT {
        for overload in [0.0f32, 0.5, 1.0] {
            let mut s = phatty::LittlePhatty::new();
            s.set_parameter(phatty::P_PATCH, phatty::patch_knob(index));
            s.set_parameter(phatty::P_OVERLOAD, overload);
            // Monophonic, so a chord is one note; a low one is the worst case.
            let m = render(&mut s, SINGLE, 127, 32);
            let name = format!("phatty {} overload {overload}", phatty::PATCH_NAMES[index]);
            check_patch(&m, "keyboard drive", &name, &mut worst);
        }
    }

    for index in 0..odyssey::PATCH_COUNT {
        for drive in DRIVE_SETTINGS {
            let mut s = odyssey::OdysseySynth::new();
            s.set_parameter(odyssey::P_PATCH, odyssey::patch_knob(index));
            s.set_parameter(odyssey::P_DRIVE, drive);
            s.set_parameter(odyssey::P_LEVEL, 1.0);
            // A single note is this instrument's worst case; it is duophonic.
            let m = render(&mut s, SINGLE, 127, 64);
            let name = format!("odyssey {} drive {drive}", odyssey::PATCH_NAMES[index]);
            check_patch(&m, "keyboard drive", &name, &mut worst);
        }
    }

    assert!(worst.0 > 0.2, "no drive setting reaches a fifth of full scale: {worst:?}");
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
/// values are 0.0187 (DX7, the tightest), 0.0198, 0.0210, 0.0235, 0.0270 and
/// 0.0314 — so it sits about 4 dB under the quietest. Enough margin that a
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
        ("rhodes", TRIAD, Box::new(rhodes::RhodesPiano::new())),
        ("phatty", TRIAD, Box::new(phatty::LittlePhatty::new())),
        ("prophet6", TRIAD, Box::new(prophet6::Prophet6::new())),
        ("teo5", TRIAD, Box::new(teo5::Teo5::new())),
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
/// The target used to be −12 dBFS, and the whole project is trimmed around
/// it. The DX7 no longer meets it, and that is a decision rather than a drift:
/// its loudest factory voice sits 10 dB above the bank's median, so the trim
/// is now sized by that voice instead of by ordinary playing. See
/// `OUTPUT_TRIM` in dx7.rs. The cost is 4 dB on everything the instrument
/// plays; the alternative was 16 voices reaching past the master limiter's
/// ceiling, where a single track's transient ducks the whole mix.
///
/// A ±1.5 dB window around the measured −14.1 dBFS: wide enough that a
/// rounded trim constant does not trip it, narrow enough that another
/// re-target cannot happen silently.
#[test]
fn dx7_default_triad_lands_where_the_trim_puts_it() {
    let (peak, _) = peak_and_rms(&mut dx7::Dx7Synth::new(), TRIAD, 100);
    let dbfs = 20.0 * peak.log10();
    assert!(
        (-15.6..=-12.6).contains(&dbfs),
        "DX7 default voice, triad @100, peaks at {peak:.4} ({dbfs:.1} dBFS); \
         the trim puts ordinary playing at -14.1 dBFS"
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
    // The Jupiter's is 12 NEG PLUCK, the median of its 64 factory patches.
    // The Rhodes' is MK2 Stage, the median of its 26.
    // The phosphor synth's is patch 0, INIT SAW, which is both its default and
    // the middle of its nine — so `PhosphorSynth::new()` below is already it.
    let mut dx7_mid = dx7_voice(DX7_MEDIAN_VOICE);
    let mut jupiter_mid = jupiter::Jupiter8Synth::new();
    jupiter_mid.set_parameter(jupiter::P_PATCH, jupiter::patch_knob(1));
    let mut odyssey_mid = odyssey::OdysseySynth::new();
    odyssey_mid.set_parameter(odyssey::P_PATCH, odyssey::patch_knob(0));
    let mut juno_mid = juno::Juno60Synth::new();
    juno_mid.set_parameter(juno::P_PATCH, juno::patch_knob(3));
    let mut rhodes_mid = rhodes::RhodesPiano::new();
    rhodes_mid.set_parameter(rhodes::P_PATCH, rhodes::patch_knob(13));
    // The Little Phatty's is 78 Half Ladder, the median of its hundred. It is
    // monophonic, so its "triad" is one note — which is why its trim is set on
    // rms rather than on peak; see OUTPUT_TRIM in phatty.rs.
    let mut phatty_mid = phatty::LittlePhatty::new();
    phatty_mid.set_parameter(phatty::P_PATCH, phatty::patch_knob(78));
    // The Prophet-6's is 019 Dynamic Phase, the median of its 500 factory
    // programs by triad RMS.
    let mut p6_mid = p6_program(19);
    // The TEO-5's is 107 Santur, the median of its 256 factory programs by
    // triad RMS.
    let mut teo5_mid = teo5_program(107);

    let levels = [
        ("dx7", triad_rms(&mut dx7_mid)),
        ("jupiter", triad_rms(&mut jupiter_mid)),
        ("odyssey", triad_rms(&mut odyssey_mid)),
        ("juno", triad_rms(&mut juno_mid)),
        ("rhodes", triad_rms(&mut rhodes_mid)),
        ("phatty", triad_rms(&mut phatty_mid)),
        ("prophet6", triad_rms(&mut p6_mid)),
        ("teo5", triad_rms(&mut teo5_mid)),
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
