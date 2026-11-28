//! The docs' structure: how a page is reached and how deep it sits, and
//! how much a package says for its code. Each rule names what it held
//! when it came in, by its id, in `large-files.toml`'s `[held]`; a held
//! page or package may keep the problem, and leaves the list once it
//! doesn't. The measures are public, so `lnk-docs-survey` reports the same
//! numbers the rules hold.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::md;
use crate::prose::{Block, blocks, characters, named};
use crate::{Check, Finding, Rule, Scope, Tree, fail};

mod metrics;
mod projections;
use metrics::docs_ratio;
pub use metrics::{Measure, measures, packages};

pub const RULES: &[Rule] = &[
    Rule {
        id: "introduced",
        scope: Scope::Site,
        fix: "a page is introduced in a paragraph of the page above it, its address's parent: \
              say there why the page exists and link to it. A list, a table or a generated \
              index is no introduction",
        check: Check::Whole(introduced),
    },
    Rule {
        id: "duplicates",
        scope: Scope::Site,
        fix: "a paragraph has one home: keep it on the page that owns its subject and link \
              there from the others",
        check: Check::Whole(duplicates),
    },
    Rule {
        id: "heading-levels",
        scope: Scope::Site,
        fix: "a heading is one level below the heading it's under: `##` under `#`, `###` \
              under `##`",
        check: Check::Each(heading_levels),
    },
    Rule {
        id: "depth",
        scope: Scope::Site,
        fix: "a page's address and its headings make one tree: lift its deepest headings, or \
              move the page nearer the top",
        check: Check::Whole(depth),
    },
    Rule {
        id: "docs-ratio",
        scope: Scope::Every,
        fix: "a package's docs grow with its Rust: explain what the thin one does, or move \
              what the heavy one says to the page that owns it",
        check: Check::Whole(docs_ratio),
    },
    Rule {
        id: "follows-code",
        scope: Scope::Site,
        fix: "a package's pages sit under its parent package's pages: move them there, or \
              move the package to the folder its pages say it belongs in",
        check: Check::Whole(follows_code),
    },
    Rule {
        id: "hook-words",
        scope: Scope::Site,
        fix: "a hook says why the page is worth reading, in a few words beyond its keyword: \
              never the title alone",
        check: Check::Whole(hook_words),
    },
    Rule {
        id: "hook-case",
        scope: Scope::Site,
        fix: "a hook is the title the website shows: Title Case, each word capitalized but the \
              short words inside it",
        check: Check::Whole(hook_case),
    },
    Rule {
        id: "held",
        scope: Scope::Every,
        fix: "large-files.toml's [held] lists, under a structure rule's id, the pages and \
              packages it held when it came in: take out these, whose rule or file is gone",
        check: Check::Whole(held_exist),
    },
];

/// The rules `[held]` may list under: these, and `link-docs-check-code`'s
/// four, whose entries are `<actions.toml>#<name>`.
const HOLDING: &[&str] = &[
    "introduced",
    "duplicates",
    "depth",
    "docs-ratio",
    "follows-code",
    "hook-words",
    "hook-case",
    "words",
    "retired-words",
    "command-pages",
    "command-folders",
    "variable-pages",
    "state-pages",
    "group-pages",
    "kind-leaves",
    "module-pages",
];

/// Each `[held]` entry names a rule that holds, and a file or folder of
/// the workspace: a gone one would sit in the list forever.
fn held_exist(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for (rule, items) in &tree.large.held {
        if !HOLDING.contains(&rule.as_str()) {
            found.push(fail(format!(
                "[held] {rule}: no structure rule holds a list"
            )));
            continue;
        }
        for item in items {
            let file = item.split('#').next().unwrap_or_default();
            let path = tree.root.join(file);
            if !tree.files.contains_key(&path) && !tree.dirs.contains(&path) {
                found.push(fail(format!("[held] {rule}: {item} is gone")));
            }
        }
    }
    found
}

/// Each page of the websites by its file: its address, from the
/// website's own (`/agents/link`, `/` for the home page).
pub fn addresses(tree: &Tree) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    for (address, files) in &tree.pages {
        let Some(website) = tree.websites.iter().find(|w| address.starts_with(&w.url)) else {
            continue;
        };
        let path = match &address[website.url.len()..] {
            "" => "/".to_string(),
            rest => rest.to_string(),
        };
        for file in files {
            out.insert(file.clone(), path.clone());
        }
    }
    out
}

