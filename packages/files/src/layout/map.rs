//! The map: `~/.lnk/safe/map.toml`, each location's ID in `layout.toml`
//! mapped to where it is on this machine, pinned the first time it's used,
//! so a later release changing a default leaves an existing machine as it is.
//!
//! The map is at a fixed path, the one location never mapped. The Link
//! folder's entry, `link`, is relative to the user's home folder; every
//! other entry is relative to its part's root (the Link folder or `~/.lnk`),
//! or absolute for data kept elsewhere. Reading it refuses an entry that
//! puts control outside a Safe, code outside `exec`, data inside either, or
//! two locations where their defaults don't overlap.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use super::{Hidden, Home, Kind, LAYOUT};

/// The map's ID for the Link folder itself.
pub const LINK: &str = "link";
const HOME_PREFIX: &str = "link.";
const HIDDEN_PREFIX: &str = "lnk.";

/// A location the map can pin: the Link folder, or a location in it or in
/// `~/.lnk`. Its ID is stable while its default path changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Entry {
    Link,
    Home(Home),
    Hidden(Hidden),
}

impl Entry {
    /// Every entry the map holds, in the order `lnk folder list` shows them.
    pub fn all() -> impl Iterator<Item = Entry> {
        std::iter::once(Entry::Link)
            .chain(Home::ALL.iter().map(|home| Entry::Home(*home)))
            .chain(
                Hidden::ALL
                    .iter()
                    .filter(|hidden| **hidden != Hidden::Map)
                    .map(|hidden| Entry::Hidden(*hidden)),
            )
    }

    pub fn name(self) -> String {
        match self {
            Entry::Link => LINK.into(),
            Entry::Home(home) => format!("{HOME_PREFIX}{}", home.id()),
            Entry::Hidden(hidden) => format!("{HIDDEN_PREFIX}{}", hidden.id()),
        }
    }

    /// The entry named `name`, refusing the map itself and unknown names.
    pub fn parse(name: &str) -> anyhow::Result<Entry> {
        if name == LINK {
            return Ok(Entry::Link);
        }
        let found = Entry::all().find(|key| key.name() == name);
        if found.is_none() && name == Entry::Hidden(Hidden::Map).name() {
            bail!("{name} is the map itself, which is never mapped");
        }
        found.with_context(|| format!("{name:?} isn't a location: lnk folder list shows them"))
    }

    pub fn kind(self) -> Kind {
        match self {
            Entry::Link => Kind::Data,
            Entry::Home(home) => home.kind(),
            Entry::Hidden(hidden) => hidden.kind(),
        }
    }

    /// The layout's default, relative to the key's root.
    pub fn default_path(self) -> &'static str {
        match self {
            Entry::Link => &LAYOUT.roots.home,
            Entry::Home(home) => home.relative(),
            Entry::Hidden(hidden) => hidden.relative(),
        }
    }
}

/// The map as written: the Link folder and each location. Its schema is
/// declared in `map/schema.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub locations: BTreeMap<String, String>,
}

/// The roots a map's entries are resolved against.
#[derive(Clone, Debug)]
pub struct Roots {
    pub user_home: PathBuf,
    pub hidden: PathBuf,
}

crate::specification!(Map, include_str!("map/schema.toml"));

impl Map {
    /// The entry for `key`, if pinned.
    pub fn entry(&self, key: Entry) -> Option<&str> {
        match key {
            Entry::Link => self.link.as_deref(),
            other => self.locations.get(&other.name()).map(String::as_str),
        }
    }

    fn set_entry(&mut self, key: Entry, value: String) {
        match key {
            Entry::Link => self.link = Some(value),
            other => {
                self.locations.insert(other.name(), value);
            }
        }
    }

    /// `key`'s entry, else its default, relative to its root or absolute.
    pub fn relative(&self, key: Entry) -> &str {
        self.entry(key).unwrap_or(key.default_path())
    }

