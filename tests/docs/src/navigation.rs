//! One reading tree, discovered once for its writer and checks. Each
//! document belongs to its nearest existing README ancestor; physical
//! bridge folders need no synthetic page or website route.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::md::{link_path, links, split};
use crate::site::normal;
use crate::{Check, Finding, Rule, Scope, Tree, fail};

pub const START: &str = "<!-- docs-tree:start -->";
pub const END: &str = "<!-- docs-tree:end -->";
/// The kinds of folder a README of code is, as the architecture guide's
/// Kinds defines them.
const KINDS: [&str; 6] = [
    "Module",
    "Package",
    "Core",
    "Layer",
    "Group",
    "Group Package",
];

/// The subsections of a README's index, in order: what the folder
/// documents, its pages (`kinds.toml`'s Pages and References), then what's
/// in it, from what a reader runs to the folders holding more, and last
/// More, its developer pages and catch-alls. A reader sees these words,
/// so none names a kind only the architecture uses, such as a layer.
const SECTIONS: [&str; 7] = [
    "Pages",
    "References",
    "Core",
    "Modules",
    "Packages",
    "Folders",
    "More",
];

/// Where `name` is among [`SECTIONS`].
fn place(name: &str) -> usize {
    SECTIONS
        .iter()
        .position(|section| *section == name)
        .unwrap_or_else(|| panic!("{name} is no index heading"))
}

/// Whether a path is under a `docs/` folder of the workspace: a page,
/// never a folder's hub.
fn in_docs(tree: &Tree, path: &Path) -> bool {
    path.strip_prefix(&tree.root)
        .is_ok_and(|rel| rel.components().any(|c| c.as_os_str() == "docs"))
}

pub const RULES: &[Rule] = &[
    Rule {
        id: "navigation",
        scope: Scope::Every,
        fix: "run cargo run -p link-docs-check --bin lnk-docs-tree -- --root <workspace> --write; \
              site outlines include every owned page beneath its nearest address ancestor, \
              each page's children in order of their kind (kinds.toml); README indexes \
              follow that outline and their heading names the hub's kind",
        check: Check::Whole(check),
    },
    Rule {
        id: "doc-reachability",
        scope: Scope::Every,
        fix: "every document is reachable from the root README through its nearest README \
              ancestors; keep a root README and run the lnk-docs-tree writer",
        check: Check::Whole(reachability),
    },
    Rule {
        id: "descriptions",
        scope: Scope::Every,
        fix: "a package's Cargo.toml description is its README's first sentence, the one \
              every index lists it by and `lnk module list` shows: change one to match the other",
        check: Check::Whole(descriptions),
    },
];

/// A generated index preserves prose outside its marked block.
#[derive(Clone, Debug)]
pub struct Update {
    pub path: PathBuf,
    pub text: String,
}

/// The selected documents, and each hub's immediate reading children.
/// BTree collections keep output stable across filesystems and runs.
pub struct Navigation<'a> {
    tree: &'a Tree,
    documents: BTreeSet<PathBuf>,
    children: BTreeMap<PathBuf, Vec<PathBuf>>,
    outlines: Result<crate::outlines::Outlines, String>,
    /// Each crate's folder and its kind.
    crates: BTreeMap<PathBuf, &'static str>,
}

