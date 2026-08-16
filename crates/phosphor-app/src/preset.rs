//! User presets — one bank file per instrument type.
//!
//! A factory patch lives in a `const` table and is reachable from the patch
//! knob. A user preset is the whole parameter block as the player left it —
//! including whichever factory patch they started from — written to
//! `~/.phosphor/presets/<instrument>.json`.
//!
//! Presets are deliberately *not* appended to the factory bank. The patch
//! selector stores a normalised fraction, so the index it lands on depends on
//! how many entries the bank has; adding an entry would move every stored
//! value in every saved session. A preset bank sits beside the factory table
//! and moves nothing.
//!
//! Human-readable JSON with atomic writes (tmp + rename), the same shape as
//! the `.phos` session format.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::session::instrument_key;
use crate::state::InstrumentType;

// ── Limits ──

/// Most presets one instrument's bank will hold.
///
/// A hardware bank is this size and nobody dials 128 sounds by hand, so the
/// cap is not a limit anyone reaches on purpose. It exists so a held key or a
/// script cannot grow the file without bound, and so the browser's list has a
/// known maximum height.
pub const MAX_PRESETS: usize = 128;

/// Longest preset name, in characters.
pub const MAX_NAME_LEN: usize = 32;

/// Current preset file format version.
pub const FORMAT_VERSION: u32 = 1;

// ── Errors ──

/// Why a preset was refused. Every variant carries the numbers the caller
/// needs to say what went wrong rather than a pre-formatted sentence.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PresetError {
    #[error("saved for the {saved}, not the {wanted}")]
    WrongInstrument { saved: String, wanted: String },

    #[error("saved with {saved} controls, this instrument has {wanted}")]
    ParamCountMismatch { saved: usize, wanted: usize },

    #[error("saved against a different panel layout ({saved}, this build is {wanted})")]
    LayoutMismatch { saved: String, wanted: String },

    #[error("file claims {declared} controls but carries {actual}")]
    Corrupt { declared: usize, actual: usize },

    #[error("a preset needs a name")]
    NameEmpty,

    #[error("name is {len} characters, the limit is {max}")]
    NameTooLong { len: usize, max: usize },

    #[error("this instrument already has {max} presets — delete one first")]
    BankFull { max: usize },
}

// ── File format ──

/// One instrument's bank of user presets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetFile {
    pub version: u32,
    /// Instrument key this file belongs to, matching the `.phos` spelling.
    pub instrument: String,
    pub presets: Vec<Preset>,
}

/// A single stored preset.
///
/// `instrument`, `param_count` and `layout` are all redundant with the file
/// they sit in — deliberately. A preset that gets hand-copied between files,
/// or survives a parameter layout change, still carries enough to be refused
/// rather than loaded into the wrong panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub instrument: String,
    /// Fingerprint of the parameter layout this was saved against.
    pub layout: String,
    pub param_count: usize,
    pub params: Vec<f32>,
}

/// What `store` did with a name that may or may not have been in the bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    Added,
    /// The name was already taken and its slot was rewritten in place.
    Replaced,
}

// ── Parameter layout ──

/// The parameter names for an instrument, in panel order.
///
/// The sampler shares the phosphor synth's engine and therefore its panel;
/// its presets still live in their own file, because the two are separate
/// instruments as far as the player is concerned.
pub fn param_names(instrument: InstrumentType) -> &'static [&'static str] {
    match instrument {
        InstrumentType::Synth | InstrumentType::Sampler => &phosphor_dsp::synth::PARAM_NAMES,
        InstrumentType::DrumRack => &phosphor_dsp::drum_rack::PARAM_NAMES,
        InstrumentType::DX7 => &phosphor_dsp::dx7::PARAM_NAMES,
        InstrumentType::Jupiter8 => &phosphor_dsp::jupiter::PARAM_NAMES,
        InstrumentType::Odyssey => &phosphor_dsp::odyssey::PARAM_NAMES,
        InstrumentType::Juno60 => &phosphor_dsp::juno::PARAM_NAMES,
    }
}

