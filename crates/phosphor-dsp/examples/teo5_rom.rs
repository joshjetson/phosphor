//! Builds `src/teo5_programs.bin` from the decoded TEO-5 factory bank.
//!
//! ```text
//! cargo run -p phosphor-dsp --example teo5_rom -- teo5_programs.json
//! cargo run -p phosphor-dsp --example teo5_rom -- teo5_programs.json --check
//! cargo run -p phosphor-dsp --example teo5_rom -- teo5_programs.json TEO5_Factory_Programs_v1.00.syx --check
//! ```
//!
//! ## Provenance
//!
//! `TEO5_Factory_Programs_v1.00.syx` is Oberheim's factory program set of
//! 8 July 2024, 1,201,920 bytes, still distributed from the TEO-5 support
//! page. It is 256 SysEx messages of 4,695 bytes:
//!
//! ```text
//! F0 10 5A 02 <bank 0-15> <program 0-15> <4688 packed bytes> F7
//! ```
//!
//! `10` is Oberheim's classic manufacturer ID, `5A` the TEO-5 device ID, `02`
//! the Program Data opcode. The payload is the same packed-MS-bit format
//! Sequential's platform has always used — eight-byte groups whose first byte
//! carries the stripped high bits of the seven that follow, least significant
//! bit first — and 4,688 packed bytes unpack to 4,102.
//!
//! `teo5_programs.json` is the decode of that file: every program's named
//! parameters, its sixteen modulation slots, its name and category, its
//! sequence, and the ten bytes nobody has identified. This program turns that
//! JSON back into the machine's own bytes and keeps the first 190 of them:
//!
//! | bytes | contents | kept |
//! |---|---|---|
//! | 0…158 | scalar program parameters; **offset == NRPN number** | yes |
//! | 159…178 | the name, 20 ASCII characters, space padded | yes |
//! | 179 | the program category, 1–15 | yes |
//! | 180…187 | key split and arpeggiator; offset == NRPN number | yes |
//! | 188…189 | a zero byte and the sequence length | yes |
//! | 190…909 | the 64-step, 5-track sequencer | no — sequencing is the DAW's |
//! | 910…4101 | reserved, zero in all 256 programs | no |
//!
//! so the output is 256 × 190 = 48,640 bytes, in bank-then-program order.
//! `teo5.rs` reads it back with the byte map in its `raw_offset`.
//!
//! Because the JSON is a *decode* rather than a copy, the reconstruction here
//! is only worth what the decode is worth — so pass the original `.syx` as a
//! second argument and this program unpacks it too and asserts the 190 bytes
//! agree, program for program. That is the check that makes the JSON path
//! safe: two independent readings of the same file have to produce the same
//! bytes.
//!
//! ## Checks
//!
//! Every reconstructed program is checked for the fourteen parameters whose
//! documented range is narrow enough to catch a misplaced field, for a
//! printable non-blank name, for a category inside 1–15, and for every
//! modulation slot being inside source 0–19 / destination 0–64. A failure
//! anywhere aborts without writing.

use std::fmt::Write as _;

use serde_json::Value;

const PROGRAM_COUNT: usize = 256;
/// Program parameters, name, category, split, arpeggiator and sequence
/// length: the whole of what `teo5.rs` reads, and everything in the program
/// record that is not the note grid.
const KEPT: usize = 190;

/// The SysEx container, for the optional cross-check against the original.
const MESSAGE: usize = 4_695;
const PACKED: usize = 4_688;
const UNPACKED: usize = 4_102;

