//! The "what's new" changelog: parsing it, and remembering what was seen.
//!
//! `CHANGELOG.md` at the repository root is the single source. It is embedded
//! into the binary at build time so a shipped copy needs no file beside it, and
//! future releases append to the very same file the app reads — one place, not
//! two that can drift.
//!
//! The parse contract is deliberately small, so that writing a release note is
//! writing prose and nothing more:
//!
//! ```text
//! ## 0.3.85 — A short title
//!
//! One or more paragraphs of plain prose.
//!
//! ## 0.3.84 — The previous one
//!
//! ...
//! ```
//!
//! A section is a line beginning `## `, then its version, an em dash, and a
//! title; the body is everything up to the next such line. Anything that does
//! not fit — a header with no em dash, a version that is not a number — is
//! skipped rather than panicked on, because a malformed changelog is a cosmetic
//! problem and must never be one that stops the application from starting.
//!
//! The pure functions here take the changelog as an argument so they can be
//! tested without a file; only [`read_last_seen`] and [`write_last_seen`] touch
//! the disk, and they follow the same best-effort shape as the theme
//! preference — a home that cannot be found is a no-op, never a crash.

use crate::version;

/// The changelog, embedded from the repository root at build time.
///
/// The path climbs `src` → `phosphor-app` → `crates` → repository root, which is
/// where `CHANGELOG.md` lives; a test parses this constant to keep the wiring
/// honest if the file is ever moved.
pub const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// The file under the application directory that remembers the newest version
/// whose card has been dismissed.
const LAST_SEEN_FILE: &str = "last_seen_version";

/// One released version, as the card will show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The dotted version, e.g. `"0.3.85"`. Guaranteed numeric — a section
    /// whose version does not parse never becomes an `Entry`.
    pub version: String,
    /// The short title after the em dash.
    pub title: String,
    /// The prose body, blank lines between paragraphs preserved, with the
    /// leading and trailing whitespace trimmed off.
    pub body: String,
}

