//! Where an effect type becomes an effect.
//!
//! One registry, because there were about to be three: a menu selection has
//! to build one, a session load has to build one from a name on disk, and a
//! new session's send buses have to be built with one already in them. Three
//! lists of effects is one list that eventually forgets an effect.
//!
//! **The EQ, the compressor, the reverb and the delay are registered; the
//! tape is not yet.** Until it exists, [`build`] answers `None` for it and the
//! caller says so out loud rather than adding a slot that does nothing. That
//! is the whole of what "the menu does not lie" costs.

pub mod compressor;
pub mod delay;
pub mod eq;
pub mod reverb;

pub use compressor::Compressor;
pub use delay::Delay;
pub use eq::Eq;
pub use reverb::Reverb;
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
/// The landing point for each of the five. Four are here; when the last one
/// arrives it joins the same match and nothing else in the application has to
/// change — the menu, the command path, the session format and the strip
/// label all already work.
#[must_use]
pub fn build(fx_type: FxType) -> Option<Box<dyn Effect>> {
    match fx_type {
        FxType::Eq => Some(Box::new(Eq::new())),
        FxType::Compressor => Some(Box::new(Compressor::new())),
        FxType::Reverb => Some(Box::new(Reverb::new())),
        FxType::Delay => Some(Box::new(Delay::new())),
        FxType::Tape => None,
    }
}

/// The whole parameter vector a character names, for a front end that is
/// recalling one.
///
/// Re-exported through this door rather than reached for in the DSP crate
/// directly, for the same reason the EQ's closed-form curve is: a caller
/// builds an effect through this module and should read what it means through
/// the same one.
///
/// **Recalling a character is the front end's job and not the effect's.**
/// [`phosphor_dsp::fx::compressor::Compressor::set_param_natural`] writes one
/// control and never eleven, so a session load cannot depend on the order the
/// controls happen to be written in — and the front end, which has to update
/// its own mirror anyway, writes all twelve.
pub use phosphor_dsp::fx::compressor::{
    character_params, matches_character, CHARACTERS, CHARACTER_COUNT,
};

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
/// **Send A opens with the plate reverb and Send B with the synced delay,
/// both at 100% wet.** A new session is then one keystroke away from two
/// audible sends — turn either one up on any track — rather than a routing
/// exercise, and the strips label the buses `rvb` and `dly` because that is
/// what a player calls them.
///
/// The delay arrives as it ships: synced on, a dotted eighth, 30% feedback,
/// loop filters at 200 Hz and 6 kHz. Those are the settings that make a send
/// delay sit *behind* the source rather than beside it, and they are the
/// difference between the send being used and being turned off again.
///
/// **The wet/dry override is the point of this function.** Every time-based
/// effect ships with an insert default — 25% for the reverb, 22% for the
/// delay — because that is what it should sound like when it is dropped
/// straight onto a track. On a bus the dry path arrives by another route, so
/// anything less than 100% wet is the send-made-it-phasey trap: the same
/// signal, twice, a few milliseconds apart.
#[must_use]
pub fn bus_default_chain(slot: SendSlot) -> Vec<FxInstance> {
    let wanted = match slot {
        SendSlot::A => FxType::Reverb,
        SendSlot::B => FxType::Delay,
    };
    if !is_built(wanted) {
        return Vec::new();
    }
    let mut params = default_params(wanted);
    if let Some(index) = wet_dry_index(wanted) {
        if let Some(mix) = params.get_mut(index) {
            *mix = 100.0;
        }
    }
    vec![FxInstance::new(wanted, params)]
}

