//! Where Link keeps things: the Link folder, `~/Link` unless this machine's
//! map names another place, is the owner's: their agents, teams and files.
//! `~/.lnk` is this machine's: `lnk` and its modules, its settings and
//! identity, journals, caches. Where each location is on this machine is
//! the map's (`link_files::layout::map`), pinned from `layout.toml`'s
//! defaults; this module finds the Link folder through it, marks it, and
//! keeps the settings that travel with it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use link_format::WithinExt;
use serde::{Deserialize, Serialize};

use link_files::layout::map::{self, Entry};

/// The shared Home and Hidden roots.
pub use link_files::layout::Root as Folder;

mod rewriting;
pub use rewriting::{is_database, rewrite_arrived, rewrite_paths, rewrite_tree};

/// `text` with each path in folder `was`, whole (`/a/b` in `/a/b/c`, not
/// in `/a/bc`), in folder `now` instead.
pub fn replace_paths(text: &str, was: &str, now: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(was) {
        let after = &rest[at + was.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.');
        out.push_str(&rest[..at]);
        out.push_str(if whole { now } else { was });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `text` with each absolute path ending in `tail`, whole, as `now`.
fn replace_arrived(text: &str, tail: &str, now: &str) -> String {
    // What ends a path written in settings or state: a quote, a space,
    // a separator of the format around it.
    let ends = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '=' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '`'
            )
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(tail) {
        let after = &rest[at + tail.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.');
        let before = &rest[..at];
        let start = before.rfind(ends).map_or(0, |i| i + 1);
        let prefix = &before[start..];
        if whole && prefix.starts_with('/') {
            out.push_str(&before[..start]);
            out.push_str(now);
        } else {
            out.push_str(before);
            out.push_str(tail);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `path` with the home folder as `~`, as Link says a path.
pub fn tilde(path: &Path) -> String {
    match link_files::try_home_dir()
        .ok()
        .and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

use link_files::layout::{Hidden, Home, LAYOUT, Marker};

pub use link_files::{INSTALLATION_DIR_VAR, installation_dir, try_installation_dir};

/// The Link folder: `~/Link`, or where this machine's settings say it
/// went. Without an absolute `HOME`, `./Link`: code that deletes or
/// trusts the folder uses [`try_link_dir`] instead.
pub fn link_dir() -> PathBuf {
    Folder::Home.path()
}

/// [`link_dir`], or an error when it can't be an absolute path or the
/// settings naming it can't be read.
pub fn try_link_dir() -> anyhow::Result<PathBuf> {
    link_dir_in(&link_files::try_home_dir()?, &try_installation_dir()?)
}

/// The Link folder of home folder `home`, as installation folder `machine`
/// names it.
fn link_dir_in(home: &Path, machine: &Path) -> anyhow::Result<PathBuf> {
    link_files::layout::link_in(home, machine)
}

/// The Home locator in Hidden and the preferences kept with Home.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The ID of the Link folder this machine uses, from its marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Each module added from outside Link, by name: where it comes from
    /// and the key its releases must be signed with.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modules: BTreeMap<String, ModuleSource>,
    /// Programs in `lnk`'s folder that are neither Link's nor added from
    /// outside, which `lnk upgrade` has said once how to keep updated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noted: Vec<String>,
    /// How output looks: `[output]`, each key also a variable
    /// (`LNK_OUTPUT_COLOR`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub output: link_format::look::Settings,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Where a module from outside Link comes from, and the key its releases
/// must be signed with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleSource {
    /// Its repository: `https://github.com/<owner>/<repo>`.
    pub source: String,
    /// The signer pinned on the first `add`: `ssh-ed25519 <base64>`.
    pub signer: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    noted: Vec<String>,
}

link_files::specification!(MachineSettings, include_str!("folders/machine.schema.toml"));
link_files::specification!(Preferences, include_str!("folders/preferences.schema.toml"));

#[derive(Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    modules: BTreeMap<String, ModuleSource>,
    #[serde(default, skip_serializing_if = "is_default")]
    output: link_format::look::Settings,
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_in(&link_files::try_home_dir()?, &try_installation_dir()?)
    }

    /// Writes them whole.
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_in(&link_files::try_home_dir()?, &try_installation_dir()?)
    }

    fn path_in(machine: &Path) -> PathBuf {
        Hidden::Settings.at(machine)
    }

    fn load_at(machine: &Path, dir: &Path) -> anyhow::Result<Self> {
        fn read<T: link_files::schema::Specification + Default>(path: &Path) -> anyhow::Result<T> {
            link_files::saved::read(path)
        }
        let machine: MachineSettings = read(&Self::path_in(machine))?;
        let preferences: Preferences = read(&Home::Settings.at(dir))?;
        Ok(Self {
            id: machine.id,
            noted: machine.noted,
            modules: preferences.modules,
            output: preferences.output,
        })
    }

    fn load_in(user_home: &Path, machine: &Path) -> anyhow::Result<Self> {
        Self::load_at(machine, &link_dir_in(user_home, machine)?)
    }

    fn save_in(&self, user_home: &Path, machine: &Path) -> anyhow::Result<()> {
        map::pin(user_home, machine)?;
        let dir = link_dir_in(user_home, machine)?;
        let preferences = Preferences {
            modules: self.modules.clone(),
            output: self.output,
        };
        let path = Home::Settings.at(&dir);
        if preferences != Preferences::default() || path.exists() {
            link_files::saved::write(&path, &preferences)?;
        }
        let machine_settings = MachineSettings {
            id: self.id.clone(),
            noted: self.noted.clone(),
        };
        link_files::saved::write(&Self::path_in(machine), &machine_settings)
            .within(format!("writing {}", Self::path_in(machine).display()))
    }
}

/// The ID in the marker of folder `dir`, if it's a Link folder.
pub fn marker_id(dir: &Path) -> Option<String> {
    link_files::layout::read_marker(dir)
        .ok()
        .flatten()
        .map(|marker| marker.id)
}

/// Where the Link folder stands on this machine.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// It's where the settings say, marked as theirs (or nothing was
    /// recorded yet, and it's there or not made yet).
    Here(PathBuf),
    /// The settings record a Link folder that isn't at its place: moved,
    /// renamed or deleted. Link asks where it went, never searching.
    Lost(PathBuf),
}

/// Inspect the Link folder without creating its marker or machine records.
pub fn inspect() -> anyhow::Result<Found> {
    inspect_in(&link_files::try_home_dir()?, &try_installation_dir()?)
}

fn inspect_in(home: &Path, machine: &Path) -> anyhow::Result<Found> {
    let settings = Settings::load_in(home, machine)?;
    let dir = link_dir_in(home, machine)?;
    Ok(match &settings.id {
        Some(known) if marker_id(&dir).as_ref() != Some(known) => Found::Lost(dir),
        _ => Found::Here(dir),
    })
}

/// Failure logging can use a selected, marked Home even when its locator
/// contents cannot be parsed. Trust failures and known identity mismatches
/// still refuse it, and damaged settings never authorize creating a Home.
pub(super) enum RecordHome {
    DamagedLocator(PathBuf),
    Need(PathBuf),
}

pub(super) fn record_home_in(home: &Path, machine: &Path) -> anyhow::Result<RecordHome> {
    let locations = map::load_cached(home, machine)?;
    let dir = locations.link_folder(home);
    let marker = link_files::layout::read_marker(&dir)?;
    let text = link_files::settings::text_optional(&Settings::path_in(machine))?;
    let locator = text
        .as_deref()
        .map(link_files::saved::parse::<MachineSettings>)
        .transpose();
    match locator {
        Ok(locator) => {
            if let Some(known) = locator.and_then(|locator| locator.id) {
                anyhow::ensure!(
                    marker.as_ref().is_some_and(|marker| marker.id == known),
                    "the recorded Link folder is missing at {}",
                    dir.display()
                );
            }
            Ok(RecordHome::Need(dir))
        }
        Err(error) => {
            anyhow::ensure!(
                locations.link.is_some() && dir.is_dir() && marker.is_some(),
                "cannot select a failure record folder with damaged locator settings: {error}"
            );
            Ok(RecordHome::DamagedLocator(dir))
        }
    }
}

/// Need the Link folder for state or a run: create and protect it, then
/// remember its identity. A moved folder must be selected first.
pub fn need() -> anyhow::Result<PathBuf> {
    need_in(&link_files::try_home_dir()?, &try_installation_dir()?)
}

pub(super) fn need_in(home: &Path, machine: &Path) -> anyhow::Result<PathBuf> {
    let dir = match inspect_in(home, machine)? {
        Found::Here(dir) => dir,
        Found::Lost(dir) => anyhow::bail!(
            "the Link folder is missing at {}: use lnk folder set link <path>",
            dir.display()
        ),
    };
    link_files::create_private_dir(&dir)?;
    map::pin(home, machine)?;
    link_files::layout::prepare_at(machine, &dir)?;
    match find_in(home, machine)? {
        Found::Here(dir) => Ok(dir),
        Found::Lost(dir) => {
            anyhow::bail!("the Link folder moved while preparing {}", dir.display())
        }
    }
}

/// Find and remember a needed Link folder's identity on this machine.
pub fn find() -> anyhow::Result<Found> {
    find_in(&link_files::try_home_dir()?, &try_installation_dir()?)
}

fn find_in(home: &Path, machine: &Path) -> anyhow::Result<Found> {
    let mut settings = Settings::load_in(home, machine)?;
    let dir = link_dir_in(home, machine)?;
    match (marker_id(&dir), &settings.id) {
        (Some(id), Some(known)) if &id == known => Ok(Found::Here(dir)),
        (_, Some(_)) => Ok(Found::Lost(dir)),
        (Some(id), None) => {
            settings.id = Some(id);
            settings.save_in(home, machine)?;
            Ok(Found::Here(dir))
        }
        (None, None) if dir.is_dir() => {
            settings.id = Some(mark(&dir)?);
            settings.save_in(home, machine)?;
            Ok(Found::Here(dir))
        }
        (None, None) => Ok(Found::Here(dir)),
    }
}

/// Marks folder `dir` as a Link folder, returning its ID: a new one, or
/// the one already there. The first marker stays: several `lnk`
/// commands start at once on a fresh machine (an agent's start runs
/// its sandbox's), and each marking with an ID of its own would leave
/// the marker naming one and the settings another, a folder lost. A
/// marker is published atomically under the same settings lock used by
/// every first-use caller, so readers never see a partially written marker
/// or a temporary second hard link to it.
fn mark(dir: &Path) -> anyhow::Result<String> {
    let path = Home::Marker.at(dir);
    let lock = link_files::settings::lock(&path)?;
    if link_files::layout::read_marker(dir)?.is_some() {
        return marker_id(dir).within(format!("{} holds a marker Link can't read", dir.display()));
    }
    let id = link_files::random_token()?;
    lock.write(&Marker { id: id.clone() })
        .within(format!("marking {} as a Link folder", dir.display()))?;
    Ok(id)
}

/// Points this machine at the Link folder now at `dir`, which must hold
/// a marker and be in the home folder.
pub fn point_at(dir: &Path) -> anyhow::Result<PathBuf> {
    point_in(&link_files::try_home_dir()?, &try_installation_dir()?, dir)
}

fn point_in(home: &Path, machine: &Path, dir: &Path) -> anyhow::Result<PathBuf> {
    let dir = dir
        .canonicalize()
        .within(format!("finding {}", dir.display()))?;
    let id = marker_id(&dir).within(format!(
        "{} isn't a Link folder: it has no {}",
        dir.display(),
        Home::Marker.relative()
    ))?;
    anyhow::ensure!(
        dir.starts_with(home),
        "{} isn't in your home folder",
        dir.display()
    );
    let mut settings = Settings::load_at(machine, &dir)?;
    map::set(home, machine, Entry::Link, &dir)?;
    settings.id = Some(id);
    settings.save_in(home, machine)?;
    Ok(dir)
}

/// Makes a new, empty Link folder at the default place, `~/Link`, and
/// points this machine at it. Refused when something is there already.
pub fn make_new() -> anyhow::Result<PathBuf> {
    make_new_in(&link_files::try_home_dir()?, &try_installation_dir()?)
}

fn make_new_in(home: &Path, machine: &Path) -> anyhow::Result<PathBuf> {
    let dir = Folder::Home.default_in(home);
    anyhow::ensure!(
        !dir.exists() || only_empty_safe(&dir)?,
        "{} is there already: point Link at it with lnk folder move {}",
        dir.display(),
        dir.display()
    );
    link_files::create_private_dir(&dir)?;
    let id = mark(&dir)?;
    let mut settings = Settings::load_in(home, machine)?;
    let was = link_dir_in(home, machine)?;
    if was != dir {
        map::set(home, machine, Entry::Link, &dir)?;
    }
    settings.id = Some(id);
    settings.save_in(home, machine)?;
    Ok(dir)
}

/// Preparing a run can create the Safe. That empty protective skeleton
/// is still a new Link folder; other existing contents are never adopted.
fn only_empty_safe(dir: &Path) -> anyhow::Result<bool> {
    let mut entries = std::fs::read_dir(dir)?;
    let Some(entry) = entries.next().transpose()? else {
        return Ok(false);
    };
    Ok(entry.file_name() == LAYOUT.boundaries.home_safe.as_str()
        && entry.file_type()?.is_dir()
        && entries.next().transpose()?.is_none()
        && std::fs::read_dir(entry.path())?
            .next()
            .transpose()?
            .is_none())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resilience;