/// A fingerprint of an instrument's parameter layout.
///
/// A count alone does not catch a reorder, and this project reorders: the
/// Juno went from 16 controls to 25, the Jupiter from 16 to 32 and the drum
/// rack from 6 to 35, and each of those rewrote the order of the slots it
/// already had into front-panel order. A preset saved against the old order
/// has the right number of values in the wrong holes, which is exactly the
/// silent-and-plausible failure the count check exists to prevent.
///
/// So the fingerprint is derived from the parameter *names* rather than
/// declared by hand: a version integer someone has to remember to bump is a
/// version integer that eventually does not get bumped. Renaming a control
/// invalidates presets that would otherwise still be correct — the trade is
/// deliberate, because a refusal is recoverable and a wrong sound is not.
///
/// FNV-1a rather than `DefaultHasher`, whose algorithm the standard library
/// explicitly reserves the right to change; this value goes in a file.
pub fn layout_fingerprint(instrument: InstrumentType) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET;
    for name in param_names(instrument) {
        // The separator keeps ["ab", "c"] from hashing as ["a", "bc"].
        for byte in name.bytes().chain(std::iter::once(0xff)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    format!("{hash:016x}")
}

/// How many parameters this instrument's panel has.
pub fn param_count(instrument: InstrumentType) -> usize {
    param_names(instrument).len()
}

// ── Paths ──

/// `~/.phosphor/presets`, or `None` when HOME is unset.
///
/// Same shape as the theme's config path: with no home directory there is
/// nowhere to put presets, and the answer is to do nothing rather than to
/// scatter files relative to the working directory.
pub fn default_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".phosphor").join("presets"))
}

/// The bank file for one instrument inside `dir`.
pub fn bank_path(dir: &Path, instrument: InstrumentType) -> PathBuf {
    dir.join(format!("{}.json", instrument_key(instrument)))
}

// ── Load / save ──

/// Read an instrument's bank. A bank that has never been written is empty,
/// not an error — that is the first-run case. A bank that exists but does not
/// parse *is* an error, so a corrupt file is reported rather than silently
/// replaced the next time the player saves.
pub fn load_bank(dir: &Path, instrument: InstrumentType) -> Result<PresetFile> {
    let path = bank_path(dir, instrument);
    if !path.exists() {
        return Ok(PresetFile::new(instrument));
    }
    let json = std::fs::read_to_string(&path)?;
    let bank: PresetFile = serde_json::from_str(&json)?;
    Ok(bank)
}

/// Write an instrument's bank. Atomic: tmp file then rename, so an interrupted
/// write cannot leave a half-written bank where the whole bank used to be.
pub fn save_bank(dir: &Path, instrument: InstrumentType, bank: &PresetFile) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = bank_path(dir, instrument);
    let json = serde_json::to_string_pretty(bank)?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &path)?;

    tracing::debug!("preset bank saved: {} ({} presets)", path.display(), bank.presets.len());
    Ok(())
}

// ── Bank operations ──

impl PresetFile {
    pub fn new(instrument: InstrumentType) -> Self {
        Self {
            version: FORMAT_VERSION,
            instrument: instrument_key(instrument).to_string(),
            presets: Vec::new(),
        }
    }

    /// Preset names in file order — what the browser lists.
    pub fn names(&self) -> Vec<&str> {
        self.presets.iter().map(|p| p.name.as_str()).collect()
    }

    /// Index of the preset with this name, if the bank holds one.
    ///
    /// Names are compared after trimming, so " pad" and "pad " are the same
    /// slot rather than two rows the player cannot tell apart.
    pub fn find(&self, name: &str) -> Option<usize> {
        let name = name.trim();
        self.presets.iter().position(|p| p.name == name)
    }