    /// The Link folder under `user_home`.
    pub fn link_folder(&self, user_home: &Path) -> PathBuf {
        resolve(user_home, self.relative(Entry::Link))
    }

    /// Where `key` is, its root resolved from `roots`.
    pub fn path(&self, key: Entry, roots: &Roots) -> PathBuf {
        match key {
            Entry::Link => self.link_folder(&roots.user_home),
            Entry::Home(_) => resolve(&self.link_folder(&roots.user_home), self.relative(key)),
            Entry::Hidden(_) => resolve(&roots.hidden, self.relative(key)),
        }
    }

    /// Pin every location not yet pinned to its default. True when a key
    /// was added, so the map needs writing.
    pub fn pin(&mut self) -> bool {
        let mut added = false;
        for key in Entry::all() {
            if self.entry(key).is_none() {
                self.set_entry(key, key.default_path().into());
                added = true;
            }
        }
        added
    }

    /// Refuse an entry in the wrong part, an unknown key or an overlap the
    /// defaults don't have. Its schema is checked as it's read.
    pub fn check(&self, roots: &Roots) -> anyhow::Result<()> {
        for name in self.locations.keys() {
            let key = Entry::parse(name)?;
            ensure!(
                key != Entry::Link,
                "the Link folder's entry is link, outside [locations]"
            );
        }
        let link = self.link_folder(&roots.user_home);
        ensure!(
            link.starts_with(&roots.user_home)
                || Path::new(self.relative(Entry::Link)).is_absolute(),
            "the Link folder is in the home folder or at an absolute path"
        );
        if let Some(entry) = &self.link {
            checked_entry(entry)?;
            if !Path::new(entry).is_absolute() {
                super::visible(Path::new(entry))?;
            }
        }
        ensure!(
            !link.starts_with(&roots.hidden) && !roots.hidden.starts_with(&link),
            "the Link folder and {} are apart",
            roots.hidden.display()
        );
        let safes = [
            self.path(Entry::Home(Home::Safe), roots),
            roots.hidden.join(&LAYOUT.boundaries.hidden_safe),
        ];
        let exec = roots.hidden.join(&LAYOUT.boundaries.exec);
        let mut resolved = Vec::new();
        for key in Entry::all().filter(|key| *key != Entry::Link) {
            let entry = self.relative(key);
            checked_entry(entry)?;
            let path = self.path(key, roots);
            let in_safe = safes.iter().any(|safe| path.starts_with(safe));
            let name = key.name();
            match key.kind() {
                Kind::Control => ensure!(
                    in_safe || key == Entry::Home(Home::Safe),
                    "{name} is control, which stays in a Safe: {} isn't",
                    path.display()
                ),
                Kind::Exec => ensure!(
                    path.starts_with(&exec),
                    "{name} holds programs, which stay in {}",
                    exec.display()
                ),
                Kind::Data => ensure!(
                    !in_safe && !path.starts_with(&exec),
                    "{name} is data, which a run may reach: {} is in a Safe or exec",
                    path.display()
                ),
            }
            if key.kind() == Kind::Data && matches!(key, Entry::Hidden(_)) {
                ensure!(
                    !path.starts_with(&link),
                    "{name} is in the Link folder, which isn't its part"
                );
            }
            resolved.push((key, path));
        }
        for (index, (key, path)) in resolved.iter().enumerate() {
            for (other, there) in &resolved[index + 1..] {
                let nested = |inner: Entry, outer: Entry| {
                    same_root(inner, outer)
                        && Path::new(inner.default_path()).starts_with(outer.default_path())
                };
                // Each pair compared once each way: a program checks the
                // map at its start, every pair of nearly a hundred.
                let (inside, around) = (path.starts_with(there), there.starts_with(path));
                if !inside && !around {
                    continue;
                }
                let allowed = (inside && nested(*key, *other) && path != there)
                    || (around && nested(*other, *key) && path != there);
                ensure!(
                    allowed,
                    "{} and {} overlap at {}",
                    key.name(),
                    other.name(),
                    if inside { there } else { path }.display()
                );
            }
        }
        Ok(())
    }
}

