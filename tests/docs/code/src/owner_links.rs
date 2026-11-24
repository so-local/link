//! A command another package owns links to its owner's section at its
//! first mention on a page past its lead (`owner-links`), so a page explains its own
//! function and sends the reader to the owner for the rest. The docs
//! writer inserts each link it can ([`updates`]): around the first
//! canonical command a page shows, on the CLI or in an agent's namespace,
//! or in place of the target of a
//! link already around it, to the page and anchor the command's `docs`
//! names. One inside a longer link's text is left for a person.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use link_docs_check::navigation::Update;
use link_docs_check::{Finding, Tree, fail};

use crate::coverage::{address, declared, package};

/// A declared command, with the section its owner names: none for one
/// that names none (plumbing, or no section yet), which a mention of it
/// matches all the same, so it never links to its parent's.
struct Owned {
    words: Vec<String>,
    package: PathBuf,
    section: Option<(PathBuf, String)>,
}

/// How a page's first mention of a command stands.
#[derive(Debug, PartialEq, Eq)]
enum Link {
    /// Plain: the writer wraps it in a link.
    None,
    /// Already a link's whole text, to `target`, from `start` to `end`.
    Wrapped {
        target: String,
        start: usize,
        end: usize,
    },
    /// Inside a longer link's text, where no link can go.
    Nested,
}

/// The first mention on a page of a command another package owns.
struct Mention {
    command: String,
    start: usize,
    end: usize,
    link: Link,
    wanted: String,
}