/// Every well-formed section, in the order written — which the file keeps
/// newest-first, and which this function does not reorder.
///
/// Malformed sections are dropped silently. The preamble before the first
/// `## ` header is not a section and is ignored.
#[must_use]
pub fn parse(changelog: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    // Group the file into blocks, each starting at a `## ` header line. A `# `
    // title or a `### ` sub-heading is not a section boundary: only exactly two
    // hashes and a space open one.
    let mut current: Option<(&str, Vec<&str>)> = None;
    for line in changelog.lines() {
        if is_section_header(line) {
            if let Some((header, body)) = current.take() {
                push_entry(&mut entries, header, &body);
            }
            current = Some((line, Vec::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push(line);
        }
    }
    if let Some((header, body)) = current.take() {
        push_entry(&mut entries, header, &body);
    }
    entries
}

/// Whether a line opens a section: `## `, but not `# ` or `### `.
fn is_section_header(line: &str) -> bool {
    line.starts_with("## ") && !line.starts_with("### ")
}

/// Parse one header + body pair and, if it is well-formed, add it.
fn push_entry(entries: &mut Vec<Entry>, header: &str, body: &[&str]) {
    let Some(entry) = parse_section(header, body) else { return };
    entries.push(entry);
}

/// One section, or `None` when its header does not name a numeric version and a
/// title separated by an em dash.
fn parse_section(header: &str, body: &[&str]) -> Option<Entry> {
    let rest = header.strip_prefix("## ")?;
    // The first em dash separates the version from the title. Splitting on the
    // first one, not the last, means a title may itself contain an em dash.
    let (version, title) = rest.split_once('\u{2014}')?;
    let version = version.trim();
    let title = title.trim();
    // A version that is not a dotted number is a malformed header, not a
    // version zero — skip the whole section rather than invent one.
    version::key(version)?;
    if title.is_empty() {
        return None;
    }
    Some(Entry {
        version: version.to_string(),
        title: title.to_string(),
        body: body.join("\n").trim().to_string(),
    })
}

/// The sections strictly newer than `last_seen`, newest first.
///
/// On a first-ever run — `last_seen` is `None`, or a file too garbled to
/// parse — only the newest section is returned, so a newcomer meets one card
/// and not the whole history. A version that cannot be compared is treated as
/// unseen, which is why an unreadable `last_seen` falls back to "just the
/// newest" rather than "everything".
#[must_use]
pub fn entries_since(changelog: &str, last_seen: Option<&str>) -> Vec<Entry> {
    let all = parse(changelog);
    match last_seen.and_then(version::key) {
        Some(seen) => all
            .into_iter()
            .filter(|entry| version::key(&entry.version).is_some_and(|k| k > seen))
            .collect(),
        None => all.into_iter().take(1).collect(),
    }
}

/// The card a launch of `current` should show, given what was last seen.
///
/// This is the whole show/don't decision as a function of its three inputs, so
/// it can be exercised without a disk: an empty result means "show nothing".
/// It is [`entries_since`] with one extra guard — never announce a version
/// newer than the binary actually running, so a working tree whose changelog is
/// ahead of the installed version does not promise features that are not there.
#[must_use]
pub fn whats_new(changelog: &str, current: &str, last_seen: Option<&str>) -> Vec<Entry> {
    let current_key = version::key(current);
    entries_since(changelog, last_seen)
        .into_iter()
        .filter(|entry| match (&current_key, version::key(&entry.version)) {
            (Some(cur), Some(entry_key)) => entry_key <= *cur,
            // A current version we cannot parse cannot clamp anything; keep the
            // entry rather than hide news over a fault that never happens with a
            // real `CARGO_PKG_VERSION`.
            _ => true,
        })
        .collect()
}

// ── Persistence ──

/// The last version whose card was dismissed, or `None` when the file is
/// absent, empty, or there is no home directory to read it from.
///
/// Best-effort by design: a missing file is the normal first-run state, not an
/// error, and the caller reads `None` the same way either way.
#[must_use]
pub fn read_last_seen() -> Option<String> {
    let dir = crate::paths::app_dir()?;
    let text = std::fs::read_to_string(dir.join(LAST_SEEN_FILE)).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Record `version` as the newest one whose card has been seen.
///
/// Written after the card is dismissed, not before it is shown: a crash between
/// showing and dismissing costs the player a second look at the same card, which
/// is a far kinder failure than never being told what changed. Errors are
/// swallowed for the same reason the theme preference swallows them — a home
/// that cannot be written is not worth interrupting a session over.
pub fn write_last_seen(version: &str) {
    if let Some(dir) = crate::paths::app_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(LAST_SEEN_FILE), version.trim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Changelog

Some preamble that is not a section.

## 0.3.85 — The newest one

The newest body. It has two sentences.

## 0.3.84 — An older one

An older body.

## 0.3.83 — The oldest one

The oldest body.
";

    #[test]
    fn every_well_formed_section_is_parsed_newest_first() {
        let entries = parse(SAMPLE);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].version, "0.3.85");
        assert_eq!(entries[0].title, "The newest one");
        assert_eq!(entries[0].body, "The newest body. It has two sentences.");
        assert_eq!(entries[2].version, "0.3.83");
    }

    #[test]
    fn the_preamble_is_not_a_section() {
        // Nothing before the first `## ` becomes an entry, and the `# Changelog`
        // title is a single hash, which is not a section boundary.
        let entries = parse(SAMPLE);
        assert!(entries.iter().all(|e| e.version.starts_with("0.3.")));
    }

    #[test]
    fn a_multi_paragraph_body_keeps_its_blank_line() {
        let text = "## 0.4.0 — Two paragraphs\n\nFirst paragraph.\n\nSecond paragraph.\n";
        let entries = parse(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn a_malformed_section_is_skipped_not_panicked() {
        // A header with no em dash, and one whose "version" is not a number:
        // both are dropped, and the good section around them still parses.
        let text = "\
## not a real header

## 0.3.90 — A good one

Good body.

## vNext — no number here

Ignored body.
";
        let entries = parse(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version, "0.3.90");
    }

    #[test]
    fn a_header_with_an_empty_title_is_skipped() {
        let text = "## 0.3.90 — \n\nBody without a title.\n";
        assert!(parse(text).is_empty());
    }

    #[test]
    fn entries_since_returns_only_strictly_newer_sections() {
        let since = entries_since(SAMPLE, Some("0.3.84"));
        assert_eq!(since.len(), 1);
        assert_eq!(since[0].version, "0.3.85");
    }

    #[test]
    fn the_last_seen_version_itself_is_not_repeated() {
        // Seeing 0.3.85 again shows nothing: strictly-newer excludes an equal.
        assert!(entries_since(SAMPLE, Some("0.3.85")).is_empty());
    }

    #[test]
    fn a_first_run_shows_only_the_newest_not_the_whole_history() {
        let first = entries_since(SAMPLE, None);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].version, "0.3.85");
    }

    #[test]
    fn an_unreadable_last_seen_falls_back_to_the_newest_only() {
        // A corrupt file must not become a wall of every version ever shipped.
        let garbled = entries_since(SAMPLE, Some("not-a-version"));
        assert_eq!(garbled.len(), 1);
        assert_eq!(garbled[0].version, "0.3.85");
    }

    #[test]
    fn whats_new_never_announces_a_version_newer_than_the_binary() {
        // The changelog is ahead of the running binary (a working tree between
        // release and build): 0.3.85's card must not appear when 0.3.84 runs.
        let shown = whats_new(SAMPLE, "0.3.84", Some("0.3.83"));
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].version, "0.3.84");
    }

    #[test]
    fn whats_new_shows_the_span_between_seen_and_running() {
        let shown = whats_new(SAMPLE, "0.3.85", Some("0.3.83"));
        let versions: Vec<&str> = shown.iter().map(|e| e.version.as_str()).collect();
        assert_eq!(versions, ["0.3.85", "0.3.84"]);
    }

    #[test]
    fn the_embedded_changelog_parses_and_leads_with_the_running_version() {
        // The workflow's contract, pinned: every release prepends its section,
        // so the newest entry is always the version being built. If this fails,
        // a release bumped the version and forgot the changelog.
        let entries = parse(CHANGELOG);
        assert!(!entries.is_empty(), "the embedded changelog must parse");
        assert_eq!(
            entries[0].version,
            env!("CARGO_PKG_VERSION"),
            "the newest changelog section must name the version being built"
        );
    }

    #[test]
    fn a_first_run_of_the_real_build_shows_exactly_the_running_version() {
        let shown = whats_new(CHANGELOG, env!("CARGO_PKG_VERSION"), None);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].version, env!("CARGO_PKG_VERSION"));
    }
}