impl<'a> Navigation<'a> {
    pub fn new(tree: &'a Tree) -> Self {
        let packages = crate::shape::packages(tree);
        let documents: BTreeSet<_> = tree
            .files
            .keys()
            .filter(|p| selected(tree, &packages, p))
            .cloned()
            .collect();
        let mut children: BTreeMap<PathBuf, Vec<PathBuf>> = documents
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n == "README.md"))
            .map(|p| (p.clone(), Vec::new()))
            .collect();
        for path in &documents {
            if let Some(owner) = owner(path, &children) {
                children.get_mut(&owner).unwrap().push(path.clone());
            }
        }
        Self {
            tree,
            documents,
            children,
            outlines: crate::outlines::Outlines::new(tree),
            crates: crate::layers::packages(tree)
                .into_values()
                .map(
                    |p| match (p.modules.iter().any(|id| id != "lnk"), p.modules.len()) {
                        (true, _) => (p.dir, "Module"),
                        (false, 0) => (p.dir, "Package"),
                        (false, _) => (p.dir, "Core"),
                    },
                )
                .collect(),
        }
    }

    /// Render every index through the same path used by the check.
    pub fn updates(&self) -> Result<Vec<Update>, String> {
        if !self.documents.is_empty() && !self.documents.contains(&self.tree.root.join("README.md"))
        {
            return Err(format!(
                "{}: root README.md is missing",
                self.tree.root.display()
            ));
        }
        let outlines = self.outlines.as_ref().map_err(Clone::clone)?;
        let mut updates = outlines.updates.clone();
        for (hub, children) in &self.children {
            let old = self.tree.text(hub).unwrap();
            // A hub with nothing under it lists nothing: no empty `## Docs`.
            let text = match children.is_empty() {
                // Validated as a replacement would be, then taken out.
                true => replace_index(old, "", hub, children).map(|_| authored(old))?,
                false => replace_index(old, &self.block(hub, children), hub, children)?,
            };
            let text = format!("{}\n", text.trim_end());
            if text != old {
                updates.push(Update {
                    path: hub.clone(),
                    text,
                });
            }
        }
        Ok(updates)
    }

    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    pub fn documents(&self) -> impl Iterator<Item = &PathBuf> {
        self.documents.iter()
    }

    /// Follow actual Markdown links: a correctly discovered tree alone
    /// must not count an unlinked page as reachable.
    pub fn unreachable(&self) -> Vec<PathBuf> {
        let reached = self.clicks();
        self.documents
            .iter()
            .filter(|path| !reached.contains_key(*path))
            .cloned()
            .collect()
    }

    /// Each document a reader reaches from the root README, by the fewest
    /// links they follow to it: the root README's is 0.
    pub fn clicks(&self) -> BTreeMap<PathBuf, usize> {
        let root = self.tree.root.join("README.md");
        let mut found = BTreeMap::new();
        if !self.documents.contains(&root) {
            return found;
        }
        let mut pending = std::collections::VecDeque::from([(root, 0)]);
        while let Some((path, clicks)) = pending.pop_front() {
            if found.contains_key(&path) {
                continue;
            }
            found.insert(path.clone(), clicks);
            for to in self.targets(&path) {
                if !found.contains_key(&to) {
                    pending.push_back((to, clicks + 1));
                }
            }
        }
        found
    }

    /// The documents a document links to, a folder's by its README.
    fn targets(&self, path: &Path) -> Vec<PathBuf> {
        let Some(text) = self.tree.text(path) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for target in links(&split(text).0) {
            if target.contains("://") || target.starts_with('#') {
                continue;
            }
            let target = target.split('#').next().unwrap_or_default();
            let mut to = normal(&path.parent().unwrap().join(link_path(target)));
            if self.tree.dirs.contains(&to) {
                to.push("README.md");
            }
            if self.documents.contains(&to) {
                found.push(to);
            }
        }
        found
    }

    fn block(&self, hub: &Path, children: &[PathBuf]) -> String {
        let authored_heading = self
            .tree
            .text(hub)
            .and_then(|source| source.split_once(START))
            .and_then(|(before, _)| {
                split(before)
                    .0
                    .into_iter()
                    .rev()
                    .find(|line| line.starts_with('#'))
            })
            .is_some_and(|heading| heading.trim_end() == "## Docs");
        let mut text = format!("{START}\n\n");
        if !authored_heading {
            text += "## Docs\n\n";
        }
        // A README of code sorts what's under it by kind, and opens its
        // own docs one level, so its index reads as the folder's tree.
        let code = self.kind(hub).is_some();
        let mut sections: BTreeMap<usize, Vec<&PathBuf>> = BTreeMap::new();
        for child in children {
            let section = if code { self.section(child) } else { 0 };
            sections.entry(section).or_default().push(child);
        }
        // Each section's lines, each an entry and its depth.
        let mut lines: BTreeMap<usize, Vec<(&PathBuf, usize)>> = BTreeMap::new();
        for (section, entries) in sections {
            let mut flat = Vec::new();
            for (path, depth) in self.nested(&entries) {
                flat.push((path, depth));
                let opened = match (code, self.children.get(path)) {
                    (true, Some(pages)) if in_docs(self.tree, path) => pages,
                    _ => continue,
                };
                for (page, under) in self.nested(&opened.iter().collect::<Vec<_>>()) {
                    flat.push((page, depth + 1 + under));
                }
            }
            if section > 0 {
                lines.entry(section).or_default().extend(flat);
                continue;
            }
            // Pages go under their kind's index, each with the pages under
            // it, a level up for each listed page above it left behind.
            let mut open: Vec<(usize, usize, usize)> = Vec::new();
            for (path, depth) in flat {
                while open.last().is_some_and(|(at, _, _)| *at >= depth) {
                    open.pop();
                }
                let (index, offset) = match open.last() {
                    Some(&(_, index, offset)) if index > 0 => (index, offset),
                    _ => {
                        let index = self.index(path);
                        (index, if index > 0 { depth } else { 0 })
                    }
                };
                open.push((depth, index, offset));
                lines.entry(index).or_default().push((path, depth - offset));
            }
        }
        let headed = lines.len() > 1;
        for (section, entries) in lines {
            if headed {
                text += &format!("### {}\n\n", SECTIONS[section]);
            }
            for (path, depth) in entries {
                text += &self.entry(hub, path, &"  ".repeat(depth));
            }
            text += "\n";
        }
        text += END;
        text
    }

    /// Entries as the website reads them: its pages in its outline's
    /// order, each under the nearest page above it there that's also
    /// listed, so an index nests only where the website does. Entries
    /// the website doesn't hold follow, alphabetically by title, then by
    /// path. Each comes with how many listed pages it's under.
    fn nested<'p>(&self, paths: &[&'p PathBuf]) -> Vec<(&'p PathBuf, usize)> {
        let order = self.outlines.as_ref().ok().map(|o| &o.order);
        let place = |path: &PathBuf| order.and_then(|o| o.get(path));
        let mut entries: Vec<_> = paths
            .iter()
            .map(|p| {
                (
                    place(p).map(|(at, _)| *at),
                    self.summary(p).0.to_lowercase(),
                    *p,
                )
            })
            .collect();
        entries
            .sort_by(|a, b| (a.0.is_none(), a.0, &a.1, a.2).cmp(&(b.0.is_none(), b.0, &b.1, b.2)));
        let listed: BTreeSet<&PathBuf> = paths.iter().copied().collect();
        entries
            .into_iter()
            .map(|(_, _, path)| {
                let mut depth = 0;
                let mut above = place(path).and_then(|(_, parent)| parent.as_ref());
                while let Some(parent) = above {
                    if listed.contains(parent) {
                        depth += 1;
                    }
                    above = place(parent).and_then(|(_, parent)| parent.as_ref());
                }
                (path, depth)
            })
            .collect()
    }

    /// One index line: the child's title, linked from `hub`, and its
    /// description.
    fn entry(&self, hub: &Path, path: &Path, indent: &str) -> String {
        let (title, description) = self.summary(path);
        let target = path.strip_prefix(hub.parent().unwrap()).unwrap();
        let target = encode_path(&target.to_string_lossy().replace('\\', "/"));
        match description.is_empty() {
            true => format!("{indent}- [{title}]({target})\n"),
            false => format!("{indent}- [{title}]({target}): {description}\n"),
        }
    }

    /// Which of [`SECTIONS`] a page is listed under, by its kind's index
    /// (`kinds.toml`).
    fn index(&self, page: &Path) -> usize {
        place(
            &crate::kinds::of(page, self.tree.text(page).unwrap_or_default())
                .1
                .index,
        )
    }

    /// Which of [`SECTIONS`] a child of a README of code is listed under:
    /// 0 for a page, else its folder's kind.
    fn section(&self, child: &Path) -> usize {
        if in_docs(self.tree, child) || !child.ends_with("README.md") {
            return 0;
        }
        place(match self.kind(child) {
            Some("Layer" | "Group") | None => "Folders",
            Some("Module") => "Modules",
            Some("Core") => "Core",
            Some(_) => "Packages",
        })
    }

    /// Which of [`KINDS`] a README's folder is: a crate's, a layer or a
    /// group of crates; none for pages.
    fn kind(&self, readme: &Path) -> Option<&'static str> {
        let dir = readme.parent().filter(|dir| *dir != self.tree.root)?;
        if !readme.ends_with("README.md") {
            None
        } else if let Some(kind) = self.crates.get(dir) {
            // A package whose folder holds other crates groups them too.
            let holds = self.crates.keys().any(|c| c != dir && c.starts_with(dir));
            Some(if *kind == KINDS[1] && holds {
                KINDS[5]
            } else {
                kind
            })
        } else if self.tree.files.contains_key(&dir.join("layer.toml")) {
            Some(KINDS[3])
        } else {
            self.crates
                .keys()
                .any(|c| c.starts_with(dir))
                .then_some(KINDS[4])
        }
    }

    fn summary(&self, path: &Path) -> (String, String) {
        let source = self.tree.text(path).unwrap();
        let lines = split(source).0;
        let title = lines
            .iter()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or_else(|| path.file_stem().unwrap().to_str().unwrap_or("Document"));
        let own = opening(&lines);
        let package = path.parent().unwrap().join("Cargo.toml");
        let description = if path.file_name().is_some_and(|n| n == "README.md") {
            self.tree
                .text(&package)
                .and_then(|text| {
                    toml::from_str::<toml::Table>(text)
                        .ok()?
                        .get("package")?
                        .get("description")?
                        .as_str()
                        .map(str::to_owned)
                })
                .unwrap_or(own)
        } else {
            own
        };
        let description = summary_description(&description);
        (shorten(&plain_title(title), 100), description)
    }
}

