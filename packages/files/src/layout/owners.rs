//! One folder per owner in each part, named by the owner's plural word at
//! the same path in every part. A folder directly in `~/Link` or in its
//! mirror `~/Link/Safe` is Title Case (`~/Link/Agents`,
//! `~/Link/Safe/Models`); every other folder is lowercase.
//! An agent `main` is `~/Link/Agents/main/`, `~/Link/Safe/Agents/main/`,
//! `~/.lnk/safe/agents/main/` and `~/.lnk/data/agents/main/`. Link's own
//! files sit at each part's root.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use anyhow::{bail, ensure};
use serde::Deserialize;

use super::{LAYOUT, Layout};

/// A part of the layout: where an owner may have its one folder.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Part {
    /// `~/Link`: what you see and work in.
    Link,
    /// `~/Link/Safe`: what you edit that steers Link.
    LinkSafe,
    /// `~/Link/Archive`: what's kept of what's gone.
    Archive,
    /// `~/Link/Logs`.
    Logs,
    /// `~/.lnk/safe`: what Link keeps that steers it.
    Safe,
    /// `~/.lnk/data`: what Link keeps that a run may be granted.
    Data,
    /// `~/.lnk/exec`: what a run may execute.
    Exec,
}

impl Part {
    /// Whether an owner's folder in it is directly in `~/Link` or in
    /// `~/Link/Safe`, which mirrors it: the folders in Title Case.
    fn top(self) -> bool {
        matches!(
            self,
            Part::Link | Part::LinkSafe | Part::Archive | Part::Logs
        )
    }
}

/// An owner of one folder in each part it's in.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    /// The module that keeps it.
    pub module: String,
    /// Every part it has a folder in; any other is declared one-sided.
    pub parts: BTreeSet<Part>,
    /// The word for one of its things, as an archive selects it
    /// (`agent.programmer`): none for an owner whose things no archive holds.
    #[serde(default)]
    pub kind: Option<String>,
    /// The parts whose folder of a thing is its state, which an archive
    /// carries whole: only parts under `~/Link`, where everything a
    /// thing keeps is state.
    #[serde(default)]
    pub state: BTreeSet<Part>,
}

impl Part {
    /// Whether it's under `~/Link`, where only state is kept.
    pub fn in_link(self) -> bool {
        matches!(self, Part::Link | Part::LinkSafe)
    }
}

/// The owner whose things are of kind `kind`, and its word.
pub fn owner_of_kind(kind: &str) -> Option<(&'static str, &'static Owner)> {
    LAYOUT
        .owners
        .iter()
        .find(|(_, owner)| owner.kind.as_deref() == Some(kind))
        .map(|(word, owner)| (word.as_str(), owner))
}

/// The locations that are owner `word`'s folders in its state parts, a
/// thing's folder in each being `<location>/<name>`: what an archive
/// carries of each of its things.
pub fn state_locations(word: &str) -> Vec<(Part, super::Home)> {
    let Some(owner) = LAYOUT.owners.get(word) else {
        return Vec::new();
    };
    super::Home::ALL
        .iter()
        .filter(|home| home.owner() == word)
        .filter_map(|home| {
            let (part, rest) = split(&LAYOUT, &Located::Home(home.relative().into()));
            let part = part?;
            (rest.len() == 1 && owner.state.contains(&part)).then_some((part, *home))
        })
        .collect()
}

/// Where part `part` is, for the Link folder `link` and the machine
/// folder `hidden`: the root an owner's folder in it is in.
pub fn part_root(part: Part, link: &Path, hidden: &Path) -> std::path::PathBuf {
    let b = &LAYOUT.boundaries;
    match part {
        Part::Link => link.into(),
        Part::LinkSafe => link.join(&b.home_safe),
        Part::Archive => link.join("Archive"),
        Part::Logs => link.join("Logs"),
        Part::Safe => hidden.join(&b.hidden_safe),
        Part::Data => hidden.join(&b.data),
        Part::Exec => hidden.join(&b.exec),
    }
}

/// Every folder thing `name` of owner `word` has, in each part its owner
/// has folders in, whether or not it's there: a name is taken here while
/// any of them is.
pub fn thing_folders(
    word: &str,
    name: &str,
    link: &Path,
    hidden: &Path,
) -> Vec<std::path::PathBuf> {
    let Some(owner) = LAYOUT.owners.get(word) else {
        return Vec::new();
    };
    owner
        .parts
        .iter()
        .filter_map(|part| {
            let folder = folder_in(&LAYOUT, word, *part).ok()?;
            Some(part_root(*part, link, hidden).join(folder).join(name))
        })
        .collect()
}

/// Every owner, by its word.
pub fn owners() -> &'static BTreeMap<String, Owner> {
    &LAYOUT.owners
}

/// The folder of `owner` in `part`: its word, Title Case directly in
/// `~/Link` or `~/Link/Safe`.
pub fn owner_folder(owner: &str, part: Part) -> anyhow::Result<String> {
    folder_in(&LAYOUT, owner, part)
}

