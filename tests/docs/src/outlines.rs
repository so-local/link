//! Canonical curated site outlines: discover every owned English page,
//! retain curator order with address-derived nesting, and rank README reading children by
//! the same completed outline that the writer persists.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::md::link_path;
use crate::navigation::Update;
use crate::site::normal;
use crate::{Tree, Website};

mod nesting;
mod parts;

pub use parts::{expanded, expanded_text};

pub(crate) fn sites(
    root: &Path,
    files: &BTreeMap<PathBuf, String>,
) -> Result<(Vec<PathBuf>, Vec<Website>), String> {
    let mut folders = BTreeSet::new();
    let mut websites = Vec::new();
    for (path, text) in files {
        if !path.ends_with("scripts/documentation-index.json") {
            continue;
        }
        let base = path.parent().unwrap().parent().unwrap();
        if base != root && base.parent() != Some(root) {
            continue;
        }
        let index: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let at = |entry: &serde_json::Value| -> Result<PathBuf, String> {
            let relative = entry["directory"]
                .as_str()
                .ok_or("documentation directory missing")?;
            let path = normal(&base.join(relative));
            if !path.starts_with(base) {
                return Err("documentation directory escapes workspace".into());
            }
            Ok(path)
        };
        let owners: Vec<_> = index["roots"]
            .as_array()
            .ok_or("documentation roots missing")?
            .iter()
            .map(at)
            .collect::<Result<_, _>>()?;
        let entries = index["sites"].as_array().cloned().unwrap_or_default();
        let apps: Vec<_> = entries.iter().map(at).collect::<Result<_, _>>()?;
        folders.extend(owners.iter().cloned());
        folders.extend(apps.iter().cloned());
        for (entry, app) in entries.iter().zip(&apps) {
            let homepage = entry
                .get("homepage")
                .map(|source| {
                    let source = source
                        .as_str()
                        .ok_or("homepage must name a Markdown file")?;
                    let relative = Path::new(source);
                    if relative.is_absolute()
                        || relative
                            .components()
                            .any(|part| part == std::path::Component::ParentDir)
                    {
                        return Err("homepage escapes the repository");
                    }
                    let path = base.join(relative);
                    if path.extension().is_none_or(|ext| ext != "md") || !files.contains_key(&path)
                    {
                        return Err("homepage must name an existing Markdown file");
                    }
                    Ok(path)
                })
                .transpose()?;
            // Pages another workspace owns, shown on this site at an
            // address of its own (`mission.md` is `<url>/mission`).
            let shared_pages = entry
                .get("sharedPages")
                .and_then(|pages| pages.as_object())
                .into_iter()
                .flatten()
                .map(|(name, source)| {
                    let source = source
                        .as_str()
                        .ok_or("a shared page names a Markdown file")?;
                    let address = name
                        .strip_suffix(".md")
                        .ok_or("a shared page's name is a Markdown file's")?;
                    Ok((address.to_owned(), normal(&base.join(source))))
                })
                .collect::<Result<Vec<_>, &str>>()?;
            websites.push(Website {
                shared_pages,
                workspace: base.to_path_buf(),
                shared: entry["roots"].as_bool() == Some(true),
                name: entry["name"].as_str().unwrap_or("Website").to_owned(),
                homepage,
                url: entry["url"]
                    .as_str()
                    .ok_or("documentation site URL missing")?
                    .trim_end_matches('/')
                    .to_owned(),
                app: app.clone(),
                folders: std::iter::once(app.clone())
                    .chain(
                        owners
                            .iter()
                            .filter(|owner| {
                                entry["roots"].as_bool() == Some(true) && !apps.contains(owner)
                            })
                            .cloned(),
                    )
                    .collect(),
            });
        }
    }
    Ok((folders.into_iter().collect(), websites))
}

/// Site outlines and their navigation/ section files are structural metadata.
/// A contributor guide named navigation.md deeper in docs remains a page.
pub(crate) fn metadata(tree: &Tree, path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == "website.md") {
        return true;
    }
    tree.websites.iter().any(|web| {
        path.strip_prefix(web.app.join("docs")).is_ok_and(|rel| {
            let components: Vec<_> = rel.iter().collect();
            let name = rel.to_string_lossy();
            name == "navigation.md"
                || crate::navigation::translation(path)
                    .is_some_and(|(english, _)| english == web.app.join("docs/navigation.md"))
                || (components.len() == 2
                    && components[0] == "navigation"
                    && path.extension().is_some_and(|ext| ext == "md"))
        })
    })
}