/// Where each named parameter of the decode lives in the program blob.
///
/// This is `PARAM_MAP.md` §2 as a table: for offsets 1–158 and 180–187 the
/// offset *is* the NRPN parameter number, which the decode established from
/// the bank rather than assuming. The two-byte fields, the name, the
/// category, the modulation matrix, the chord memory and the ten unidentified
/// bytes are placed separately below.
const SCALARS: &[(usize, &str)] = &[
    (1, "osc1_freq"),
    (2, "osc2_freq"),
    (3, "osc1_detune"),
    (4, "osc2_detune"),
    (5, "osc1_pulse_width"),
    (6, "osc2_pulse_width"),
    (7, "osc1_tri_on"),
    (8, "osc2_tri_on"),
    (9, "osc1_saw_on"),
    (10, "osc2_saw_on"),
    (11, "osc1_pulse_on"),
    (12, "osc2_pulse_on"),
    (13, "osc1_on"),
    (14, "osc2_on"),
    (15, "osc1_level"),
    (16, "osc2_level"),
    (17, "sub_osc_on"),
    (18, "sub_osc_level"),
    (19, "noise_on"),
    (20, "noise_type"),
    (21, "noise_level"),
    (22, "osc1_glide"),
    (23, "osc2_glide"),
    (24, "osc1_key_on"),
    (25, "osc2_key_on"),
    (26, "x_mod_amount"),
    (27, "osc1_sync"),
    (28, "osc2_filter_bypass"),
    (30, "glide_mode"),
    (31, "glide_on"),
    (32, "pitch_bend_range_up"),
    (33, "pitch_bend_range_down"),
    (34, "filter_cutoff_lsb"),
    (35, "filter_cutoff_msb"),
    (36, "filter_resonance"),
    (37, "filter_bandpass"),
    (38, "filter_state_lsb"),
    (39, "filter_state_msb"),
    (40, "filter_key_amount"),
    (42, "fx_on"),
    (43, "fx_select"),
    (44, "fx_mix"),
    (45, "fx_time"),
    (46, "fx_misc"),
    (47, "fx_sync_on"),
    (48, "fx_sync_rate"),
    (50, "reverb_on"),
    (52, "reverb_mix"),
    (53, "reverb_size"),
    (54, "reverb_predelay"),
    (55, "reverb_decay"),
    (56, "reverb_tone"),
    (58, "lfo1_freq"),
    (59, "lfo2_freq"),
    (60, "lfo1_amount"),
    (61, "lfo2_amount"),
    (62, "lfo1_shape"),
    (63, "lfo2_shape"),
    (64, "lfo1_sync_on"),
    (65, "lfo2_sync_on"),
    (66, "lfo1_dest"),
    (67, "lfo2_dest"),
    (68, "lfo1_freq_sync"),
    (69, "lfo2_freq_sync"),
    (70, "lfo1_note_reset"),
    (71, "lfo2_note_reset"),
    (72, "lfo1_slew"),
    (73, "lfo2_slew"),
    (75, "env1_amount"),
    (76, "env2_amount"),
    (77, "env1_velocity_on"),
    (78, "env2_velocity_on"),
    (79, "env1_delay"),
    (80, "env2_delay"),
    (81, "env1_attack"),
    (82, "env2_attack"),
    (83, "env1_decay"),
    (84, "env2_decay"),
    (85, "env1_sustain"),
    (86, "env2_sustain"),
    (87, "env1_release"),
    (88, "env2_release"),
    (89, "env_routing"),
    (90, "env1_dest"),
    (91, "env_repeat"),
    (93, "voice_volume"),
    (95, "distortion"),
    (96, "vintage"),
    (97, "unison_on"),
    (98, "unison_voices"),
    (99, "unison_detune"),
    (153, "key_mode"),
    (154, "env_retrigger"),
    (155, "scale"),
    (156, "transpose"),
    (157, "clock_bpm"),
    (158, "clock_divide"),
    (180, "key_split_1oct"),
    (181, "key_split_2oct"),
    (182, "key_split_note"),
    (183, "arp_on"),
    (184, "arp_mode"),
    (185, "arp_range"),
    (186, "arp_repeat"),
    (187, "arp_relatch"),
];

/// The bytes the decode could not name, kept so that the reconstruction is
/// the machine's record rather than an interpretation of it.
const UNIDENTIFIED: &[(usize, &str)] = &[
    (0, "byte_0"),
    (29, "byte_29"),
    (41, "byte_41"),
    (49, "byte_49"),
    (51, "byte_51"),
    (57, "byte_57"),
    (74, "byte_74"),
    (92, "byte_92"),
    (94, "byte_94"),
    (188, "byte_188"),
];

/// Fields whose documented range is narrow enough that a value outside it
/// means something landed in the wrong place: `(offset, name, max)`.
const RANGE_CHECKS: &[(usize, &str, u8)] = &[
    (1, "osc 1 frequency", 63),
    (2, "osc 2 frequency", 63),
    (30, "glide mode", 3),
    (32, "pitch bend up", 12),
    (33, "pitch bend down", 24),
    (35, "filter cutoff msb", 7),
    (39, "filter state msb", 1),
    (43, "effect type", 12),
    (48, "effect sync division", 10),
    (62, "LFO 1 shape", 4),
    (63, "LFO 2 shape", 4),
    (89, "envelope routing", 2),
    (91, "envelope repeat", 3),
    (98, "unison voices", 5),
    (153, "key mode", 2),
    (156, "transpose", 4),
    (158, "clock divide", 7),
    (184, "arpeggiator mode", 4),
    (185, "arpeggiator range", 2),
];

fn byte(value: &Value, key: &str) -> Option<u8> {
    let n = value.get(key)?.as_i64()?;
    u8::try_from(n).ok()
}

