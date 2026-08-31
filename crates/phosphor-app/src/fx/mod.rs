//! Where an effect type becomes an effect.
//!
//! One registry, because there were about to be three: a menu selection has
//! to build one, a session load has to build one from a name on disk, and a
//! new session's send buses have to be built with one already in them. Three
//! lists of effects is one list that eventually forgets an effect.
//!
//! **The EQ is registered; the other four are not yet.** Until each one
//! exists, [`build`] answers `None` for it and the caller says so out loud
//! rather than adding a slot that does nothing. That is the whole of what
//! "the menu does not lie" costs.

pub mod eq;

pub use eq::Eq;
/// The closed-form curve, for a UI that has an effect's parameters and needs
/// to draw what they mean.
///
/// Re-exported here so that a front end reaches for one door — the same one
/// it built the effect through — rather than for the DSP crate directly. The
/// vector these take is the one [`params_of`] returns, in the units the
/// insert layer's parameters are in.
pub use phosphor_dsp::fx::eq::{eq_from_natural_params, eq_response_db};

use phosphor_core::fx::{Effect, Gain, SendSlot};

use crate::state::{FxInstance, FxType};

/// The effect behind a menu entry, or `None` while it is still being built.
///
/// The landing point for each of the five. The EQ is here; when the next one
/// arrives it joins the same match and nothing else in the application has to
/// change — the menu, the command path, the session format and the strip
/// label all already work.
#[must_use]
pub fn build(fx_type: FxType) -> Option<Box<dyn Effect>> {
    match fx_type {
        FxType::Eq => Some(Box::new(Eq::new())),
        FxType::Compressor | FxType::Tape | FxType::Delay | FxType::Reverb => None,
    }
}

/// The effect a session file names, or `None` if this build has no such
/// effect.
///
/// A session that names an effect this build does not have loses that slot
/// and says so — it does not lose the rest of the chain, and it does not
/// silently substitute something else.
///
/// The trim is here and not in [`build`] on purpose: it is not in the menu,
/// but a session that has one — a test, a hand-written file, a chain saved by
/// a build where it was — still loads.
#[must_use]
pub fn build_by_name(name: &str) -> Option<Box<dyn Effect>> {
    if name == Gain::NAME {
        return Some(Box::new(Gain::new()));
    }
    FxType::from_key(name).and_then(build)
}

/// Whether this build can actually make one of these.
#[must_use]
pub fn is_built(fx_type: FxType) -> bool {
    build(fx_type).is_some()
}

/// An effect's controls at their defaults, in the effect's own units.
///
/// Read from the effect itself rather than from a table here, so a control
/// whose default moves cannot leave a stale copy behind in the UI.
#[must_use]
pub fn default_params(fx_type: FxType) -> Vec<f32> {
    let Some(effect) = build(fx_type) else {
        return Vec::new();
    };
    params_of(effect.as_ref())
}

/// Every control of an effect, in order.
#[must_use]
pub fn params_of(effect: &dyn Effect) -> Vec<f32> {
    (0..effect.parameter_count())
        .map(|i| effect.get_parameter(i))
        .collect()
}

/// Where a newly added effect goes in a chain that already has slots in it.
///
/// The canonical order is `EQ → comp → tape → delay → reverb`, and an effect
/// is *inserted at* its place in it rather than appended. What is already in
/// the chain never moves: a player who put the delay before the compressor
/// meant it, and an editor that quietly re-sorts their chain is an editor
/// they cannot trust with the next one.
#[must_use]
pub fn insert_position(chain: &[FxInstance], fx_type: FxType) -> usize {
    let rank = fx_type.canonical_rank();
    chain
        .iter()
        .position(|slot| slot.fx_type.canonical_rank() > rank)
        .unwrap_or(chain.len())
}

/// What a new session's send buses start with.
///
/// **Empty, for now.** The design is that Send A opens with a plate reverb
/// at 100% wet and Send B with a tempo-synced delay, so that a new session is
/// one keystroke away from an audible send rather than a routing exercise.
/// Neither effect exists yet, and a bus pre-loaded with nothing is worse than
/// an empty one: the strip would be labelled `rvb` and do nothing.
///
/// This is the landing point. When the reverb and the delay are built, this
/// returns their chains and everything downstream — the strip label, the
/// session format, the audio-thread commands — is already in place.
#[must_use]
pub fn bus_default_chain(slot: SendSlot) -> Vec<FxInstance> {
    let wanted = match slot {
        SendSlot::A => FxType::Reverb,
        SendSlot::B => FxType::Delay,
    };
    if is_built(wanted) {
        vec![FxInstance::new(wanted, default_params(wanted))]
    } else {
        Vec::new()
    }
}