#[derive(Clone, Debug)]
pub struct Node {
    pub label: String,
    pub target: String,
    pub children: Vec<Node>,
    pub file: Option<PathBuf>,
    pub address: Option<String>,
}

/// Parse just the structural page list, shared by validation and section
/// checks. Links are resolved later against the index-backed page model.
pub fn nodes(text: &str) -> Result<Vec<Node>, String> {
    let mut flat = Vec::new();
    let mut pages = false;
    for (number, line) in text.lines().enumerate() {
        if line == "## Pages" {
            pages = true;
            continue;
        }
        if line.starts_with("## ") && pages {
            break;
        }
        if !pages || line.trim().is_empty() {
            continue;
        }
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let Some(item) = trimmed.strip_prefix("- [") else {
            return Err(format!("line {} is not a page entry", number + 1));
        };
        if indent % 2 != 0 {
            return Err(format!("line {} needs two-space nesting", number + 1));
        }
        let (label, rest) = item.split_once("](").ok_or("page entry has no target")?;
        let target = rest
            .strip_suffix(')')
            .ok_or("page entry has trailing prose or no closing parenthesis")?;
        flat.push((
            indent / 2,
            Node {
                label: label.to_owned(),
                target: target.to_owned(),
                children: Vec::new(),
                file: None,
                address: None,
            },
        ));
    }
    if !pages {
        return Err("navigation has no ## Pages".into());
    }
    fn level(flat: &[(usize, Node)], at: &mut usize, depth: usize) -> Result<Vec<Node>, String> {
        let mut out = Vec::new();
        while *at < flat.len() {
            let (indent, node) = &flat[*at];
            if *indent < depth {
                break;
            }
            if *indent > depth {
                return Err("navigation skips a nesting level".into());
            }
            let mut node = node.clone();
            *at += 1;
            if flat.get(*at).is_some_and(|(indent, _)| *indent > depth) {
                node.children = level(flat, at, depth + 1)?;
            }
            out.push(node);
        }
        Ok(out)
    }
    level(&flat, &mut 0, 0)
}

pub fn structure(text: &str, translated: bool) -> Result<(), String> {
    let mut title = false;
    let mut pages = false;
    let mut section = "";
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if line.starts_with("<!-- Translated from ") && translated {
            continue;
        }
        if line.starts_with("# ") && !title && section.is_empty() {
            title = true;
            continue;
        }
        if line == "## Pages" {
            if pages {
                return Err("navigation repeats ## Pages".into());
            }
            pages = true;
            section = "pages";
            continue;
        }
        if line == "## URLs" && translated {
            section = "urls";
            continue;
        }
        let valid = match section {
            "pages" => line.trim_start().starts_with("- ["),
            "urls" => line
                .strip_prefix("- ")
                .and_then(|item| item.split_once(": "))
                .is_some_and(|(source, translated)| {
                    !source.trim().is_empty() && !translated.trim().is_empty()
                }),
            _ => false,
        };
        if !title || !valid {
            return Err("navigation contains prose or an unsupported section; use ## Pages links and translated ## URLs slug mappings".into());
        }
    }
    if !title {
        return Err("navigation has no site title".into());
    }
    nodes(text).map(|_| ())
}

pub struct Outlines {
    pub updates: Vec<Update>,
    /// Each page of the outlines by its file: its place in reading order,
    /// and the page it's under, if any. README indexes read in this order
    /// and nest as the website does.
    pub order: BTreeMap<PathBuf, (usize, Option<PathBuf>)>,
}