fn same_root(a: Entry, b: Entry) -> bool {
    matches!(
        (a, b),
        (Entry::Home(_), Entry::Home(_)) | (Entry::Hidden(_), Entry::Hidden(_))
    )
}

fn checked_entry(entry: &str) -> anyhow::Result<()> {
    let path = Path::new(entry);
    ensure!(
        !entry.is_empty()
            && path
                .components()
                .all(|part| { matches!(part, Component::RootDir | Component::Normal(_)) }),
        "{entry:?} isn't a plain path: no .., . or empty part"
    );
    Ok(())
}

fn resolve(root: &Path, entry: &str) -> PathBuf {
    if Path::new(entry).is_absolute() {
        PathBuf::from(entry)
    } else {
        root.join(entry)
    }
}

/// Read the map in installation folder `hidden`: an empty one when missing.
pub fn read(hidden: &Path) -> anyhow::Result<Map> {
    let path = Hidden::Map.at_default(hidden);
    #[cfg(test)]
    tests::READS.with(|reads| reads.set(reads.get() + 1));
    let map: Option<Map> = crate::saved::read_optional(&path)?;
    Ok(map.unwrap_or_default())
}

/// Read and check the map in `hidden`, its entries resolved for `user_home`.
pub fn load(user_home: &Path, hidden: &Path) -> anyhow::Result<Map> {
    let map = read(hidden)?;
    map.check(&Roots {
        user_home: user_home.into(),
        hidden: hidden.into(),
    })
    .with_context(|| format!("reading {}", Hidden::Map.at_default(hidden).display()))?;
    Ok(map)
}

/// Write `map` whole, after checking it, through `lock`, the map's lock.
fn write(
    lock: &crate::saved::Held,
    user_home: &Path,
    hidden: &Path,
    map: &Map,
) -> anyhow::Result<()> {
    map.check(&Roots {
        user_home: user_home.into(),
        hidden: hidden.into(),
    })?;
    lock.write(map)?;
    forget(&Hidden::Map.at_default(hidden));
    Ok(())
}

/// Pin every location not yet in the map in `hidden`, under its lock.
pub fn pin(user_home: &Path, hidden: &Path) -> anyhow::Result<()> {
    let path = Hidden::Map.at_default(hidden);
    let lock = crate::settings::lock(&path)?;
    let mut map = load(user_home, hidden)?;
    if map.pin() {
        write(&lock, user_home, hidden, &map)?;
    }
    Ok(())
}

/// Move `key` to `to`: its files, when they're there and `to` isn't, and its
/// entry with every entry nested in it. When the files are already at `to`
/// and gone from where the map said, only the entry changes. Returns where
/// it was.
pub fn set(user_home: &Path, hidden: &Path, key: Entry, to: &Path) -> anyhow::Result<PathBuf> {
    ensure!(to.is_absolute(), "{} isn't an absolute path", to.display());
    checked_entry(&to.display().to_string())?;
    let path = Hidden::Map.at_default(hidden);
    let lock = crate::settings::lock(&path)?;
    let mut map = load(user_home, hidden)?;
    map.pin();
    let roots = Roots {
        user_home: user_home.into(),
        hidden: hidden.into(),
    };
    let was = map.path(key, &roots);
    if was == to {
        return Ok(was);
    }
    // What's inside it moves with it: an entry under it, relative to the
    // Link folder's or written out whole, follows to the same place in `to`.
    let nested: Vec<(Entry, PathBuf)> = Entry::all()
        .filter(|other| *other != key)
        .map(|other| (other, map.path(other, &roots)))
        .filter(|(other, there)| {
            there.starts_with(&was)
                && (key != Entry::Link || Path::new(map.relative(*other)).is_absolute())
        })
        .collect();
    let root_of = |map: &Map, key: Entry| match key {
        Entry::Link => user_home.to_path_buf(),
        Entry::Home(_) => map.link_folder(user_home),
        Entry::Hidden(_) => hidden.to_path_buf(),
    };
    let entry = relative_to(&root_of(&map, key), to);
    map.set_entry(key, entry);
    for (other, there) in &nested {
        let moved = to.join(there.strip_prefix(&was).expect("nested"));
        let entry = relative_to(&root_of(&map, *other), &moved);
        map.set_entry(*other, entry);
    }
    map.check(&roots)?;
    match (exists(&was)?, exists(to)?) {
        (true, false) => {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("making {}", parent.display()))?;
            }
            std::fs::rename(&was, to).map_err(|err| {
                if err.raw_os_error() == Some(libc::EXDEV) {
                    anyhow::anyhow!(
                        "{} is on another disk from {}: copy it there, delete the original, then set it again",
                        to.display(),
                        was.display()
                    )
                } else {
                    anyhow::Error::new(err)
                        .context(format!("moving {} to {}", was.display(), to.display()))
                }
            })?;
        }
        (false, _) => {}
        (true, true) => bail!(
            "{} and {} both exist: move or delete one first",
            was.display(),
            to.display()
        ),
    }
    write(&lock, user_home, hidden, &map)?;
    Ok(was)
}