fn unpack(packed: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(UNPACKED);
    let mut i = 0;
    while i < packed.len() {
        let ms = packed[i];
        i += 1;
        for j in 0..7 {
            if i >= packed.len() {
                break;
            }
            let b = packed[i];
            i += 1;
            out.push(if ms & (1 << j) != 0 { b | 0x80 } else { b });
        }
    }
    out
}

/// The 190 bytes of one program, rebuilt from the decode.
fn rebuild(program: &Value, index: usize, fail: &mut impl FnMut(String)) -> [u8; KEPT] {
    let mut out = [0u8; KEPT];
    let Some(params) = program.get("params") else {
        fail("no params object".into());
        return out;
    };

    for (offset, name) in SCALARS {
        match byte(params, name) {
            Some(v) => out[*offset] = v,
            None => fail(format!("{name} is missing or not a byte")),
        }
    }
    let unknown = program.get("unidentified").cloned().unwrap_or(Value::Null);
    for (offset, name) in UNIDENTIFIED {
        match byte(&unknown, name) {
            Some(v) => out[*offset] = v,
            None => fail(format!("{name} is missing or not a byte")),
        }
    }

    // Chord memory: five semitone offsets, 127 for an empty slot.
    match params.get("unison_notes").and_then(Value::as_array) {
        Some(notes) if notes.len() == 5 => {
            for (slot, note) in notes.iter().enumerate() {
                match note.as_i64().and_then(|n| u8::try_from(n).ok()) {
                    Some(v) => out[100 + slot] = v,
                    None => fail(format!("unison note {slot} is not a byte")),
                }
            }
        }
        _ => fail("unison_notes is not five values".into()),
    }

    // The modulation matrix is field-major: sixteen sources, then sixteen
    // amounts, then sixteen destinations.
    match program.get("mod_matrix").and_then(Value::as_array) {
        Some(slots) if slots.len() == 16 => {
            for (i, slot) in slots.iter().enumerate() {
                for (base, key) in [(105, "source"), (121, "amount"), (137, "destination")] {
                    match byte(slot, key) {
                        Some(v) => out[base + i] = v,
                        None => fail(format!("mod slot {} {key} is not a byte", i + 1)),
                    }
                }
                let source = out[105 + i];
                let destination = out[137 + i];
                if source > 19 {
                    fail(format!("mod slot {} source is {source} (max 19)", i + 1));
                }
                if destination > 64 {
                    fail(format!("mod slot {} destination is {destination} (max 64)", i + 1));
                }
            }
        }
        _ => fail("mod_matrix is not sixteen slots".into()),
    }

    // The name, space padded to twenty characters.
    let name = program.get("name").and_then(Value::as_str).unwrap_or("");
    let bytes = name.as_bytes();
    if bytes.len() > 20 || bytes.iter().any(|c| !(0x20..=0x7E).contains(c)) {
        fail(format!("name {name:?} is not up to twenty printable ASCII characters"));
    }
    for slot in &mut out[159..179] {
        *slot = b' ';
    }
    for (slot, c) in out[159..179].iter_mut().zip(bytes) {
        *slot = *c;
    }
    if bytes.iter().all(|c| *c == b' ') || bytes.is_empty() {
        fail("name is blank".into());
    }

    // The category, which is not in any NRPN table: the categorized bank is
    // this file sorted on it.
    match program.get("category").and_then(Value::as_i64) {
        Some(c) if (1..=15).contains(&c) => out[179] = c as u8,
        other => fail(format!("category {other:?} is outside 1-15")),
    }

    // The sequence length, the one byte of the sequencer block that is not
    // note data.
    match program.get("sequencer").and_then(|s| s.get("length")).and_then(Value::as_i64) {
        Some(n) if (0..=64).contains(&n) => out[189] = n as u8,
        other => fail(format!("sequence length {other:?} is outside 0-64")),
    }

    for (offset, name, max) in RANGE_CHECKS {
        if out[*offset] > *max {
            fail(format!("{name} at byte {offset} is {} (max {max})", out[*offset]));
        }
    }

    // The bank and program the decode says this is, against where it sits.
    let bank = program.get("bank").and_then(Value::as_i64).unwrap_or(-1);
    let slot = program.get("program").and_then(Value::as_i64).unwrap_or(-1);
    if bank * 16 + slot != index as i64 {
        fail(format!("says bank {bank} program {slot}, expected index {index}"));
    }
    out
}

