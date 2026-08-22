//! Builds `src/p6_programs.bin` from Sequential's factory SysEx file.
//!
//! ```text
//! cargo run -p phosphor-dsp --example p6_rom -- P6_Programs_v1.01.syx
//! cargo run -p phosphor-dsp --example p6_rom -- P6_Programs_v1.01.syx --check
//! ```
//!
//! The source is `P6_Programs_v1.01.syx`, the factory program set Sequential
//! published on 23 July 2015 and still distributes from the Prophet-6 support
//! page. 589,000 bytes, SHA-1 `3813fa0a41a260515cb5458ce081b373c545d55b`.
//! This program is the whole provenance of `p6_programs.bin`: run it against
//! that file and the bytes come out again.
//!
//! ## What it does
//!
//! The file is 500 SysEx messages of 1178 bytes each:
//!
//! ```text
//! F0 01 2D 02 <bank 0-4> <program 0-99> <1171 packed bytes> F7
//! ```
//!
//! `01` is the DSI manufacturer ID, `2D` the Prophet-6 family, `02` the
//! Program Data Dump opcode. The payload is in the packed-MS-bit format the
//! Operation Manual documents under *Packed Data Format*: eight-byte packets
//! whose first byte carries the stripped high bits of the seven that follow,
//! least significant bit first. 1171 packed bytes unpack to 1024.
//!
//! Of those 1024 only the first 127 are kept:
//!
//! | bytes | contents | kept |
//! |---|---|---|
//! | 0…106 | the program parameters | yes |
//! | 107…126 | the name, 20 ASCII characters, space padded | yes |
//! | 127 | unidentified, correlates with the trailer | no |
//! | 128…895 | the 64-step sequencer, 6 tracks | no — sequencing is the DAW's |
//! | 896…1023 | uninitialised firmware RAM, 8 distinct blobs in 500 dumps | no |
//!
//! so the output is 500 × 127 = 63,500 bytes, in bank-then-program order.
//! `prophet6.rs` reads it back with the byte map in its `raw` module.
//!
//! ## Checks
//!
//! Every message is checked for its header, its length and its terminator;
//! every unpacked block for the eleven parameters whose documented range is
//! narrow enough to catch a misaligned unpack (the two 0–164 cutoffs, the two
//! effect-type selectors, the LFO shape, unison and key mode, the two
//! arpeggiator selectors and BPM); and every name for printable ASCII. A
//! failure anywhere aborts without writing.

use std::fmt::Write as _;

const MESSAGE: usize = 1178;
const PACKED: usize = 1171;
const UNPACKED: usize = 1024;
const PROGRAM_COUNT: usize = 500;
/// Program parameters plus the name: the whole of what `prophet6.rs` reads.
const KEPT: usize = 127;

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

/// The parameters whose documented range is narrow enough that a byte out of
/// it means the unpack landed in the wrong place: `(offset, name, max)`.
const RANGE_CHECKS: &[(usize, &str, u8)] = &[
    (0, "osc 1 frequency", 60),
    (1, "osc 2 frequency", 60),
    (19, "low-pass cutoff", 164),
    (23, "high-pass cutoff", 164),
    (21, "low-pass key amount", 2),
    (25, "high-pass key amount", 2),
    (44, "effect A type", 5),
    (45, "effect B type", 9),
    (56, "effect A sync division", 10),
    (57, "effect B sync division", 10),
    (62, "LFO shape", 4),
    (85, "unison mode", 6),
    (86, "key mode", 5),
    (89, "arpeggiator mode", 4),
    (90, "arpeggiator range", 2),
    (92, "arpeggiator division", 9),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: p6_rom <P6_Programs_v1.01.syx> [--check]");
        std::process::exit(2);
    };
    let check_only = args.next().as_deref() == Some("--check");

    let raw = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    if raw.len() != MESSAGE * PROGRAM_COUNT {
        eprintln!(
            "{path}: {} bytes, expected {} ({PROGRAM_COUNT} messages of {MESSAGE})",
            raw.len(),
            MESSAGE * PROGRAM_COUNT
        );
        std::process::exit(1);
    }

    let mut rom = Vec::with_capacity(PROGRAM_COUNT * KEPT);
    let mut report = String::new();
    let mut failures = 0usize;

    for (index, message) in raw.chunks_exact(MESSAGE).enumerate() {
        let mut fail = |why: String| {
            failures += 1;
            let _ = writeln!(report, "  program {index}: {why}");
        };
        if message[0] != 0xF0
            || message[1] != 0x01
            || message[2] != 0x2D
            || message[3] != 0x02
            || message[MESSAGE - 1] != 0xF7
        {
            fail(format!("header {:02X?} is not a Prophet-6 program dump", &message[..6]));
        }
        let bank = message[4];
        let program = message[5];
        if usize::from(bank) * 100 + usize::from(program) != index {
            fail(format!("says bank {bank} program {program}, expected index {index}"));
        }
        let payload = &message[6..6 + PACKED];
        if payload.iter().any(|b| *b & 0x80 != 0) {
            fail("payload has a byte with bit 7 set, which is not MIDI data".into());
        }
        let data = unpack(payload);
        if data.len() != UNPACKED {
            fail(format!("unpacked to {} bytes, expected {UNPACKED}", data.len()));
        }
        for (offset, name, max) in RANGE_CHECKS {
            if data[*offset] > *max {
                fail(format!("{name} at byte {offset} is {} (max {max})", data[*offset]));
            }
        }
        let name = &data[107..127];
        if name.iter().any(|c| !(0x20..=0x7E).contains(c)) {
            fail(format!("name {name:02X?} is not printable ASCII"));
        }
        if name.iter().all(|c| *c == b' ') {
            fail("name is blank".into());
        }
        rom.extend_from_slice(&data[..KEPT]);
    }

    if failures > 0 {
        eprintln!("{failures} problem(s):\n{report}");
        std::process::exit(1);
    }

    println!("{PROGRAM_COUNT} programs, {} bytes", rom.len());
    let sample: Vec<String> = [0usize, 31, 191, 419, 499]
        .iter()
        .map(|i| {
            let name = String::from_utf8_lossy(&rom[i * KEPT + 107..i * KEPT + KEPT]);
            format!("{i}:{:?}", name.trim_end())
        })
        .collect();
    println!("  {}", sample.join(" "));

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/p6_programs.bin");
    if check_only {
        match std::fs::read(&out) {
            Ok(existing) if existing == rom => println!("{} is up to date", out.display()),
            Ok(_) => {
                eprintln!("{} differs from the SysEx file", out.display());
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