    /// Store `params` under `name`.
    ///
    /// A name already in the bank rewrites that slot in place rather than
    /// adding a second row: the browser lists by name, so two rows called
    /// "warm pad" are two rows the player cannot choose between, and `d`
    /// would delete an arbitrary one of them. Callers are expected to confirm
    /// before overwriting — `find` says whether they need to.
    pub fn store(
        &mut self,
        name: &str,
        instrument: InstrumentType,
        params: &[f32],
    ) -> Result<StoreOutcome, PresetError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PresetError::NameEmpty);
        }
        let len = name.chars().count();
        if len > MAX_NAME_LEN {
            return Err(PresetError::NameTooLong { len, max: MAX_NAME_LEN });
        }

        let preset = Preset {
            name: name.to_string(),
            instrument: instrument_key(instrument).to_string(),
            layout: layout_fingerprint(instrument),
            param_count: params.len(),
            params: params.to_vec(),
        };

        match self.find(name) {
            Some(idx) => {
                self.presets[idx] = preset;
                Ok(StoreOutcome::Replaced)
            }
            None => {
                // Only a new slot can overflow the bank; overwriting a name
                // stays legal at the cap, which is what a full bank needs to
                // remain usable.
                if self.presets.len() >= MAX_PRESETS {
                    return Err(PresetError::BankFull { max: MAX_PRESETS });
                }
                self.presets.push(preset);
                Ok(StoreOutcome::Added)
            }
        }
    }

    /// Remove the preset at `index`, returning it.
    pub fn remove(&mut self, index: usize) -> Option<Preset> {
        (index < self.presets.len()).then(|| self.presets.remove(index))
    }

    /// The parameter block at `index`, if it is safe to load into this
    /// instrument's current panel.
    pub fn params_at(
        &self,
        index: usize,
        instrument: InstrumentType,
        want_count: usize,
    ) -> Option<Result<&[f32], PresetError>> {
        let preset = self.presets.get(index)?;
        Some(preset.check(instrument, want_count).map(|()| preset.params.as_slice()))
    }
}