fn main() {
    let mut json_path = None;
    let mut syx_path = None;
    let mut check_only = false;
    for arg in std::env::args().skip(1) {
        if arg == "--check" {
            check_only = true;
        } else if arg.ends_with(".syx") {
            syx_path = Some(arg);
        } else {
            json_path = Some(arg);
        }
    }
    let Some(json_path) = json_path else {
        eprintln!("usage: teo5_rom <teo5_programs.json> [<TEO5_Factory_Programs_v1.00.syx>] [--check]");
        std::process::exit(2);
    };

    let text = match std::fs::read_to_string(&json_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{json_path}: {e}");
            std::process::exit(1);
        }
    };
    let decoded: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{json_path}: {e}");
            std::process::exit(1);
        }
    };
    let programs = match decoded.get("programs").and_then(Value::as_array) {
        Some(p) if p.len() == PROGRAM_COUNT => p,
        Some(p) => {
            eprintln!("{json_path}: {} programs, expected {PROGRAM_COUNT}", p.len());
            std::process::exit(1);
        }
        None => {
            eprintln!("{json_path}: no programs array");
            std::process::exit(1);
        }
    };

    let mut rom = Vec::with_capacity(PROGRAM_COUNT * KEPT);
    let mut report = String::new();
    let mut failures = 0usize;

    for (index, program) in programs.iter().enumerate() {
        let mut fail = |why: String| {
            failures += 1;
            let _ = writeln!(report, "  program {index}: {why}");
        };
        rom.extend_from_slice(&rebuild(program, index, &mut fail));
    }

    // The cross-check: unpack the original SysEx and demand that the decode
    // reproduces its first 190 bytes exactly, program for program.
    if let Some(path) = &syx_path {
        match std::fs::read(path) {
            Ok(raw) if raw.len() == MESSAGE * PROGRAM_COUNT => {
                for (index, message) in raw.chunks_exact(MESSAGE).enumerate() {
                    let mut fail = |why: String| {
                        failures += 1;
                        let _ = writeln!(report, "  program {index}: {why}");
                    };
                    if message[0] != 0xF0
                        || message[1] != 0x10
                        || message[2] != 0x5A
                        || message[3] != 0x02
                        || message[MESSAGE - 1] != 0xF7
                    {
                        fail(format!("header {:02X?} is not a TEO-5 program dump", &message[..6]));
                        continue;
                    }
                    if usize::from(message[4]) * 16 + usize::from(message[5]) != index {
                        fail(format!(
                            "SysEx says bank {} program {}, expected index {index}",
                            message[4], message[5]
                        ));
                    }
                    let payload = &message[6..6 + PACKED];
                    if payload.iter().any(|b| *b & 0x80 != 0) {
                        fail("payload has a byte with bit 7 set, which is not MIDI data".into());
                        continue;
                    }
                    let data = unpack(payload);
                    if data.len() != UNPACKED {
                        fail(format!("unpacked to {} bytes, expected {UNPACKED}", data.len()));
                        continue;
                    }
                    let built = &rom[index * KEPT..(index + 1) * KEPT];
                    if built != &data[..KEPT] {
                        let at = built
                            .iter()
                            .zip(&data[..KEPT])
                            .position(|(a, b)| a != b)
                            .unwrap_or(0);
                        fail(format!(
                            "decode and SysEx disagree at byte {at}: {} against {}",
                            built[at], data[at]
                        ));
                    }
                }
                println!("cross-checked against {path}");
            }
            Ok(raw) => {
                eprintln!(
                    "{path}: {} bytes, expected {} ({PROGRAM_COUNT} messages of {MESSAGE})",
                    raw.len(),
                    MESSAGE * PROGRAM_COUNT
                );
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                std::process::exit(1);
            }
        }
    }

    if failures > 0 {
        eprintln!("{failures} problem(s):\n{report}");
        std::process::exit(1);
    }

    println!("{PROGRAM_COUNT} programs, {} bytes", rom.len());
    let sample: Vec<String> = [0usize, 31, 128, 200, 255]
        .iter()
        .map(|i| {
            let name = String::from_utf8_lossy(&rom[i * KEPT + 159..i * KEPT + 179]);
            format!("{i}:{:?}", name.trim_end())
        })
        .collect();
    println!("  {}", sample.join(" "));

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/teo5_programs.bin");
    if check_only {
        match std::fs::read(&out) {
            Ok(existing) if existing == rom => println!("{} is up to date", out.display()),
            Ok(_) => {
                eprintln!("{} differs from the decoded bank", out.display());
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("{}: {e}", out.display());
                std::process::exit(1);
            }
        }
    } else if let Err(e) = std::fs::write(&out, &rom) {
        eprintln!("{}: {e}", out.display());
        std::process::exit(1);
    } else {
        println!("wrote {}", out.display());
    }
}