/// A page's opening paragraph: the lines after its title, past a hook
/// quoted under it, up to a blank line, a heading or a comment.
fn opening(lines: &[&str]) -> String {
    let mut paragraph = Vec::new();
    let mut after_title = false;
    for line in lines {
        if line.starts_with("# ") {
            after_title = true;
            continue;
        }
        if !after_title
            || (paragraph.is_empty() && (line.trim().is_empty() || line.starts_with('>')))
        {
            continue;
        }
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with("<!--") {
            break;
        }
        paragraph.push(line.trim());
    }
    paragraph.join(" ")
}

/// Each package whose description isn't its README's first sentence, as
/// the indexes summarize it, but for the package's name opening it
/// (`` `link-files`: the files... ``) and the case of its first letter.
fn descriptions(tree: &Tree) -> Vec<Finding> {
    let mut found = Vec::new();
    for (path, text) in &tree.files {
        if !path.ends_with("Cargo.toml") || !path.starts_with(&tree.root) {
            continue;
        }
        let Some(package) = toml::from_str::<toml::Table>(text)
            .ok()
            .and_then(|t| t.get("package").and_then(|p| p.as_table()).cloned())
        else {
            continue;
        };
        let readme = path.with_file_name("README.md");
        let (Some(name), Some(source)) = (
            package.get("name").and_then(|n| n.as_str()),
            tree.text(&readme),
        ) else {
            continue;
        };
        let described = package
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or_default();
        let sentence = first_sentence(name, &opening(&split(source).0));
        if !same_sentence(&summary_description(described), &sentence) {
            found.push(fail(format!(
                "{}: its description isn't its README's first sentence, \"{sentence}\"",
                tree.shown(path)
            )));
        }
    }
    found
}

