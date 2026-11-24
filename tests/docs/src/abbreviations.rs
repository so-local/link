//! Abbreviations learned along the way: a page spells an abbreviation
//! out at its first use, `Agent State Protocol (ASP)`, and abbreviates it
//! after. The abbreviations are `abbreviations.toml`'s.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::md::split;
use crate::site::is_page;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const RULES: &[Rule] = &[Rule {
    id: "abbreviations",
    scope: Scope::Site,
    fix: "spell an abbreviation out where a page first uses it, as \"Agent State Protocol \
          (ASP)\", and use only the abbreviation after; headings may spell it out",
    check: Check::Each(abbreviations),
}];

#[derive(Deserialize)]
struct File {
    abbreviation: BTreeMap<String, Abbreviation>,
}

#[derive(Deserialize)]
struct Abbreviation {
    means: String,
}

/// `abbreviations.toml`, read once.
fn known() -> &'static BTreeMap<String, Abbreviation> {
    static KNOWN: std::sync::OnceLock<BTreeMap<String, Abbreviation>> = std::sync::OnceLock::new();
    KNOWN.get_or_init(|| {
        toml::from_str::<File>(include_str!("../abbreviations.toml"))
            .expect("abbreviations.toml")
            .abbreviation
    })
}

fn abbreviations(tree: &Tree, path: &Path, text: &str) -> Vec<Finding> {
    if !is_page(tree, path) {
        return Vec::new();
    }
    let prose = plain(&crate::navigation::authored(text));
    known()
        .iter()
        .filter_map(|(short, a)| misuse(&prose, short, &a.means))
        .map(|why| fail(format!("{}: {why}", tree.shown(path))))
        .collect()
}

/// A page's prose on one line: no headings, code or link targets.
pub(crate) fn plain(text: &str) -> String {
    let lines: Vec<_> = split(text)
        .0
        .into_iter()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let joined = lines.join(" ");
    let mut out = String::new();
    let mut chars = joined.chars().peekable();
    let mut last = ' ';
    while let Some(c) = chars.next() {
        match c {
            '`' => {
                while chars.next_if(|n| *n != '`').is_some() {}
                chars.next();
            }
            '(' if last == ']' => {
                while chars.next_if(|n| *n != ')').is_some() {}
                chars.next();
            }
            c if c.is_whitespace() && last.is_whitespace() => continue,
            c => {
                let c = if c.is_whitespace() { ' ' } else { c };
                out.push(c);
                last = c;
                continue;
            }
        }
        last = ' ';
    }
    out
}

/// Why `prose` misuses `short`, if it does: abbreviated before it's spelled
/// out, or spelled out again after.
fn misuse(prose: &str, short: &str, means: &str) -> Option<String> {
    let mut uses: Vec<(usize, bool)> = prose
        .match_indices(means)
        .map(|(at, _)| (at, true))
        .collect();
    uses.extend(
        prose
            .match_indices(short)
            .filter(|(at, _)| word(prose, *at, short.len()))
            .map(|(at, _)| (at, false)),
    );
    uses.sort();
    let defines = |at: usize| {
        let rest = &prose[at + means.len()..];
        let rest = rest.strip_prefix("'s").unwrap_or(rest);
        rest.starts_with(&format!(" ({short}"))
    };
    let mut defined = false;
    for (at, full) in uses {
        match (full, defined) {
            (true, false) if defines(at) => defined = true,
            (true, false) => {}
            (true, true) => return Some(format!("{means} spelled out again after ({short})")),
            (false, false) => {
                return Some(format!(
                    "{short} before it's spelled out as {means} ({short})"
                ));
            }
            (false, true) => {}
        }
    }
    None
}

/// Whether `prose[at..at + len]` is a word of its own, not part of a name
/// such as `non-ACP` or `_asp`, and not the definition's own `(ASP)`.
fn word(prose: &str, at: usize, len: usize) -> bool {
    let before = prose[..at].chars().next_back();
    let after = prose[at + len..].chars().next();
    let part = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || "_-/.".contains(c));
    !part(before) && !part(after) && !prose[..at].ends_with('(')
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASP: &str = "Agent State Protocol";

    #[test]
    fn the_first_use_spells_it_out_and_the_rest_abbreviate() {
        assert_eq!(
            misuse("the Agent State Protocol (ASP), then ASP's", "ASP", ASP),
            None
        );
        assert_eq!(
            misuse("the Agent State Protocol's (ASP's) forms, ASP", "ASP", ASP),
            None
        );
        assert!(misuse("ASP, then Agent State Protocol (ASP)", "ASP", ASP).is_some());
        assert!(
            misuse(
                "Agent State Protocol (ASP) and Agent State Protocol",
                "ASP",
                ASP
            )
            .is_some()
        );
        assert_eq!(misuse("a non-ASP harness and `_asp`", "ASP", ASP), None);
    }

    #[test]
    fn code_link_targets_and_headings_are_not_prose() {
        let text = "# ASP\n\nUse `ASP` and [the Agent State Protocol (ASP)](asp.md#asp).\n";
        assert_eq!(misuse(&plain(text), "ASP", ASP), None);
        assert!(plain("```\nASP\n```\nprose\n").trim() == "prose");
    }
}
