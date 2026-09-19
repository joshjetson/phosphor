//! Comparing dotted version strings, in one place.
//!
//! Two features want the same fact — "is this version newer than that one?".
//! The what's-new card asks it of the changelog against what was last seen; the
//! update check asks it of crates.io against what is running. A version string
//! is compared as a tuple of numbers, never as text: `"0.3.85"` is newer than
//! `"0.3.9"`, which a byte comparison gets exactly backwards because `'8' < '9'`.
//!
//! An unparseable version is not an error to shout about — it is a reason to do
//! nothing. Both callers treat "cannot read this" as "no news", so [`key`]
//! returns `None` and [`is_newer`] answers `false` rather than guessing.

/// A version split into its numeric components, or `None` when any component is
/// not a plain non-negative integer.
///
/// The components are kept as a `Vec` rather than a fixed tuple so that a
/// two-part or four-part string does not silently lose or gain a field; `[0, 3]`
/// and `[0, 3, 0]` then order the way arithmetic says they should.
#[must_use]
pub fn key(version: &str) -> Option<Vec<u32>> {
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    version.split('.').map(|part| part.parse::<u32>().ok()).collect()
}

/// Whether `candidate` is strictly newer than `baseline`.
///
/// Either side failing to parse answers `false`: with nothing trustworthy to
/// compare, there is no news to report. Equal versions are not newer.
#[must_use]
pub fn is_newer(candidate: &str, baseline: &str) -> bool {
    match (key(candidate), key(baseline)) {
        (Some(candidate), Some(baseline)) => candidate > baseline,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_larger_final_component_is_read_as_a_number_not_text() {
        // The byte comparison that this guards against: '8' < '9', so "0.3.85"
        // would sort *below* "0.3.9" as text. As numbers it is above it.
        assert!(is_newer("0.3.85", "0.3.9"));
        assert!(!is_newer("0.3.9", "0.3.85"));
    }

    #[test]
    fn equal_versions_are_not_newer_in_either_direction() {
        assert!(!is_newer("0.3.85", "0.3.85"));
    }

    #[test]
    fn a_higher_minor_beats_a_higher_patch() {
        assert!(is_newer("0.4.0", "0.3.99"));
        assert!(!is_newer("0.3.99", "0.4.0"));
    }

    #[test]
    fn a_missing_component_orders_below_its_longer_self() {
        assert_eq!(key("0.3"), Some(vec![0, 3]));
        assert!(is_newer("0.3.1", "0.3"));
        assert!(!is_newer("0.3", "0.3.1"));
    }

    #[test]
    fn a_nonnumeric_component_will_not_parse() {
        assert_eq!(key("0.3.x"), None);
        assert_eq!(key("v0.3.85"), None);
        assert_eq!(key(""), None);
        assert_eq!(key("   "), None);
    }

    #[test]
    fn an_unparseable_side_reports_no_news() {
        assert!(!is_newer("garbage", "0.3.85"));
        assert!(!is_newer("0.3.86", "garbage"));
        assert!(!is_newer("garbage", "rubbish"));
    }

    #[test]
    fn surrounding_whitespace_is_ignored() {
        // The last-seen file is a line of text a person might have edited; a
        // trailing newline must not turn a match into a mismatch.
        assert_eq!(key(" 0.3.85\n"), Some(vec![0, 3, 85]));
        assert!(!is_newer("0.3.85\n", " 0.3.85 "));
    }
}