/// A README's first sentence, as its package's description says it: the
/// name opening it, as in `` `link-files`: ``, left out.
fn first_sentence(name: &str, paragraph: &str) -> String {
    let sentence = summary_description(paragraph);
    sentence
        .strip_prefix(&format!("`{name}`: "))
        .unwrap_or(&sentence)
        .to_owned()
}

/// Whether two sentences are the same but for their first letter's case.
fn same_sentence(a: &str, b: &str) -> bool {
    let lower = |s: &str| {
        let mut chars = s.chars();
        chars.next().map_or(String::new(), |c| {
            c.to_lowercase().chain(chars).collect::<String>()
        })
    };
    lower(a) == lower(b)
}

fn summary_description(text: &str) -> String {
    let plain = plain(text);
    let sentence = plain
        .split_once(". ")
        .map_or(plain.as_str(), |(first, _)| first);
    shorten(sentence.trim_end_matches('.').trim_end(), 240)
}

fn owner(path: &Path, hubs: &BTreeMap<PathBuf, Vec<PathBuf>>) -> Option<PathBuf> {
    let mut at = path.parent();
    while let Some(dir) = at {
        let candidate = dir.join("README.md");
        if candidate != path && hubs.contains_key(&candidate) {
            return Some(candidate);
        }
        at = dir.parent();
    }
    None
}