/// The address above `address`, `/` above a top one.
pub fn parent_address(address: &str) -> Option<&str> {
    if address == "/" {
        return None;
    }
    Some(match address.rfind('/') {
        Some(0) | None => "/",
        Some(at) => &address[..at],
    })
}

/// How many levels below the home page an address is: `/agents/link` is 2.
pub fn address_depth(address: &str) -> usize {
    address.matches('/').count() - usize::from(address == "/")
}

/// The pages a file's links lead to, among `pages`, by the kind of block
/// each link is in: paragraphs, lists or tables.
pub fn links_by_block(file: &Path, text: &str, pages: &BTreeSet<&PathBuf>) -> Links {
    let mut found = Links::default();
    for block in blocks(text) {
        let (kind, lines): (&mut BTreeSet<PathBuf>, Vec<String>) = match block {
            Block::Paragraph(text) => (&mut found.paragraphs, vec![text]),
            Block::List(items) => (&mut found.lists, items),
            Block::Table(rows) => (&mut found.tables, rows.concat()),
            _ => continue,
        };
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        for target in md::links(&lines) {
            if let Some(page) = resolve(file, target, pages) {
                kind.insert(page);
            }
        }
    }
    found
}

/// A file's links to pages, by the kind of block they're in.
#[derive(Default)]
pub struct Links {
    pub paragraphs: BTreeSet<PathBuf>,
    pub lists: BTreeSet<PathBuf>,
    pub tables: BTreeSet<PathBuf>,
}

/// The page a relative link from `file` leads to: a file, or a folder's
/// `README.md`.
fn resolve(file: &Path, target: &str, pages: &BTreeSet<&PathBuf>) -> Option<PathBuf> {
    let target = target.split(['#', '?']).next()?;
    if target.is_empty() || target.contains(':') {
        return None;
    }
    let path = crate::site::normal(&file.parent()?.join(target));
    [path.clone(), path.join("README.md")]
        .into_iter()
        .find(|p| pages.contains(p))
}

/// Every page of the websites is introduced in a paragraph of the page at
/// its parent address, the nearest one that's a page: following those
/// links from the home page reaches every page, each with a sentence on
/// why it's there.
fn introduced(tree: &Tree) -> Vec<Finding> {
    let addresses = addresses(tree);
    let by_address: BTreeMap<&str, &PathBuf> =
        addresses.iter().map(|(f, a)| (a.as_str(), f)).collect();
    let pages: BTreeSet<&PathBuf> = addresses.keys().collect();
    let mut found = Vec::new();
    for (file, address) in &addresses {
        let mut above = parent_address(address);
        while let Some(at) = above.filter(|at| !by_address.contains_key(at)) {
            above = parent_address(at);
        }
        let Some(parent) = above.map(|at| by_address[at]) else {
            continue;
        };
        let text = tree.text(parent).unwrap_or_default();
        let ok = links_by_block(parent, text, &pages)
            .paragraphs
            .contains(file);
        found.extend(held(tree, "introduced", &tree.shown(file), ok, || {
            format!(
                "{}: no paragraph of {} links to it",
                tree.shown(file),
                tree.shown(parent)
            )
        }));
    }
    found
}