/// Every declared command, longest first, so `lnk agent harness-permission add` is
/// that command, not `lnk agent harness`.
fn owned(tree: &Tree) -> Vec<Owned> {
    let mut pages: BTreeMap<String, &PathBuf> = BTreeMap::new();
    for file in tree.files.keys() {
        if let Some(address) = address(file) {
            pages.entry(address).or_insert(file);
        }
    }
    let mut out: Vec<Owned> = declared(tree)
        .into_iter()
        .flat_map(|command| {
            let package = command.documentation_owner(tree);
            let section = command
                .action
                .docs
                .as_ref()
                .filter(|_| command.offered())
                .and_then(|docs| docs.split_once('#'))
                .and_then(|(address, anchor)| {
                    Some((pages.get(address)?.to_path_buf(), anchor.to_string()))
                });
            command
                .spellings()
                .into_iter()
                .map(|spelling| Owned {
                    words: spelling.split_whitespace().map(str::to_string).collect(),
                    package: package.clone(),
                    section: section.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    out.sort_by(|a, b| {
        b.words
            .len()
            .cmp(&a.words.len())
            .then(a.words.cmp(&b.words))
    });
    out
}

/// `to` as a link from a page in the folder `from`.
fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<Component> = from.components().collect();
    let to_parts: Vec<Component> = to.components().collect();
    let shared = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - shared];
    parts.extend(
        to_parts[shared..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// The byte ranges of a page's prose a mention may be in: outside code
/// blocks, headings, comments and the generated indexes and inventories,
/// and past its lead, the paragraph under its title, which the indexes
/// copy to pages elsewhere, where its links wouldn't resolve.
fn prose(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut fence = false;
    let mut generated = false;
    // Before the title, in its lead (and whether a line of it was seen),
    // or past it.
    let mut lead: Option<bool> = None;
    let mut past = false;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim_start();
        if !past {
            match lead {
                None if trimmed.starts_with("# ") => {
                    lead = Some(false);
                    continue;
                }
                // A summary quote under the title comes before the lead.
                Some(false) if trimmed.starts_with('>') => continue,
                Some(seen) if trimmed.is_empty() => {
                    past = seen;
                    continue;
                }
                Some(_) => {
                    lead = Some(true);
                    continue;
                }
                None => {}
            }
        }
        if trimmed.starts_with("```") {
            fence = !fence;
            continue;
        }
        if trimmed.starts_with("<!--") {
            if trimmed.contains(":start -->") {
                generated = true;
            } else if trimmed.contains(":end -->") {
                generated = false;
            }
            continue;
        }
        if fence || generated || trimmed.starts_with('#') {
            continue;
        }
        out.push((start, at));
    }
    out
}

/// The first mention of each command another package than the page's
/// owns, in `text`, a page at `file` of `package`.
fn mentions(file: &Path, package: &Path, text: &str, owned: &[Owned]) -> Vec<Mention> {
    let folder = file.parent().unwrap_or(Path::new(""));
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for (from, to) in prose(text) {
        let line = &text[from..to];
        let ticks: Vec<usize> = line.match_indices('`').map(|(i, _)| i).collect();
        for &[open, close] in ticks.as_chunks::<2>().0 {
            let span = &line[open + 1..close];
            let words: Vec<&str> = span.split_whitespace().collect();
            let Some(command) = owned.iter().find(|c| {
                c.words.len() <= words.len() && c.words.iter().zip(&words).all(|(a, b)| a == b)
            }) else {
                continue;
            };
            let name = command.words.join(" ");
            let Some((page, anchor)) = &command.section else {
                continue;
            };
            if !seen.insert(name.clone()) || command.package == package || page == file {
                continue;
            }
            let (start, end) = (from + open, from + close + 1);
            let before = &line[..open];
            let link = if before.ends_with('[') && line[close + 1..].starts_with("](") {
                let target_end = line[close + 3..].find(')').map(|i| close + 3 + i);
                match target_end {
                    Some(target_end) => Link::Wrapped {
                        target: line[close + 3..target_end].to_string(),
                        start: from + close + 3,
                        end: from + target_end,
                    },
                    None => Link::Nested,
                }
            } else if before.matches('[').count() > before.matches(']').count() {
                Link::Nested
            } else {
                Link::None
            };
            out.push(Mention {
                command: name,
                start,
                end,
                link,
                wanted: format!("{}#{}", relative(folder, page), anchor),
            });
        }
    }
    out
}

/// Each page of the workspace's websites with its package.
fn pages(tree: &Tree) -> impl Iterator<Item = (&PathBuf, &String, PathBuf)> {
    tree.files.iter().filter_map(|(file, text)| {
        // A generated page is its generator's, which writes its commands
        // plain.
        (file.starts_with(&tree.root)
            && address(file).is_some()
            && !link_docs_check::generated_reference(text))
        .then(|| Some((file, text, package(tree, file)?)))
        .flatten()
    })
}

/// The rule: each page's first mention of a command another package
/// owns links to the section its `docs` names.
pub fn check(tree: &Tree) -> Vec<Finding> {
    let owned = owned(tree);
    let mut found = Vec::new();
    for (file, text, package) in pages(tree) {
        for mention in mentions(file, &package, text, &owned) {
            let problem = match &mention.link {
                Link::Wrapped { target, .. } if *target == mention.wanted => continue,
                Link::None => "run lnk-docs-inventories --write to link it",
                Link::Wrapped { .. } => "it links elsewhere: run lnk-docs-inventories --write",
                Link::Nested => "it's inside another link: mention it outside one",
            };
            found.push(fail(format!(
                "{}: the first `{}` links to its owner's section, {}: {problem}",
                tree.shown(file),
                mention.command,
                mention.wanted
            )));
        }
    }
    found
}

/// Each page with the links the writer can insert: a plain first mention
/// wrapped in its link, and a link around it pointed at its owner's
/// section.
pub fn updates(tree: &Tree) -> Vec<Update> {
    let owned = owned(tree);
    let mut out = Vec::new();
    for (file, text, package) in pages(tree) {
        let mut edits: Vec<(usize, usize, String)> = mentions(file, &package, text, &owned)
            .into_iter()
            .filter_map(|m| match m.link {
                Link::None => {
                    let span = &text[m.start..m.end];
                    Some((m.start, m.end, format!("[{span}]({})", m.wanted)))
                }
                Link::Wrapped { target, start, end } if target != m.wanted => {
                    Some((start, end, m.wanted))
                }
                Link::Wrapped { .. } | Link::Nested => None,
            })
            .collect();
        if edits.is_empty() {
            continue;
        }
        edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
        let mut text = text.clone();
        for (start, end, link) in edits {
            text.replace_range(start..end, &link);
        }
        out.push(Update {
            path: file.clone(),
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_climbs_to_the_shared_folder() {
        assert_eq!(
            relative(Path::new("/r/a/docs/x"), Path::new("/r/b/docs/y/guide.md")),
            "../../../b/docs/y/guide.md"
        );
        assert_eq!(
            relative(Path::new("/r/a"), Path::new("/r/a/guide.md")),
            "guide.md"
        );
    }

    #[test]
    fn prose_leaves_out_code_headings_and_generated_lists() {
        let text = "# `lnk a`\n\nThe lead, `lnk a`.\n\nSee `lnk a`.\n\n```sh\nlnk a\n```\n\
                    <!-- inventory:x:start -->\n`lnk a`\n<!-- inventory:x:end -->\nEnd.\n";
        let shown: Vec<&str> = prose(text).into_iter().map(|(a, b)| &text[a..b]).collect();
        assert_eq!(shown, ["See `lnk a`.\n", "\n", "End.\n"]);
    }

    fn owned(words: &str, section: Option<&str>) -> Owned {
        Owned {
            words: format!("lnk {words}")
                .split(' ')
                .map(str::to_string)
                .collect(),
            package: PathBuf::from("/r/b"),
            section: section.map(|anchor| (PathBuf::from("/r/b/docs/guide.md"), anchor.into())),
        }
    }

    fn found(text: &str, owned: &[Owned]) -> Vec<(String, Link, String)> {
        let page = Path::new("/r/a/docs/page.md");
        mentions(page, Path::new("/r/a"), text, owned)
            .into_iter()
            .map(|m| (m.command, m.link, m.wanted))
            .collect()
    }

    #[test]
    fn a_mention_links_its_own_command_once_and_never_its_parents() {
        let commands = [
            owned("b answer", None),
            owned("b list", Some("list")),
            owned("b", Some("b")),
        ];
        let text = "# A\n\nLead.\n\nRun `lnk b list --json`, then `lnk b list`, \
                    `lnk b answer x` and [`lnk b`](elsewhere.md).\n";
        let found = found(text, &commands);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].0, "lnk b list");
        assert_eq!(found[0].1, Link::None);
        assert_eq!(found[0].2, "../../b/docs/guide.md#list");
        assert_eq!(found[1].0, "lnk b");
        assert!(matches!(&found[1].1, Link::Wrapped { target, .. } if target == "elsewhere.md"));
    }

    #[test]
    fn a_mention_of_its_own_packages_command_or_inside_a_longer_link_is_left() {
        let mut ours = owned("a x", Some("x"));
        ours.package = PathBuf::from("/r/a");
        let commands = [ours, owned("b", Some("b"))];
        let text = "# A\n\nLead.\n\nRun `lnk a x`, or [see `lnk b` there](y.md).\n";
        let found = found(text, &commands);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, Link::Nested);
    }
}