impl Outlines {
    pub fn new(tree: &Tree) -> Result<Self, String> {
        let packages = crate::shape::packages(tree);
        for path in tree.files.keys() {
            if !crate::navigation::selected(tree, &packages, path)
                || !tree
                    .websites
                    .iter()
                    .any(|web| web.shared && path.starts_with(&web.workspace))
                || !path.ancestors().skip(1).any(|dir| {
                    dir.file_name().is_some_and(|n| n == "docs")
                        // A crate or web app named `docs` holds no pages of its own.
                        && !packages.iter().any(|p| p == dir)
                        && !tree.websites.iter().any(|web| web.app == dir)
                        && dir.parent().is_some_and(|owner| {
                            owner == tree.root
                                || packages.iter().any(|p| p == owner)
                                || tree.websites.iter().any(|web| web.app == owner)
                                || tree.files.contains_key(&owner.join("layer.toml"))
                                || tree.files.contains_key(&owner.join("README.md"))
                        })
                })
            {
                continue;
            }
            if !tree
                .websites
                .iter()
                .flat_map(|web| &web.folders)
                .any(|folder| path.starts_with(folder.join("docs")))
            {
                return Err(format!(
                    "{}: English documentation owner is not registered in scripts/documentation-index.json",
                    tree.shown(path)
                ));
            }
        }
        for (path, text) in &tree.files {
            if metadata(tree, path)
                && crate::navigation::translation(path).is_some_and(|(english, _)| {
                    english.file_name().is_some_and(|n| n == "navigation.md")
                })
            {
                structure(text, true).map_err(|error| format!("{}: {error}", tree.shown(path)))?;
            }
        }
        if let Some((address, _)) = tree.pages.iter().find(|(_, files)| files.len() != 1) {
            return Err(format!("duplicate documentation ownership at {address}"));
        }
        let mut result = Self {
            updates: Vec::new(),
            order: BTreeMap::new(),
        };
        let mut websites: Vec<_> = tree.websites.iter().collect();
        websites.sort_by_key(|web| (!web.shared, &web.url));
        for web in websites {
            let path = web.app.join("docs/navigation.md");
            let old = tree.text(&path);
            if old.is_none() && tree.text(&web.app.join("docs/website.md")).is_some() {
                return Err(format!(
                    "{}: rename website metadata to navigation.md before writing",
                    tree.shown(&path)
                ));
            }
            let title = old
                .and_then(|s| s.lines().find_map(|l| l.strip_prefix("# ")))
                .unwrap_or(&web.name);
            let layout = parts::Layout::read(tree, &path)?;
            let mut outline = if let Some(text) = old {
                structure(text, false).map_err(|e| format!("{}: {e}", tree.shown(&path)))?;
                layout.nodes.clone()
            } else {
                Vec::new()
            };
            let mut seen = BTreeSet::new();
            resolve(tree, &path, &mut outline, &mut seen)?;
            refresh_labels(tree, web, &mut outline, true);
            let mut missing: Vec<_> = tree
                .pages
                .iter()
                .filter(|(url, _)| site_address(&web.url, url))
                .flat_map(|(url, paths)| paths.iter().map(move |p| (url.clone(), p.clone())))
                .filter(|(_, file)| !seen.contains(file))
                .collect();
            missing
                .sort_by_key(|(url, file)| (url.matches('/').count(), url.clone(), file.clone()));
            for (address, file) in missing {
                let source = tree.text(&file).unwrap_or_default();
                let label = source
                    .lines()
                    .find_map(|l| l.strip_prefix("# "))
                    .unwrap_or("Document")
                    .to_owned();
                let node = Node {
                    label,
                    target: relative(path.parent().unwrap(), &file),
                    children: Vec::new(),
                    file: Some(file),
                    address: Some(address),
                };
                insert(&mut outline, node);
            }
            nesting::normalize(&mut outline, &web.url);
            for node in &mut outline {
                read_in_order(tree, &mut node.children);
            }
            record(&outline, None, &mut result.order);
            result
                .updates
                .extend(layout.updates(tree, &path, title, &outline));
        }
        Ok(result)
    }
}

/// Each page of `nodes` in reading order, under `parent`, after the
/// pages `order` already holds.
fn record(
    nodes: &[Node],
    parent: Option<&PathBuf>,
    order: &mut BTreeMap<PathBuf, (usize, Option<PathBuf>)>,
) {
    for node in nodes {
        if let Some(file) = &node.file {
            let at = order.len();
            order.entry(file.clone()).or_insert((at, parent.cloned()));
        }
        record(&node.children, node.file.as_ref().or(parent), order);
    }
}

/// Each page's children in reading order, by kind (`kinds.toml`), the
/// curated order kept within a kind. The site's top level is the
/// curator's alone.
fn read_in_order(tree: &Tree, nodes: &mut [Node]) {
    nodes.sort_by_cached_key(|node| match &node.file {
        Some(file) => crate::kinds::of(file, tree.text(file).unwrap_or_default()).0,
        None => crate::kinds::of(Path::new("link.md"), "").0,
    });
    for node in nodes {
        read_in_order(tree, &mut node.children);
    }
}