/// A paragraph as a reader reads it: its links' targets left out, lower
/// case, words alone.
fn normal_paragraph(text: &str) -> String {
    let mut rest = text.to_string();
    while let Some(start) = rest.find("](") {
        let Some(end) = rest[start..].find(')') else {
            break;
        };
        rest.replace_range(start + 1..start + end + 1, "");
    }
    rest.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each paragraph long enough to say something, and the pages that say
/// it, among paragraphs on more than one page.
pub fn repeated(tree: &Tree) -> BTreeMap<String, BTreeSet<PathBuf>> {
    let least = tree.limits.of("duplicates");
    let mut said: BTreeMap<String, BTreeSet<PathBuf>> = BTreeMap::new();
    for file in addresses(tree).keys() {
        for block in blocks(tree.text(file).unwrap_or_default()) {
            if let Block::Paragraph(text) = block
                && characters(&text) >= least
            {
                said.entry(normal_paragraph(&text))
                    .or_default()
                    .insert(file.clone());
            }
        }
    }
    said.retain(|_, files| files.len() > 1);
    said
}

/// Paragraphs on two pages that share most of their wording without
/// being copies: each pair, with the share of their word runs in common.
pub fn near_copies(tree: &Tree) -> Vec<(f64, PathBuf, PathBuf)> {
    let survey = &tree.limits.survey;
    let mut runs: Vec<(PathBuf, BTreeSet<String>)> = Vec::new();
    for file in addresses(tree).keys() {
        for block in blocks(tree.text(file).unwrap_or_default()) {
            if let Block::Paragraph(text) = block
                && characters(&text) >= survey.near_length
            {
                let normal = normal_paragraph(&text);
                let words: Vec<&str> = normal.split(' ').collect();
                let set: BTreeSet<String> = words
                    .windows(survey.near_run)
                    .map(|w| w.join(" "))
                    .collect();
                if !set.is_empty() {
                    runs.push((file.clone(), set));
                }
            }
        }
    }
    let mut by_run: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, (_, set)) in runs.iter().enumerate() {
        for run in set {
            by_run.entry(run).or_default().push(i);
        }
    }
    let mut shared: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for holders in by_run.values().filter(|h| h.len() < 8) {
        for (n, a) in holders.iter().enumerate() {
            for b in &holders[n + 1..] {
                if runs[*a].0 != runs[*b].0 {
                    *shared.entry((*a, *b)).or_default() += 1;
                }
            }
        }
    }
    let mut pairs: Vec<(f64, PathBuf, PathBuf)> = shared
        .into_iter()
        .filter_map(|((a, b), n)| {
            let union = runs[a].1.union(&runs[b].1).count();
            let overlap = n as f64 / union as f64;
            (overlap >= survey.near_overlap && overlap < 1.0)
                .then(|| (overlap, runs[a].0.clone(), runs[b].0.clone()))
        })
        .collect();
    pairs.sort_by(|x, y| y.0.total_cmp(&x.0));
    pairs
}

/// Each page the sidebar nests under a page its address isn't below: the
/// page and the one the sidebar puts it under. The sidebar keeps its
/// order, which is editorial; its nesting is the address tree's.
pub fn sidebar_strays(tree: &Tree) -> Vec<(PathBuf, PathBuf)> {
    let addresses = addresses(tree);
    let mut found = Vec::new();
    for web in tree.web() {
        let file = web.app.join("docs/navigation.md");
        let dir = file.parent().expect("a folder").to_path_buf();
        let Ok(nodes) = crate::outlines::expanded(tree, &file) else {
            continue;
        };
        fn walk(
            nodes: &[crate::outlines::Node],
            above: Option<&PathBuf>,
            dir: &Path,
            addresses: &BTreeMap<PathBuf, String>,
            found: &mut Vec<(PathBuf, PathBuf)>,
        ) {
            for node in nodes {
                if node.target.contains("://") {
                    continue;
                }
                let page =
                    crate::site::normal(&dir.join(md::link_path(md::target_path(&node.target))));
                if let (Some(parent), Some(address)) = (above, addresses.get(&page))
                    && let Some(under) = addresses.get(parent)
                    && !below(address, under)
                {
                    found.push((page.clone(), parent.clone()));
                }
                let next = addresses.contains_key(&page).then_some(&page).or(above);
                walk(&node.children, next, dir, addresses, found);
            }
        }
        walk(&nodes, None, &dir, &addresses, &mut found);
    }
    found
}

/// No paragraph of a page is word for word on another page.
fn duplicates(tree: &Tree) -> Vec<Finding> {
    let repeated = repeated(tree);
    let mut found = Vec::new();
    for file in addresses(tree).keys() {
        // A generated page's entries are its declarations' help, which a
        // type composed into two kinds, or an argument two commands take,
        // gives both (`limits.toml`).
        if tree.generated_exempt("duplicates", file, tree.text(file).unwrap_or_default()) {
            continue;
        }
        let elsewhere: Vec<(&String, &BTreeSet<PathBuf>)> = repeated
            .iter()
            .filter(|(_, files)| files.contains(file))
            .collect();
        found.extend(held(
            tree,
            "duplicates",
            &tree.shown(file),
            elsewhere.is_empty(),
            || {
                let (text, files) = elsewhere[0];
                let others: Vec<String> = files
                    .iter()
                    .filter(|f| *f != file)
                    .map(|f| tree.shown(f))
                    .collect();
                format!(
                    "{}: {} is also on {}",
                    tree.shown(file),
                    named(text),
                    others.join(", ")
                )
            },
        ));
    }
    found
}

