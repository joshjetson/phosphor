//! Prints everything a saved session depends on, so that two builds can be
//! compared byte for byte.
//!
//! `cargo run -p phosphor-app --example session_digest -- sessions/*.phos`
//!
//! Not part of the product: this exists so that "a session saved before a
//! change loads identically after it" can be *checked* rather than asserted,
//! by running it in a worktree of the previous commit and diffing.

use phosphor_app::preset::{layout_fingerprint, param_count};
use phosphor_app::session;
use phosphor_app::state::InstrumentType;

fn main() {
    println!("== instrument layouts ==");
    for instrument in InstrumentType::ALL {
        let key = session::instrument_key(*instrument);
        println!(
            "{key:<10} params={:<3} layout={} label={:?}",
            param_count(*instrument),
            layout_fingerprint(*instrument),
            instrument.label()
        );
        // Every selector on the panel, and how many positions it has, because
        // that is what a stored position is an index into.
        let count = param_count(*instrument);
        let mut selectors = Vec::new();
        for param in 0..count {
            if let Some(positions) = phosphor_app::discrete::positions(*instrument, param) {
                selectors.push(format!("{param}:{}", positions.len()));
            }
        }
        println!("           selectors {}", selectors.join(" "));
    }

    for path in std::env::args().skip(1) {
        println!("\n== {path} ==");
        let file = match session::load(std::path::Path::new(&path)) {
            Ok(f) => f,
            Err(e) => {
                println!("  unreadable: {e}");
                continue;
            }
        };
        println!("  version {} tracks {}", file.version, file.tracks.len());
        for track in &file.tracks {
            let Some(instrument) = session::parse_instrument_type(&track.instrument_type) else {
                println!("  {:<12} UNKNOWN {}", track.name, track.instrument_type);
                continue;
            };
            let mut params = track.synth_params.clone();
            let clamped = session::apply_selectors(instrument, &mut params, &track.discrete);
            let digest: u64 = params.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, v| {
                v.to_bits().to_le_bytes().iter().fold(h, |h, b| {
                    (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
                })
            });
            println!(
                "  {:<12} {:<10} n={:<3} digest={digest:016x} clamped={clamped:?}",
                track.name, track.instrument_type, params.len()
            );
        }
    }
}