pub(crate) fn selected(tree: &Tree, packages: &[PathBuf], path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(&tree.root) else {
        return false;
    };
    if path.extension().is_none_or(|e| e != "md")
        || tree.is_data(path)
        || crate::outlines::metadata(tree, path)
    {
        return false;
    }
    let parts: Vec<_> = relative.iter().filter_map(|p| p.to_str()).collect();
    let fixture_hub = relative.starts_with("tests/fixtures")
        && path.file_name().is_some_and(|name| name == "README.md");
    if parts.iter().any(|p| {
        p.starts_with('.') || matches!(*p, "assets" | "src") || (*p == "fixtures" && !fixture_hub)
    }) || parts
        .last()
        .is_some_and(|p| matches!(*p, "AGENTS.md" | "CLAUDE.md" | "SKILL.md" | "website.md"))
    {
        return false;
    }
    if parts.last().is_some_and(|p| *p == concat!("HUMAN", ".md")) {
        return false;
    }
    if translation(path).is_some() {
        return false;
    }
    if packages.iter().any(|package| {
        ["tests", "benches", "examples"]
            .iter()
            .any(|part| path.starts_with(package.join(part)))
    }) {
        return false;
    }
    // Package Markdown outside its documentation tree is implementation
    // data (for example embedded prompts), except README navigation hubs.
    if path.file_name().is_none_or(|name| name != "README.md")
        && let Some(package) = packages
            .iter()
            .filter(|package| path.starts_with(package))
            .max_by_key(|package| package.components().count())
        && !path.starts_with(package.join("docs"))
    {
        return false;
    }
    true
}

/// Whether a file is in an owner's `docs/`: the first folder named `docs`
/// on its path, below the workspace's root or in a Cargo workspace's own.
/// A plan's `docs/` beneath a repository's `docs/` is no owner's.
pub fn owned_docs(tree: &Tree, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(&tree.root) else {
        return false;
    };
    let mut parts = relative.parent().into_iter().flat_map(Path::iter);
    match parts.position(|part| part == "docs") {
        Some(0) => tree.files.contains_key(&tree.root.join("Cargo.toml")),
        Some(_) => true,
        None => false,
    }
}

/// A translation, `<page>.<language>.md` beside its English `<page>.md`:
/// the English page's path and the language, one of
/// [`link_component::languages`].
pub fn translation(path: &Path) -> Option<(PathBuf, &str)> {
    let (english, language) = link_component::languages::translation(path.file_name()?.to_str()?)?;
    Some((path.with_file_name(english), language))
}