/// Each heading's level, outside code blocks and generated indexes.
fn levels(text: &str) -> Vec<usize> {
    let authored = crate::navigation::authored(text);
    let (prose, _) = md::split(&authored);
    md::headings(&prose).into_iter().map(|(l, _)| l).collect()
}

fn heading_levels(tree: &Tree, file: &Path, text: &str) -> Vec<Finding> {
    if !crate::site::is_page(tree, file) {
        return Vec::new();
    }
    let levels = levels(text);
    levels
        .windows(2)
        .filter(|pair| pair[1] > pair[0] + 1)
        .map(|pair| {
            fail(format!(
                "{}: a level-{} heading under a level-{} one",
                tree.shown(file),
                pair[1],
                pair[0]
            ))
        })
        .collect()
}

/// How deep a page's deepest heading is in the tree its address and its
/// headings make: its address's depth, then a level for each heading
/// below its title.
pub fn page_depth(address: &str, text: &str) -> usize {
    address_depth(address)
        + levels(text)
            .into_iter()
            .max()
            .unwrap_or(1)
            .saturating_sub(1)
}

fn depth(tree: &Tree) -> Vec<Finding> {
    let most = tree.limits.of("depth");
    let mut found = Vec::new();
    for (file, address) in addresses(tree) {
        // A generated page is as deep as its declarations: a kind's key
        // paths, or a module's commands in their parts (`limits.toml`).
        if tree.generated_exempt("depth", &file, tree.text(&file).unwrap_or_default()) {
            continue;
        }
        let deepest = page_depth(&address, tree.text(&file).unwrap_or_default());
        found.extend(held(
            tree,
            "depth",
            &tree.shown(&file),
            deepest <= most,
            || {
                format!(
                    "{}: {deepest} levels deep, its address and its headings, past {most}",
                    tree.shown(&file)
                )
            },
        ));
    }
    found
}

/// Each page of the websites with a hook, and its hook.
pub fn hooks(tree: &Tree) -> Vec<(PathBuf, String)> {
    addresses(tree)
        .into_keys()
        .filter_map(|file| {
            let blocks = blocks(tree.text(&file).unwrap_or_default());
            let hook = crate::pages::hook(&blocks)?.to_string();
            Some((file, hook))
        })
        .collect()
}

/// A hook has at least `hook-words` words: more than its keyword.
fn hook_words(tree: &Tree) -> Vec<Finding> {
    let least = tree.limits.of("hook-words");
    let mut found = Vec::new();
    for (file, hook) in hooks(tree) {
        let words = hook.split_whitespace().count();
        found.extend(held(
            tree,
            "hook-words",
            &tree.shown(&file),
            words >= least,
            || {
                format!(
                    "{}: \"{hook}\" is {words} words, fewer than {least}",
                    tree.shown(&file)
                )
            },
        ));
    }
    found
}

/// A hook is in Title Case, as the website shows it.
fn hook_case(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for (file, hook) in hooks(tree) {
        let wrong = crate::site::untitled(&hook);
        found.extend(held(
            tree,
            "hook-case",
            &tree.shown(&file),
            wrong.is_empty(),
            || {
                format!(
                    "{}: \"{hook}\" isn't Title Case: {}",
                    tree.shown(&file),
                    wrong.join(", ")
                )
            },
        ));
    }
    found
}

