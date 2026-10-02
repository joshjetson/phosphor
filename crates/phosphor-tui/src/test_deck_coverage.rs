//! Does the deck fit the app? Every key the app answers, against every key
//! the deck can send.
//!
//! The app's key handlers are read straight out of the source — the same
//! trick the sequencer's grep test uses — so a key added to any handler is
//! audited the day it is added, with nobody remembering to. Each key a
//! handler matches must be one of:
//!
//! * a key the deck sends (its bindings, read from the panel's own table),
//! * a key a deck control stands in for with the same action (the strip's
//!   M/S/R for `m`/`s`/`r` on the track list), or
//! * a gap, listed with its reason in `deck_gaps.txt`.
//!
//! The list is a ratchet in both directions: a new gap fails until it is
//! written down with a reason, and a gap that closes fails until it is struck
//! off — so the file is always the true list of what the hardware cannot yet
//! do. `cargo test deck_coverage -- --nocapture` prints it.

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashSet};

    use phosphor_app::surface::binding::{bind, nav_letter, Intent, Key, Nav};
    use phosphor_app::surface::layout::deck;

    /// One key as a handler matches it: the `KeyCode` spelling, and whether
    /// Ctrl is required.
    #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    struct Handled {
        key: String,
        ctrl: bool,
    }

    fn spell(key: Key) -> String {
        match key {
            Key::Char(c) => format!("Char('{c}')"),
            Key::Enter => "Enter".into(),
            Key::Esc => "Esc".into(),
            Key::Tab => "Tab".into(),
            Key::BackTab => "BackTab".into(),
        }
    }

    /// Everything the deck can send, from its own bindings.
    fn producible() -> HashSet<Handled> {
        let mut out = HashSet::new();
        let mut add = |key: String, ctrl: bool| {
            out.insert(Handled { key, ctrl });
        };
        for c in deck() {
            for shift in [false, true] {
                match bind(c.id, shift) {
                    Intent::Key(chord) => add(spell(chord.key), chord.ctrl),
                    Intent::Turn { up, down } => {
                        add(spell(up.key), up.ctrl);
                        add(spell(down.key), down.ctrl);
                    }
                    Intent::Nav { dir, stride } => {
                        add(format!("Char('{}')", nav_letter(dir, stride)), false);
                        // In a field being typed into the arrows are sent as
                        // arrows; see the deck module.
                        add(format!("{dir:?}"), false);
                    }
                    _ => {}
                }
            }
        }
        // VALUE's push is Enter.
        add("Enter".into(), false);
        let _ = Nav::Up;
        out
    }

    /// Keys a deck control stands in for with the same action, where the
    /// key itself is never sent: `(handler, key, the control)`.
    const STANDS_IN: &[(&str, &str, &str)] = &[
        ("handle_tracks_keys", "Char('m')", "M 1-5"),
        ("handle_tracks_keys", "Char('s')", "S 1-5"),
        ("handle_tracks_keys", "Char('r')", "R 1-5"),
        ("handle_tracks_keys", "Char('R')", "LOOP REC"),
        ("dispatch_event", "Char('=')", "TEMPO (the deck sends +)"),
    ];

    /// A key the handlers match, with where.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Site {
        handler: String,
        handled: Handled,
    }

    /// Read every key handler in the app's own source.
    fn handled_keys() -> Vec<Site> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
        let mut sites = BTreeSet::new();
        let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        paths.sort();
        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            // The deck module sends keys; it handles none.
            if !name.ends_with(".rs") || name == "deck.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let source = source.split("#[cfg(test)]").next().unwrap();
            let lines: Vec<&str> = source.lines().collect();
            let mut handler = String::from("?");
            for (i, line) in lines.iter().enumerate() {
                if let Some(at) = line.find("fn ") {
                    let rest = &line[at + 3..];
                    if let Some(end) = rest.find('(') {
                        let ident = rest[..end].trim();
                        if !ident.is_empty() && ident.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            handler = ident.to_string();
                        }
                    }
                }
                if line.trim_start().starts_with("//") {
                    continue;
                }
                // The arm, up to its `=>`: a guard naming Ctrl may sit on the
                // line after the key.
                let arm: String = lines[i..(i + 4).min(lines.len())].join(" ");
                let arm = arm.split("=>").next().unwrap_or("").to_string();
                let ctrl = arm.contains("ctrl") || arm.contains("CONTROL");
                let shift_guard = arm.contains("if shift");
                let mut rest = *line;
                while let Some(at) = rest.find("KeyCode::") {
                    rest = &rest[at + "KeyCode::".len()..];
                    for key in parse_key(rest, shift_guard) {
                        sites.insert(Site { handler: handler.clone(), handled: Handled { key, ctrl } });
                    }
                }
            }
        }
        sites.into_iter().collect()
    }

    /// The key a `KeyCode::…` names. A bare `Char(c)` is a field being typed
    /// into and names nothing; a range names every key in it.
    fn parse_key(rest: &str, shift_guard: bool) -> Vec<String> {
        let ident: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if ident != "Char" {
            return match ident.as_str() {
                "Enter" | "Esc" | "Tab" | "BackTab" | "Backspace" | "Delete" | "Up" | "Down" | "Left"
                | "Right" | "PageUp" | "PageDown" | "Home" | "End" => vec![ident],
                _ => vec![],
            };
        }
        let inner = &rest[5..];
        let close = inner.find(')').unwrap_or(inner.len());
        let inner = &inner[..close];
        let quoted: Vec<char> = inner
            .split('\'')
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .filter_map(|(_, s)| s.chars().next())
            .collect();
        let chars: Vec<char> = match quoted.as_slice() {
            [] => return vec![],
            [a, b] if inner.contains("..=") => (*a..=*b).collect(),
            list => list.to_vec(),
        };
        chars
            .into_iter()
            .map(|c| if shift_guard && c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c })
            .map(|c| format!("Char('{c}')"))
            .collect()
    }

    fn gaps() -> Vec<Site> {
        let can = producible();
        handled_keys()
            .into_iter()
            .filter(|s| !can.contains(&s.handled))
            .filter(|s| {
                !STANDS_IN
                    .iter()
                    .any(|(h, k, _)| *h == s.handler && *k == s.handled.key && !s.handled.ctrl)
            })
            .collect()
    }

    fn line_of(site: &Site) -> String {
        let ctrl = if site.handled.ctrl { "Ctrl+" } else { "" };
        format!("{}\t{ctrl}{}", site.handler, site.handled.key)
    }

    /// The list on file: `handler<TAB>key<TAB>reason`, `#` for comments.
    fn listed() -> Vec<(String, String)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("deck_gaps.txt");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        text.lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|l| {
                let mut parts = l.splitn(3, '\t');
                let site = format!("{}\t{}", parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                (site, parts.next().unwrap_or("").to_string())
            })
            .collect()
    }

    #[test]
    fn deck_coverage_every_key_the_app_answers_is_on_the_deck_or_on_the_list() {
        let found: BTreeSet<String> = gaps().iter().map(line_of).collect();
        let listed = listed();
        let known: BTreeSet<String> = listed.iter().map(|(s, _)| s.clone()).collect();
        let unexplained: Vec<_> = found.difference(&known).collect();
        let closed: Vec<_> = known.difference(&found).collect();
        let reasonless: Vec<_> = listed.iter().filter(|(_, r)| r.trim().is_empty()).map(|(s, _)| s).collect();

        let handled = handled_keys().len();
        println!("deck coverage: {handled} keys handled, {} reachable from the deck, {} gaps", handled - found.len(), found.len());
        for (site, reason) in &listed {
            println!("  gap  {site}\t{reason}");
        }
        assert!(
            unexplained.is_empty(),
            "keys the deck cannot send and nobody has written down — add each to deck_gaps.txt with a reason:\n{}",
            unexplained.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"),
        );
        assert!(closed.is_empty(), "gaps that are no longer gaps — strike them off deck_gaps.txt:\n{closed:?}");
        assert!(reasonless.is_empty(), "every gap needs its reason: {reasonless:?}");
    }

    /// The scanner itself: it must see the keys it is meant to see, or the
    /// audit above passes by reading nothing.
    #[test]
    fn the_scanner_reads_the_handlers_it_is_meant_to() {
        let sites = handled_keys();
        let has = |handler: &str, key: &str| sites.iter().any(|s| s.handler == handler && s.handled.key == key);
        assert!(sites.len() > 200, "only {} keys found — the scanner is reading nothing", sites.len());
        assert!(has("handle_trim_keys", "Char('w')"), "the trim strip's hug is invisible to the scanner");
        assert!(has("handle_sampler_keys", "Char('c')"));
        assert!(has("handle_tracks_keys", "Char('R')"));
        assert!(sites.iter().any(|s| s.handled.ctrl && s.handled.key == "Char('s')"), "Ctrl+S is invisible");
        assert_eq!(parse_key("Char(digit @ '1'..='3') =>", false), ["Char('1')", "Char('2')", "Char('3')"]);
        assert!(parse_key("Char(ch) =>", false).is_empty(), "a field's catch-all was read as a key");
    }
}