fn plain_title(text: &str) -> String {
    plain(text).replace(['[', ']', '`'], "")
}

fn plain(text: &str) -> String {
    text.split('`')
        .enumerate()
        .map(|(index, part)| {
            if index % 2 == 1 {
                part.to_owned()
            } else {
                plain_prose(part)
            }
        })
        .collect::<Vec<_>>()
        .join("`")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn plain_prose(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find('[') {
        let Some(mid) = out[start..].find("](") else {
            break;
        };
        let mid = start + mid;
        let Some(end) = out[mid + 2..].find(')') else {
            break;
        };
        let label = out[start + 1..mid].to_string();
        out.replace_range(start..mid + 3 + end, &label);
    }
    out.replace('*', "")
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let shortened: String = text.chars().take(max - 1).collect();
    let shortened = shortened
        .rsplit_once(' ')
        .map_or(shortened.as_str(), |(s, _)| s);
    let complete = if shortened.matches('`').count() % 2 == 1 {
        &shortened[..shortened.rfind('`').unwrap()]
    } else {
        shortened
    };
    format!("{}…", complete.trim_end())
}

fn encode_path(path: &str) -> String {
    path.replace('%', "%25")
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('#', "%23")
        .replace('?', "%3F")
}

fn replace_index(
    old: &str,
    block: &str,
    hub: &Path,
    children: &[PathBuf],
) -> Result<String, String> {
    let starts: Vec<_> = markers(old, START);
    let ends: Vec<_> = markers(old, END);
    match (starts.as_slice(), ends.as_slice()) {
        ([start], [end]) if end > start => {
            return Ok(format!(
                "{}{}{}",
                &old[..*start],
                block,
                &old[end + END.len()..]
            ));
        }
        ([], []) => {}
        _ => {
            return Err(format!(
                "{}: malformed or repeated docs-tree markers",
                hub.display()
            ));
        }
    }
    let lines: Vec<_> = old.split_inclusive('\n').collect();
    let docs = lines.iter().position(|l| l.trim_end() == "## Docs");
    if let Some(start) = docs {
        let end = (start + 1..lines.len())
            .find(|i| lines[*i].starts_with("## "))
            .unwrap_or(lines.len());
        let mut kept = String::new();
        let mut at = start + 1;
        while at < end {
            if !lines[at].starts_with("- [") {
                kept.push_str(lines[at]);
                at += 1;
                continue;
            }
            let next = (at + 1..end)
                .find(|i| lines[*i].starts_with("- ") || lines[*i].trim().is_empty())
                .unwrap_or(end);
            let item = lines[at..next].concat().replace('\n', " ");
            let targets = links(&[item.as_str()])
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let generated_link = !targets.is_empty()
                && targets.iter().all(|target| {
                    let target = target.split('#').next().unwrap_or_default();
                    let to = normal(&hub.parent().unwrap().join(link_path(target)));
                    children.contains(&to) || children.contains(&to.join("README.md"))
                });
            if !generated_link {
                kept.push_str(&lines[at..next].concat());
            }
            at = next;
        }
        return Ok(format!(
            "{}{block}\n\n{}{}",
            lines[..start].concat(),
            kept.trim_start_matches('\n'),
            lines[end..].concat()
        ));
    }
    let insert = lines
        .iter()
        .position(|l| l.trim_end() == "## Code")
        .unwrap_or(lines.len());
    Ok(format!(
        "{}\n\n{block}\n\n{}",
        lines[..insert].concat().trim_end(),
        lines[insert..].concat()
    ))
}

/// Whether `line` is a README's `## Docs` heading, or one of its index's
/// sections.
pub(crate) fn is_docs_heading(line: &str) -> bool {
    line.trim_end() == "## Docs"
        || line
            .trim_end()
            .strip_prefix("### ")
            .is_some_and(|heading| SECTIONS.contains(&heading))
}

pub fn check(tree: &Tree) -> Vec<Finding> {
    match Navigation::new(tree).updates() {
        Ok(updates) => updates
            .into_iter()
            .map(|update| {
                fail(format!(
                    "{}: reading-tree navigation is missing or stale",
                    tree.shown(&update.path)
                ))
            })
            .collect(),
        Err(error) => vec![fail(error)],
    }
}

pub fn reachability(tree: &Tree) -> Vec<Finding> {
    Navigation::new(tree)
        .unreachable()
        .iter()
        .map(|path| {
            fail(format!(
                "{}: no reading path from README.md",
                tree.shown(path)
            ))
        })
        .collect()
}

/// Authored text for size/heading budgets. Only exactly one complete
/// generated block is omitted; malformed markers cannot hide content.
/// Other prose and all rendered links remain subject to their checks.
pub fn authored(text: &str) -> String {
    let starts: Vec<_> = markers(text, START);
    let ends: Vec<_> = markers(text, END);
    match (starts.as_slice(), ends.as_slice()) {
        ([start], [end]) if end > start => {
            let before = text[..*start].trim_end_matches('\n');
            let after = text[end + END.len()..].trim_start_matches('\n');
            if after.is_empty() {
                format!("{before}\n")
            } else {
                format!("{before}\n\n{after}")
            }
        }
        _ => text.to_owned(),
    }
}

/// Authored explanations, excluding indexes and generated inventories of names.
pub fn explanations(text: &str) -> String {
    let mut text = authored(text);
    while let Some(start) = text.find("<!-- inventory:") {
        let Some(name_end) = text[start..].find(":start -->") else {
            break;
        };
        let name = &text[start + "<!-- inventory:".len()..start + name_end];
        let end_marker = format!("<!-- inventory:{name}:end -->");
        let Some(end) = text[start..].find(&end_marker) else {
            break;
        };
        text.replace_range(start..start + end + end_marker.len(), "");
    }
    text
}

fn markers(text: &str, marker: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut offset = 0;
    let mut fence = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let opening = if trimmed.starts_with("```") {
            Some('`')
        } else if trimmed.starts_with("~~~") {
            Some('~')
        } else {
            None
        };
        if let Some(kind) = opening {
            if fence == Some(kind) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(kind);
            }
        } else if fence.is_none() && line.trim_end() == marker {
            found.push(offset);
        }
        offset += line.len();
    }
    found
}