/// The address all of a package's pages are under, by whole segments:
/// `/` when they share none.
pub fn common_address<'a>(addresses: impl IntoIterator<Item = &'a str>) -> String {
    let mut common: Option<Vec<&str>> = None;
    for address in addresses {
        let parts: Vec<&str> = address.split('/').filter(|s| !s.is_empty()).collect();
        common = Some(match common {
            None => parts,
            Some(before) => before
                .into_iter()
                .zip(parts)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    format!("/{}", common.unwrap_or_default().join("/"))
}

/// Whether `address` is `under` or below it, by whole segments.
fn below(address: &str, under: &str) -> bool {
    under == "/" || address == under || address.starts_with(&format!("{under}/"))
}

/// Each package with pages, and its pages' addresses.
pub fn package_addresses(tree: &Tree) -> BTreeMap<PathBuf, Vec<String>> {
    let packages = packages(tree);
    let mut out: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for (file, address) in addresses(tree) {
        if let Some(package) = packages
            .iter()
            .filter(|p| file.starts_with(p.join("docs")))
            .max_by_key(|p| p.as_os_str().len())
        {
            out.entry(package.clone()).or_default().push(address);
        }
    }
    out
}

/// A package's pages are under the address its nearest enclosing
/// package's pages share.
fn follows_code(tree: &Tree) -> Vec<Finding> {
    let pages = package_addresses(tree);
    let projections = projections::homes(tree);
    let mut found = Vec::new();
    for (package, addresses) in &pages {
        let Some((parent, above)) = pages
            .iter()
            .filter(|(p, _)| *p != package && package.starts_with(p))
            .max_by_key(|(p, _)| p.as_os_str().len())
        else {
            continue;
        };
        let under = common_address(above.iter().map(String::as_str));
        let homes = projections.get(package);
        let astray: Vec<&String> = addresses
            .iter()
            .filter(|a| {
                !below(a, &under)
                    && !homes.is_some_and(|homes| homes.iter().any(|home| below(a, home)))
            })
            .collect();
        found.extend(held(
            tree,
            "follows-code",
            &tree.shown(package),
            astray.is_empty(),
            || {
                format!(
                    "{}: {} not under {under}, where {}'s pages are",
                    tree.shown(package),
                    astray
                        .iter()
                        .map(|a| a.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    tree.shown(parent)
                )
            },
        ));
    }
    found
}

/// A rule's finding for `item`, given whether it keeps the rule: none
/// when it does and isn't held, a failure when it doesn't and isn't held,
/// and a failure when it's held but keeps the rule now, so the list only
/// shrinks.
pub fn held(
    tree: &Tree,
    rule: &str,
    item: &str,
    ok: bool,
    problem: impl FnOnce() -> String,
) -> Option<Finding> {
    match (ok, tree.large.holds(rule, item)) {
        (true, false) | (false, true) => None,
        (false, false) => Some(fail(problem())),
        (true, true) => Some(fail(format!(
            "{item}: keeps {rule} now; take it out of large-files.toml's [held]"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_has_a_parent_and_a_depth() {
        assert_eq!(parent_address("/agents/link"), Some("/agents"));
        assert_eq!(parent_address("/agents"), Some("/"));
        assert_eq!(parent_address("/"), None);
        assert_eq!(address_depth("/"), 0);
        assert_eq!(address_depth("/agents"), 1);
        assert_eq!(address_depth("/agents/link/guide"), 3);
    }

    #[test]
    fn a_page_is_as_deep_as_its_address_and_its_headings() {
        let text = "# Title\n\n## Part\n\n### Detail\n\n```sh\n#### not a heading\n```\n";
        assert_eq!(page_depth("/agents/link", text), 4);
        assert_eq!(page_depth("/", "# Home\n"), 0);
    }

    #[test]
    fn packages_share_the_address_their_pages_share() {
        assert_eq!(
            common_address(["/agents/tools/run", "/agents/tools/run/guide"]),
            "/agents/tools/run"
        );
        assert_eq!(common_address(["/agents/a", "/config/b"]), "/");
        assert!(below("/agents/tools/run", "/agents/tools"));
        assert!(!below("/agents/toolsmith", "/agents/tools"));
    }

    #[test]
    fn a_paragraph_is_compared_as_it_reads() {
        assert_eq!(
            normal_paragraph("Run [this](a/b.md), then **that**."),
            normal_paragraph("run [This](c.md) then that")
        );
    }

    #[test]
    fn a_ratio_is_off_by_the_same_either_way() {
        let measure = |rust, docs| Measure {
            package: PathBuf::new(),
            rust,
            docs,
            pages: 1,
            ..Measure::default()
        };
        assert!((measure(10_000, 100).off_by(0.5) - 1.0).abs() < 1e-9);
        assert!((measure(10_000, 400).off_by(0.5) - 4.0).abs() < 1e-9);
        assert!((measure(10_000, 25).off_by(0.5) - 4.0).abs() < 1e-9);
        assert!((measure(10_000, 0).off_by(0.5) - 100.0).abs() < 1e-9);
    }
}