/// Which of an effect's controls is its wet/dry, when it has one.
///
/// A short list rather than a convention on the trait: an effect's parameter
/// *names* are its own business, and the alternative — searching for a
/// control called "mix" — would silently pick up a compressor's parallel-blend
/// knob the day one is added and set it to 100% on a bus, which is a very
/// different instruction.
#[must_use]
pub fn wet_dry_index(fx_type: FxType) -> Option<usize> {
    match fx_type {
        FxType::Reverb => Some(phosphor_dsp::fx::reverb::PARAM_MIX),
        FxType::Delay => Some(phosphor_dsp::fx::delay::PARAM_MIX),
        FxType::Eq | FxType::Compressor | FxType::Tape => None,
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

    /// Every built effect survives being built twice, with the same controls.
    #[test]
    fn a_built_effect_reports_the_registrys_own_defaults() {
        for fx_type in [FxType::Eq, FxType::Compressor, FxType::Reverb, FxType::Delay] {
            let effect = build(fx_type).expect("built");
            assert_eq!(effect.name(), fx_type.key());
            assert_eq!(params_of(effect.as_ref()), default_params(fx_type));
            assert_eq!(build_by_name(fx_type.key()).map(|e| e.name()), Some(fx_type.key()));
        }
    }

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

    /// The buses are labelled by what is in them, and both of them now have
    /// something in them.
    #[test]
    fn the_buses_are_labelled_by_what_is_in_them() {
        let send_a = bus_default_chain(SendSlot::A);
        assert_eq!(send_a.len(), 1);
        assert_eq!(send_a[0].fx_type, FxType::Reverb);
        assert_eq!(bus_label(&send_a, SendSlot::A), "rvb");

        let send_b = bus_default_chain(SendSlot::B);
        assert_eq!(send_b.len(), 1);
        assert_eq!(send_b[0].fx_type, FxType::Delay);
        assert_eq!(bus_label(&send_b, SendSlot::B), "dly");

        // An emptied bus goes back to its letter.
        assert_eq!(bus_label(&[], SendSlot::A), "snd a");
        assert_eq!(bus_label(&[], SendSlot::B), "snd b");
    }

    /// **Send B opens with the delay, synced, fully wet.**
    ///
    /// The bootstrap the strip reads `dly` from, checked control by control
    /// against the delay's own factory settings so that a default which moves
    /// cannot leave a stale copy in the bus.
    #[test]
    fn the_send_b_delay_is_synced_and_fully_wet() {
        use phosphor_dsp::fx::delay::{
            SYNC_DEFAULT, SYNC_LABELS, PARAM_DIVISION, PARAM_FEEDBACK, PARAM_HIGH_CUT_HZ,
            PARAM_LOW_CUT_HZ, PARAM_MIX, PARAM_ROUTING, PARAM_SYNC,
        };

        let bus = bus_default_chain(SendSlot::B);
        let params = &bus[0].params;
        assert_eq!(params.len(), phosphor_dsp::fx::delay::PARAM_COUNT);
        assert!(!bus[0].bypass, "the shipped delay arrived bypassed");
        assert_eq!(params[PARAM_MIX], 100.0, "a send bus must be fully wet");
        assert_eq!(params[PARAM_SYNC], 1.0, "the send delay is not synced");
        assert_eq!(params[PARAM_DIVISION], SYNC_DEFAULT as f32);
        assert_eq!(SYNC_LABELS[SYNC_DEFAULT], "1/8D");
        assert_eq!(params[PARAM_FEEDBACK], 30.0);
        assert_eq!(params[PARAM_LOW_CUT_HZ], 200.0, "the loop filters ship on");
        assert_eq!(params[PARAM_HIGH_CUT_HZ], 6_000.0);
        assert_eq!(params[PARAM_ROUTING], 0.0, "ping-pong ships off");

        // ...and nothing else about it moved off the insert default.
        let insert = default_params(FxType::Delay);
        assert_eq!(insert[PARAM_MIX], 22.0, "an insert delay ships at 22% wet");
        for (index, (a, b)) in insert.iter().zip(params).enumerate() {
            if index != PARAM_MIX {
                assert_eq!(a, b, "index {index} differs between insert and bus");
            }
        }
    }

    /// **A send bus is 100% wet, and an insert is not.**
    ///
    /// The dry signal reaches the master by its own path, so a bus that
    /// passed any of it would be sending the same signal twice a few
    /// milliseconds apart — which is the phasey-send trap, and it is the
    /// reason a player who tries a send once never tries it again.
    #[test]
    fn the_send_bus_reverb_is_fully_wet_and_the_insert_default_is_not() {
        let mix = wet_dry_index(FxType::Reverb).expect("the reverb has a wet/dry");
        let insert = default_params(FxType::Reverb);
        assert_eq!(insert[mix], 25.0, "an insert reverb ships at 25% wet");

        let bus = bus_default_chain(SendSlot::A);
        assert_eq!(bus[0].params[mix], 100.0, "a bus reverb ships fully wet");
        // ...and nothing else about it moved.
        for (index, (a, b)) in insert.iter().zip(&bus[0].params).enumerate() {
            if index != mix {
                assert_eq!(a, b, "index {index} differs between insert and bus");
            }
        }
        assert!(!bus[0].bypass, "the bus reverb ships in the signal path");

        // An effect with no wet/dry is not given one.
        assert_eq!(wet_dry_index(FxType::Eq), None);
    }
}