fn folder_in(layout: &Layout, owner: &str, part: Part) -> anyhow::Result<String> {
    let Some(declared) = layout.owners.get(owner) else {
        bail!("{owner:?} isn't an owner in layout.toml");
    };
    ensure!(
        declared.parts.contains(&part),
        "{owner} has no folder in {part:?}"
    );
    Ok(if part.top() {
        title(owner)
    } else {
        owner.into()
    })
}

fn title(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// The names the layout puts directly in `owner`'s folders, which none of
/// its instances can take: an agent can't be named `journal`.
pub fn reserved(owner: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (path, location_owner) in paths(&LAYOUT) {
        if location_owner != owner {
            continue;
        }
        let (_, rest) = split(&LAYOUT, &path);
        if let Some(name) = rest.get(1) {
            names.insert(name.to_ascii_lowercase());
        }
    }
    names
}

/// Every location's path, relative to its root, and its owner, Home's
/// marked by a leading `~/Link` component the split below reads.
fn paths(layout: &Layout) -> Vec<(Located, &str)> {
    layout
        .home
        .values()
        .map(|location| {
            (
                Located::Home(location.path.clone()),
                location.owner.as_str(),
            )
        })
        .chain(layout.hidden.values().map(|location| {
            (
                Located::Hidden(location.path.clone()),
                location.owner.as_str(),
            )
        }))
        .collect()
}

enum Located {
    Home(String),
    Hidden(String),
}

/// A location's part and its components within that part.
fn split(layout: &Layout, located: &Located) -> (Option<Part>, Vec<String>) {
    let (path, home) = match located {
        Located::Home(path) => (path, true),
        Located::Hidden(path) => (path, false),
    };
    let parts: Vec<String> = Path::new(path)
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let b = &layout.boundaries;
    let (part, rest) = match (home, parts.first().map(String::as_str)) {
        (true, Some(first)) if first == b.home_safe => (Part::LinkSafe, 1),
        (true, Some("Archive")) => (Part::Archive, 1),
        (true, Some("Logs")) => (Part::Logs, 1),
        (true, _) => (Part::Link, 0),
        (false, Some(first)) if first == b.hidden_safe => (Part::Safe, 1),
        (false, Some(first)) if first == b.data => (Part::Data, 1),
        (false, Some(first)) if first == b.exec => (Part::Exec, 1),
        (false, _) => return (None, parts),
    };
    if rest == 1 && parts.len() == 1 {
        // The part's root itself, Link's.
        return (None, parts);
    }
    (Some(part), parts[rest..].to_vec())
}

impl Layout {
    /// Each location is in its owner's folder of its part, or Link's own at
    /// the part's root; every owner has its folder in exactly the parts it
    /// declares, so each path has its twin in the others or is declared
    /// one-sided.
    pub(super) fn check_owners(&self) -> anyhow::Result<()> {
        let core = &self.boundaries.core;
        let mut seen: BTreeMap<&str, BTreeSet<Part>> = BTreeMap::new();
        let mut kinds = BTreeSet::new();
        for (word, owner) in &self.owners {
            super::check_owner_name(word, core)?;
            ensure!(
                !owner.parts.is_empty(),
                "{word} has a folder in at least one part"
            );
            ensure!(
                owner.state.is_subset(&owner.parts),
                "{word}'s state is in parts it has folders in"
            );
            ensure!(
                owner.state.iter().all(|part| part.in_link()),
                "{word}'s state is only under ~/Link, never in ~/.lnk"
            );
            if let Some(kind) = &owner.kind {
                super::check_owner_name(kind, core)?;
                ensure!(
                    !kind.contains('.') && kinds.insert(kind.as_str()),
                    "{word}'s kind {kind:?} is one word no other owner has"
                );
                ensure!(
                    !owner.state.is_empty(),
                    "{word} has a kind, so it says which of its parts are state"
                );
            }
        }
        let owner_words: BTreeSet<String> = self
            .owners
            .keys()
            .flat_map(|word| [word.clone(), title(word)])
            .collect();
        for (located, owner) in paths(self) {
            let (part, rest) = split(self, &located);
            let shown = match &located {
                Located::Home(path) => format!("~/Link/{path}"),
                Located::Hidden(path) => format!("~/.lnk/{path}"),
            };
            if owner == core {
                ensure!(
                    rest.first()
                        .is_none_or(|first| !owner_words.contains(first)),
                    "{shown} is Link's own, so it's not in an owner's folder"
                );
                continue;
            }
            let Some(part) = part else {
                bail!("{shown} belongs to {owner} but isn't in a part")
            };
            let folder = folder_in(self, owner, part)?;
            ensure!(
                rest.first() == Some(&folder),
                "{shown} belongs to {owner}, whose folder there is {folder}"
            );
            seen.entry(owner).or_default().insert(part);
        }
        for (word, owner) in &self.owners {
            let found = seen.remove(word.as_str()).unwrap_or_default();
            ensure!(
                found == owner.parts,
                "{word} declares folders in {:?} but has them in {found:?}",
                owner.parts
            );
        }
        Ok(())
    }
}