fn exists(path: &Path) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// `path` relative to `root` when inside it, else absolute.
fn relative_to(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
        _ => path.display().to_string(),
    }
}

/// Which file a map is (a write replaces it with another), when it was
/// last changed and how long it is, or none when it's missing: while it's
/// the same, the file is the one already read.
type Stamp = Option<(u64, std::time::SystemTime, u64)>;
type Cached = (PathBuf, Stamp, Map);
static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());
/// Maps read and checked: the map's path, the user's home it was checked
/// for, its stamp and the map.
type Checked = (PathBuf, PathBuf, Stamp, Map);
static CHECKED: Mutex<Vec<Checked>> = Mutex::new(Vec::new());

/// Drops what was read of the map at `path`, which a write just replaced:
/// every other map's reading stays.
fn forget(path: &Path) {
    CACHE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .retain(|(at, _, _)| at != path);
    CHECKED
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .retain(|(at, _, _, _)| at != path);
}

fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| Some((meta.ino(), meta.modified().ok()?, meta.len())))
}

/// [`load`], read and checked once while its file is unchanged: each path
/// a program resolves in the Link folder asks for the map, dozens of times
/// in one command. A map refused is read again when next asked for.
pub fn load_cached(user_home: &Path, hidden: &Path) -> anyhow::Result<Map> {
    let path = Hidden::Map.at_default(hidden);
    let seen = stamp(&path);
    {
        let checked = CHECKED.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some((_, _, _, map)) = checked
            .iter()
            .find(|(at, home, was, _)| at == &path && home == user_home && was == &seen)
        {
            return Ok(map.clone());
        }
    }
    let map = load(user_home, hidden)?;
    let mut checked = CHECKED.lock().unwrap_or_else(|poison| poison.into_inner());
    checked.retain(|(at, home, _, _)| !(at == &path && home == user_home));
    checked.push((path, user_home.to_path_buf(), seen, map.clone()));
    Ok(map)
}

/// The map in `hidden`, unchecked, as cheaply as its file allows: a reader
/// asking for many paths reads it once while it's unchanged. A map that
/// can't be read is the defaults here; `lnk` refuses it at start.
pub(super) fn cached(hidden: &Path) -> Map {
    let path = Hidden::Map.at_default(hidden);
    let stamp = stamp(&path);
    let mut cache = CACHE.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some((_, _, map)) = cache
        .iter()
        .find(|(at, seen, _)| at == &path && seen == &stamp)
    {
        return map.clone();
    }
    let map = if stamp.is_some() {
        read(hidden).unwrap_or_else(|_| Map::default())
    } else {
        Map::default()
    };
    cache.retain(|(at, _, _)| at != &path);
    cache.push((path, stamp, map.clone()));
    map
}

#[cfg(test)]
mod tests;