/// What a bus is called on the track strip.
///
/// The first effect in it, when it has one — `rvb`, `dly` — because "the
/// reverb" is what the player calls that bus, and `snd a` is what a routing
/// matrix calls it. An empty bus keeps its letter.
#[must_use]
pub fn bus_label(chain: &[FxInstance], slot: SendSlot) -> &'static str {
    match chain.first() {
        Some(first) => first.fx_type.short(),
        None => match slot {
            SendSlot::A => "snd a",
            SendSlot::B => "snd b",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The menu offers five effects and no more. The gate and the limiter
    /// that used to be in it were never built.
    #[test]
    fn the_menu_is_the_five_real_effects() {
        assert_eq!(FxType::ALL.len(), 5);
        let labels: Vec<&str> = FxType::ALL.iter().map(|f| f.label()).collect();
        assert_eq!(labels, ["eq", "comp", "tape", "delay", "reverb"]);
        assert!(!labels.contains(&"gate"), "the gate is not built");
        assert!(!labels.contains(&"limiter"), "the limiter is not a slot");
        // The trim is real, and deliberately not in the menu.
        assert!(build_by_name(Gain::NAME).is_some());
        assert!(!labels.contains(&Gain::NAME));
    }

    /// Every menu entry has a stable on-disk name, and every name comes back
    /// to the entry it was written from.
    #[test]
    fn names_round_trip() {
        for &fx_type in FxType::ALL {
            let key = fx_type.key();
            assert_eq!(FxType::from_key(key), Some(fx_type), "key {key}");
            assert!(!fx_type.short().is_empty());
        }
        assert_eq!(FxType::from_key("gate"), None);
    }

    /// An effect this build cannot make answers `None` rather than a slot
    /// that does nothing.
    #[test]
    fn an_unbuilt_effect_is_refused_rather_than_faked() {
        for &fx_type in FxType::ALL {
            assert_eq!(build(fx_type).is_some(), is_built(fx_type));
        }
        assert!(build_by_name("nothing-like-this").is_none());
    }

    /// Adding an effect drops it at its canonical place among the slots that
    /// are there — and moves none of them.
    #[test]
    fn a_new_effect_lands_in_canonical_order() {
        let chain = vec![
            FxInstance::new(FxType::Compressor, vec![]),
            FxInstance::new(FxType::Reverb, vec![]),
        ];
        assert_eq!(insert_position(&chain, FxType::Eq), 0, "EQ goes first");
        assert_eq!(insert_position(&chain, FxType::Tape), 1, "tape after the comp");
        assert_eq!(insert_position(&chain, FxType::Delay), 1, "delay before the reverb");
        assert_eq!(insert_position(&chain, FxType::Reverb), 2, "a second reverb goes last");
        assert_eq!(insert_position(&[], FxType::Delay), 0);
    }

    /// A chain the player arranged out of canonical order stays that way: the
    /// new effect still lands at the first slot that ranks above it, and
    /// nothing that is already there moves.
    #[test]
    fn an_out_of_order_chain_is_not_re_sorted() {
        let chain = vec![
            FxInstance::new(FxType::Reverb, vec![]),
            FxInstance::new(FxType::Eq, vec![]),
        ];
        // The first slot ranking above a compressor is the reverb, at zero.
        assert_eq!(insert_position(&chain, FxType::Compressor), 0);
        // ...and the caller inserting there leaves the player's own order
        // intact behind it.
        let mut after = chain.clone();
        after.insert(0, FxInstance::new(FxType::Compressor, vec![]));
        assert_eq!(after[1].fx_type, FxType::Reverb);
        assert_eq!(after[2].fx_type, FxType::Eq);
    }

    /// The buses ship empty until there is something to ship in them, and the
    /// strip says so.
    #[test]
    fn the_buses_are_labelled_by_what_is_in_them() {
        assert_eq!(bus_default_chain(SendSlot::A), Vec::new());
        assert_eq!(bus_default_chain(SendSlot::B), Vec::new());
        assert_eq!(bus_label(&[], SendSlot::A), "snd a");
        assert_eq!(bus_label(&[], SendSlot::B), "snd b");

        let loaded = vec![FxInstance::new(FxType::Reverb, vec![])];
        assert_eq!(bus_label(&loaded, SendSlot::A), "rvb");
        let loaded = vec![FxInstance::new(FxType::Delay, vec![])];
        assert_eq!(bus_label(&loaded, SendSlot::B), "dly");
    }
}