impl Preset {
    /// Whether this preset can be loaded into `instrument`'s current panel of
    /// `want_count` controls. Refuses rather than truncating or padding: a
    /// block that is the wrong length or the wrong order is a different panel,
    /// and copying it in slot by slot produces a plausible wrong sound with
    /// nothing on screen to say so.
    pub fn check(&self, instrument: InstrumentType, want_count: usize) -> Result<(), PresetError> {
        let wanted_key = instrument_key(instrument);
        if self.instrument != wanted_key {
            return Err(PresetError::WrongInstrument {
                saved: self.instrument.clone(),
                wanted: wanted_key.to_string(),
            });
        }
        if self.param_count != self.params.len() {
            return Err(PresetError::Corrupt {
                declared: self.param_count,
                actual: self.params.len(),
            });
        }
        if self.params.len() != want_count {
            return Err(PresetError::ParamCountMismatch {
                saved: self.params.len(),
                wanted: want_count,
            });
        }
        let wanted_layout = layout_fingerprint(instrument);
        if self.layout != wanted_layout {
            return Err(PresetError::LayoutMismatch {
                saved: self.layout.clone(),
                wanted: wanted_layout,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory of our own, so the tests never touch the player's
    /// `~/.phosphor` and can run beside each other.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("phosphor-presets-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn juno_panel() -> Vec<f32> {
        phosphor_dsp::juno::PARAM_DEFAULTS.to_vec()
    }

    /// A preset written to disk comes back with every value bit-identical.
    /// f32 through JSON is the part worth checking: a value that round-trips
    /// to within a rounding error is a filter cutoff that moved.
    #[test]
    fn a_preset_round_trips_through_the_file() {
        let dir = scratch("round-trip");
        let mut panel = juno_panel();
        panel[phosphor_dsp::juno::P_CUTOFF] = 0.317_25;
        panel[phosphor_dsp::juno::P_RESO] = 0.812_5;
        panel[phosphor_dsp::juno::P_PATCH] = 0.437_1;

        let mut bank = PresetFile::new(InstrumentType::Juno60);
        assert_eq!(
            bank.store("evening pad", InstrumentType::Juno60, &panel),
            Ok(StoreOutcome::Added)
        );
        save_bank(&dir, InstrumentType::Juno60, &bank).unwrap();

        let reopened = load_bank(&dir, InstrumentType::Juno60).unwrap();
        assert_eq!(reopened.names(), vec!["evening pad"]);
        let loaded = reopened
            .params_at(0, InstrumentType::Juno60, panel.len())
            .unwrap()
            .expect("its own panel should load");
        assert_eq!(loaded, panel.as_slice(), "the panel came back changed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A bank that has never been written is empty rather than an error —
    /// the first time a player opens the browser there is no file yet.
    #[test]
    fn a_bank_that_does_not_exist_is_empty() {
        let dir = scratch("missing");
        let bank = load_bank(&dir, InstrumentType::DX7).unwrap();
        assert!(bank.presets.is_empty());
        assert_eq!(bank.instrument, "dx7");
    }

    /// The case this whole mechanism exists for: a preset written when the
    /// Juno had 16 controls, opened after it grew to 25. Loading it would put
    /// 16 values into the first 16 of 25 holes — a plausible sound that is not
    /// the one that was saved.
    #[test]
    fn a_preset_with_the_wrong_control_count_is_refused() {
        let dir = scratch("count");
        let mut bank = PresetFile::new(InstrumentType::Juno60);
        bank.store("old panel", InstrumentType::Juno60, &juno_panel()).unwrap();
        // Rewrite it as the 16-control panel the Juno used to have.
        bank.presets[0].params.truncate(16);
        bank.presets[0].param_count = 16;
        save_bank(&dir, InstrumentType::Juno60, &bank).unwrap();

        let reopened = load_bank(&dir, InstrumentType::Juno60).unwrap();
        let want = param_count(InstrumentType::Juno60);
        assert_eq!(
            reopened.params_at(0, InstrumentType::Juno60, want).unwrap(),
            Err(PresetError::ParamCountMismatch { saved: 16, wanted: want })
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A count alone does not catch a reorder. The Odyssey and the phosphor
    /// synth are different panels; so is one instrument before and after its
    /// controls were shuffled into front-panel order.
    #[test]
    fn a_preset_from_a_reordered_panel_is_refused() {
        let panel = juno_panel();
        let mut preset = Preset {
            name: "reordered".into(),
            instrument: "juno60".into(),
            layout: layout_fingerprint(InstrumentType::Juno60),
            param_count: panel.len(),
            params: panel,
        };
        assert_eq!(preset.check(InstrumentType::Juno60, 25), Ok(()));

        // Same instrument, same count, panel shuffled: the fingerprint moves.
        preset.layout = "0000000000000000".into();
        assert!(matches!(
            preset.check(InstrumentType::Juno60, 25),
            Err(PresetError::LayoutMismatch { .. })
        ));
    }

    /// Reordering the names really does move the fingerprint — the property
    /// the layout check depends on.
    #[test]
    fn the_fingerprint_separates_every_instrument() {
        let mut seen = Vec::new();
        for inst in InstrumentType::ALL {
            let fp = layout_fingerprint(*inst);
            assert_eq!(fp.len(), 16, "{fp} is not a 64-bit fingerprint");
            seen.push((inst, fp));
        }
        // The sampler shares the phosphor synth's panel, so those two match by
        // design; every other pair is a different panel.
        for (a, fa) in &seen {
            for (b, fb) in &seen {
                let shared_panel = matches!(
                    (a, b),
                    (InstrumentType::Synth, InstrumentType::Sampler)
                        | (InstrumentType::Sampler, InstrumentType::Synth)
                );
                if a != b && !shared_panel {
                    assert_ne!(fa, fb, "{a:?} and {b:?} fingerprint the same");
                }
            }
        }
    }

    /// One file per instrument keeps a DX7 preset out of a Juno's browser, but
    /// a preset that gets hand-copied between files still has to be caught.
    #[test]
    fn a_preset_saved_for_another_instrument_is_refused() {
        let dir = scratch("instrument");
        let mut dx7 = PresetFile::new(InstrumentType::DX7);
        dx7.store("e.piano", InstrumentType::DX7, &phosphor_dsp::dx7::PARAM_DEFAULTS).unwrap();

        // As if the player pasted the entry into juno60.json by hand.
        let mut juno = PresetFile::new(InstrumentType::Juno60);
        juno.presets.push(dx7.presets[0].clone());
        save_bank(&dir, InstrumentType::Juno60, &juno).unwrap();

        let reopened = load_bank(&dir, InstrumentType::Juno60).unwrap();
        assert_eq!(
            reopened.params_at(0, InstrumentType::Juno60, 9).unwrap(),
            Err(PresetError::WrongInstrument {
                saved: "dx7".into(),
                wanted: "juno60".into()
            }),
            "a DX7 preset loaded into a Juno"
        );

        // ...and the bank files are separate to begin with.
        assert_ne!(
            bank_path(&dir, InstrumentType::DX7),
            bank_path(&dir, InstrumentType::Juno60)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Saving under a name the bank already holds rewrites that slot instead
    /// of adding a second row with the same label.
    #[test]
    fn saving_over_a_name_replaces_it_in_place() {
        let mut bank = PresetFile::new(InstrumentType::Juno60);
        let mut first = juno_panel();
        first[phosphor_dsp::juno::P_CUTOFF] = 0.2;
        let mut second = juno_panel();
        second[phosphor_dsp::juno::P_CUTOFF] = 0.9;

        bank.store("brass", InstrumentType::Juno60, &first).unwrap();
        bank.store("strings", InstrumentType::Juno60, &juno_panel()).unwrap();
        assert_eq!(
            bank.store("brass", InstrumentType::Juno60, &second),
            Ok(StoreOutcome::Replaced)
        );

        assert_eq!(bank.names(), vec!["brass", "strings"], "the slot moved or duplicated");
        assert_eq!(bank.presets[0].params[phosphor_dsp::juno::P_CUTOFF], 0.9);

        // Whitespace is trimmed, so " brass" is the same slot rather than a
        // second row the player cannot tell from the first.
        assert_eq!(
            bank.store("  brass  ", InstrumentType::Juno60, &first),
            Ok(StoreOutcome::Replaced)
        );
        assert_eq!(bank.presets.len(), 2);
    }

    /// The bank has a ceiling, and a full bank can still be overwritten.
    #[test]
    fn the_bank_stops_at_its_limit() {
        let mut bank = PresetFile::new(InstrumentType::Juno60);
        let panel = juno_panel();
        for i in 0..MAX_PRESETS {
            bank.store(&format!("p{i}"), InstrumentType::Juno60, &panel).unwrap();
        }
        assert_eq!(
            bank.store("one more", InstrumentType::Juno60, &panel),
            Err(PresetError::BankFull { max: MAX_PRESETS })
        );
        assert_eq!(
            bank.store("p0", InstrumentType::Juno60, &panel),
            Ok(StoreOutcome::Replaced),
            "a full bank became read-only"
        );

        bank.remove(0);
        assert_eq!(bank.presets.len(), MAX_PRESETS - 1);
        assert_eq!(
            bank.store("one more", InstrumentType::Juno60, &panel),
            Ok(StoreOutcome::Added)
        );
    }

    /// A name has to be something a player can pick out of a list.
    #[test]
    fn names_are_bounded_and_non_empty() {
        let mut bank = PresetFile::new(InstrumentType::Juno60);
        let panel = juno_panel();
        assert_eq!(
            bank.store("   ", InstrumentType::Juno60, &panel),
            Err(PresetError::NameEmpty)
        );
        let long = "x".repeat(MAX_NAME_LEN + 1);
        assert_eq!(
            bank.store(&long, InstrumentType::Juno60, &panel),
            Err(PresetError::NameTooLong { len: MAX_NAME_LEN + 1, max: MAX_NAME_LEN })
        );
        assert!(bank.presets.is_empty());
    }

    /// A hand-edited file whose declared count disagrees with what it carries
    /// is refused rather than trusted.
    #[test]
    fn a_preset_that_contradicts_itself_is_refused() {
        let panel = juno_panel();
        let preset = Preset {
            name: "hand edited".into(),
            instrument: "juno60".into(),
            layout: layout_fingerprint(InstrumentType::Juno60),
            param_count: 99,
            params: panel.clone(),
        };
        assert_eq!(
            preset.check(InstrumentType::Juno60, panel.len()),
            Err(PresetError::Corrupt { declared: 99, actual: panel.len() })
        );
    }

    /// Every instrument's declared panel size matches the block a new track
    /// gets, so no instrument is born unable to save a preset.
    #[test]
    fn every_instrument_has_a_panel() {
        for inst in InstrumentType::ALL {
            assert!(param_count(*inst) > 0, "{inst:?} has no parameters");
        }
        assert_eq!(param_count(InstrumentType::Juno60), phosphor_dsp::juno::PARAM_COUNT);
        assert_eq!(param_count(InstrumentType::Jupiter8), phosphor_dsp::jupiter::PARAM_COUNT);
        assert_eq!(param_count(InstrumentType::DX7), phosphor_dsp::dx7::PARAM_COUNT);
        assert_eq!(param_count(InstrumentType::Odyssey), phosphor_dsp::odyssey::PARAM_COUNT);
        assert_eq!(param_count(InstrumentType::DrumRack), phosphor_dsp::drum_rack::PARAM_COUNT);
        assert_eq!(param_count(InstrumentType::Synth), phosphor_dsp::synth::PARAM_COUNT);
        assert_eq!(param_count(InstrumentType::Sampler), phosphor_dsp::synth::PARAM_COUNT);
    }
}