#[cfg(test)]
mod summary_tests {
    #[test]
    fn ellipses_do_not_leave_trailing_whitespace_in_generated_indexes() {
        assert_eq!(
            super::summary_description("Mount, sync, serve, ..."),
            "Mount, sync, serve,"
        );
        assert_eq!(
            super::summary_description("First sentence. Next sentence."),
            "First sentence"
        );
        assert_eq!(super::summary_description("Use `a...`."), "Use `a...`");
    }

    #[test]
    fn a_description_is_its_readmes_first_sentence_but_for_its_name() {
        let first = super::first_sentence(
            "link-files",
            "`link-files`: the [files](docs/files.md) Link keeps. More.",
        );
        assert_eq!(first, "the files Link keeps");
        assert!(super::same_sentence("The files Link keeps", &first));
        assert!(!super::same_sentence("Files Link keeps", &first));
        let own = super::first_sentence("link-core", "`link-core` holds modules.");
        assert_eq!(own, "`link-core` holds modules");
    }
}

#[cfg(test)]
mod index_tests {
    #[test]
    fn every_kinds_index_is_a_heading_in_reading_order() {
        let at: Vec<usize> = crate::kinds::INDEXES
            .iter()
            .map(|i| super::place(i))
            .collect();
        assert!(at[0] < super::place("Modules") && super::place("Packages") < at[2]);
        assert!(super::place("Modules") < super::place("Folders"));
    }
}