fn refresh_labels(tree: &Tree, web: &Website, nodes: &mut [Node], first: bool) {
    for (i, node) in nodes.iter_mut().enumerate() {
        if !(first && i == 0 && node.label == "Get Started")
            && !(first && node.target.ends_with(" \"tab\""))
            && node
                .address
                .as_ref()
                .is_some_and(|address| site_address(&web.url, address))
            && let Some(source) = node.file.as_ref().and_then(|file| tree.text(file))
            && let Some(title) = source.lines().find_map(|line| line.strip_prefix("# "))
        {
            node.label = title.to_owned();
        }
        refresh_labels(tree, web, &mut node.children, false);
    }
}

fn site_address(site: &str, url: &str) -> bool {
    url == site
        || url
            .strip_prefix(site)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn resolve(
    tree: &Tree,
    outline: &Path,
    nodes: &mut [Node],
    seen: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut external = BTreeSet::new();
    fn visit(
        tree: &Tree,
        outline: &Path,
        nodes: &mut [Node],
        seen: &mut BTreeSet<PathBuf>,
        external: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        for node in nodes {
            let target = crate::md::target_path(&node.target);
            if target.contains("://") {
                if let Some((site, address)) =
                    tree.site_page(target.split(['#', '?']).next().unwrap_or_default())
                {
                    let files = tree.pages.get(address).ok_or_else(|| {
                        format!("{}: dangling page {}", outline.display(), node.target)
                    })?;
                    node.file = files.first().cloned();
                    node.address = Some(format!(
                        "{}{}",
                        site.url,
                        address.strip_prefix(&site.url).unwrap()
                    ));
                } else if !external.insert(target.to_owned()) {
                    return Err(format!(
                        "{}: duplicate link {}",
                        outline.display(),
                        node.target
                    ));
                }
            } else {
                let target = target.split(['#', '?']).next().unwrap_or_default();
                let mut file = normal(&outline.parent().unwrap().join(link_path(target)));
                if tree.dirs.contains(&file) {
                    file.push("README.md");
                }
                let address = tree
                    .pages
                    .iter()
                    .find_map(|(address, files)| files.contains(&file).then_some(address.clone()));
                if address.is_none() {
                    return Err(format!(
                        "{}: dangling or non-page target {}",
                        outline.display(),
                        node.target
                    ));
                }
                node.file = Some(file);
                node.address = address;
            }
            if let Some(file) = &node.file
                && !seen.insert(file.clone())
            {
                return Err(format!(
                    "{}: duplicate page {}",
                    outline.display(),
                    file.display()
                ));
            }
            visit(tree, outline, &mut node.children, seen, external)?;
        }
        Ok(())
    }
    visit(tree, outline, nodes, seen, &mut external)
}

fn insert(nodes: &mut Vec<Node>, node: Node) {
    fn nearest(nodes: &[Node], address: &str, best: &mut Option<String>) {
        for candidate in nodes {
            if let Some(parent) = &candidate.address
                && address.starts_with(&format!("{parent}/"))
                && parent.split('/').count() > 3
                && best.as_ref().is_none_or(|old| parent.len() > old.len())
            {
                *best = Some(parent.clone());
            }
            nearest(&candidate.children, address, best);
        }
    }
    fn append(nodes: &mut [Node], parent: &str, node: &mut Option<Node>) -> bool {
        for candidate in nodes {
            if candidate.address.as_deref() == Some(parent) {
                candidate.children.push(node.take().unwrap());
                return true;
            }
            if append(&mut candidate.children, parent, node) {
                return true;
            }
        }
        false
    }
    let mut best = None;
    nearest(nodes, node.address.as_deref().unwrap(), &mut best);
    if let Some(parent) = best {
        append(nodes, &parent, &mut Some(node));
    } else {
        nodes.push(node);
    }
}

fn render(nodes: &[Node], depth: usize, text: &mut String) {
    for node in nodes {
        *text += &format!(
            "{}- [{}]({})\n",
            "  ".repeat(depth),
            node.label,
            node.target
        );
        render(&node.children, depth + 1, text);
    }
}

fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut result = PathBuf::new();
    for _ in shared..from.len() {
        result.push("..");
    }
    for part in &to[shared..] {
        result.push(part.as_os_str());
    }
    result
        .to_string_lossy()
        .replace('\\', "/")
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('#', "%23")
}
