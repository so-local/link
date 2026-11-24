//! The local side: the folders Link backs up, the paths users type, and
//! the files under them. Each folder has its own bucket in every cloud:
//! your files (`~/Link/Files`) and each owner's, a folder another
//! module says is its own ([`FOLDERS`]), named by the owner's ID.
//! Paths inside a folder are always relative and `/`-separated
//! (`photos/2019/a.jpg`), the same in its bucket.

use std::path::{Component, Path, PathBuf};

use crate::said;
use anyhow::bail;
use link_format::{Foreign, WithinExt, sentence};
use link_storage_contract::{FOLDERS, Folders, Owned};
pub use link_storage_contract::{Owner, OwnerKind};

/// Your files.
pub const PERSONAL: &str = "personal";

/// The longest an owner's name-safe form ([`safe`]) is: a label's
/// value, as every cloud takes it.
pub const SAFE_MAX: usize = 63;

/// Whether `owner` is one `lnk storage` can name a folder by: its ID's
/// id lowercase letters and digits, so its name-safe form ([`safe`]) is
/// one way back, and that form fits a label.
pub fn check_owner(owner: &Owner) -> anyhow::Result<()> {
    let id = owner.id();
    let plain = id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && safe(owner).len() <= SAFE_MAX;
    anyhow::ensure!(
        plain,
        "{:?} isn't an owner lnk storage backs up: its ID's id is lowercase letters and digits, \
         and with its kind at most {SAFE_MAX} long",
        link_format::clean(owner)
    );
    Ok(())
}

/// `text` as an owner's ID `lnk storage` can name a folder by
/// ([`check_owner`]).
pub fn parse_owner(text: &str) -> anyhow::Result<Owner> {
    let owner: Owner = text.parse().map_err(|err| {
        anyhow::anyhow!(
            "{:?} isn't an owner's ID, <kind>:<id>: {err}",
            link_format::clean(text)
        )
    })?;
    check_owner(&owner)?;
    Ok(owner)
}

/// The one name-safe form of a folder's key ([`Folder::key`]), for what
/// can't hold a `:`: an rclone section, a file's name, a folder's name in
/// a cloud, a cloud only for one owner and a bucket's label. Your files
/// are [`PERSONAL`]; an owner's are `<kind>-<id>` (`job-k3v9q2x7m1ab`),
/// read back by [`from_safe`].
pub fn safe(key: &str) -> String {
    key.replacen(':', "-", 1)
}

/// The owner whose name-safe form ([`safe`]) is `text`, if it's one.
pub fn from_safe(text: &str) -> Option<Owner> {
    let (kind, id) = text.rsplit_once('-')?;
    parse_owner(&format!("{kind}:{id}")).ok()
}

/// An owner whose folder Link backs up.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderOwner {
    pub id: Owner,
    pub name: String,
}

/// A folder Link backs up, with its own bucket in every cloud.
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    /// [`PERSONAL`], or the owner's ID: its bucket in each cloud, and
    /// its manifest, are by it ([`safe`] where a name can't hold it).
    pub id: String,
    pub root: PathBuf,
    /// The owner whose folder it is, if it is.
    pub owner: Option<FolderOwner>,
    /// Whether that owner runs.
    pub running: Running,
    /// The cloud account that keeps it, when one does: it's there,
    /// reached through the module that answers [`link_storage_contract::HOME`]
    /// ([`crate::hosted`]), and this folder is where it's pushed from and
    /// pulled to.
    pub hosted: Option<String>,
}

impl Folder {
    /// Your files, in `root`.
    pub fn personal(root: PathBuf) -> Self {
        Folder {
            id: PERSONAL.into(),
            root,
            owner: None,
            running: Running::No,
            hosted: None,
        }
    }

    /// Owner `id` (named `name`)'s folder, in `root`.
    pub fn of_owner(id: &Owner, name: &str, root: PathBuf, running: Running) -> Self {
        Folder {
            id: id.to_string(),
            root,
            owner: Some(FolderOwner {
                id: id.clone(),
                name: name.into(),
            }),
            running,
            hosted: None,
        }
    }

    /// Owner `id` (named `name`)'s folder kept on cloud account
    /// `account`, pushed from and pulled to `root` here.
    pub fn of_hosted(id: &Owner, name: &str, root: PathBuf, account: &str) -> Self {
        Folder {
            hosted: Some(account.into()),
            ..Folder::of_owner(id, name, root, Running::No)
        }
    }

    /// What it holds, for people.
    pub fn label(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{}'s files", owner.name),
            None => "your files".into(),
        }
    }

    pub fn is_owned(&self) -> bool {
        self.owner.is_some()
    }
}

/// The owner whose folder's key is `key` (an owner's ID), named by
/// that ID: for a command that reads only `clouds.toml`, which keys each
/// bucket by its folder's key and keeps no owner's name. None for your
/// files.
pub fn owner_of(key: &str) -> Option<FolderOwner> {
    let id = parse_owner(key).ok()?;
    Some(FolderOwner {
        name: id.to_string(),
        id,
    })
}

/// Each owner's name, by its ID, as the modules that answer [`FOLDERS`]
/// say: a command that reads `clouds.toml` names owners by these. An
/// owner whose module didn't answer, or that isn't here, is left out.
pub fn names() -> std::collections::BTreeMap<String, String> {
    names_of(&asked().folders)
}

/// Of `asked`, each plain owner's name, by its ID.
pub fn names_of(asked: &Folders) -> std::collections::BTreeMap<String, String> {
    asked
        .owned
        .iter()
        .filter(|o| plain(o))
        .map(|o| (o.owner.to_string(), o.name.clone()))
        .collect()
}

/// The owner whose folder's key is `key`, named as `names` says, else
/// by its ID ([`owner_of`]).
pub fn owner_in(
    key: &str,
    names: &std::collections::BTreeMap<String, String>,
) -> Option<FolderOwner> {
    let mut owner = owner_of(key)?;
    if let Some(name) = names.get(key) {
        owner.name = name.clone();
    }
    Some(owner)
}

/// What the folder whose key is `key` holds, for people, as
/// [`Folder::label`] says it, an owner named by its ID ([`owner_of`]).
pub fn label_of(key: &str) -> String {
    match (key == PERSONAL, owner_of(key)) {
        (true, _) => "your files".into(),
        (false, Some(owner)) => format!("{}'s files", owner.name),
        (false, None) => format!("{}'s files", link_format::clean(key)),
    }
}

/// Your files: `LNK_PERSONAL_DIR`, else `~/Link/Files`.
pub fn personal() -> anyhow::Result<Folder> {
    let root = match std::env::var_os("LNK_PERSONAL_DIR").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(_) => bail!("LNK_PERSONAL_DIR must be an absolute path"),
        None => link_files::layout::Home::Files.try_path()?,
    };
    Ok(Folder::personal(root))
}

/// Whether an owner is running, as far as Link can tell.
#[derive(Debug, Clone, PartialEq)]
pub enum Running {
    No,
    Yes,
    /// The module that keeps it couldn't say (why): treated as running
    /// wherever that matters.
    Unknown(String),
}

impl Running {
    /// Why `what` can't happen in the owner's folder now, if it can't.
    pub fn refuse(&self, what: &str) -> anyhow::Result<()> {
        match self {
            Running::No => Ok(()),
            Running::Yes => bail!(
                "its owner is running, and could change its files while {what}: stop it first"
            ),
            Running::Unknown(why) => bail!(
                "couldn't tell whether its owner is running ({why}), so not {what} in its folder"
            ),
        }
    }
}

/// This machine's owners' folders, as the modules that answer
/// [`FOLDERS`] say: each owner with a usable folder here, not kept on a
/// cloud account. None without one. When one doesn't answer, an error:
/// only for a command that must find an owner among them.
pub fn owned() -> anyhow::Result<Vec<Folder>> {
    let asked = asked();
    if let Some(unanswered) = asked.unanswered.first() {
        bail!(
            "couldn't list the folders the {} module keeps: {}",
            unanswered.module,
            unanswered.why
        );
    }
    Ok(of_owned(&asked.folders))
}

/// A module that answers [`FOLDERS`] but didn't, and why: its folders
/// are left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Unanswered {
    pub module: String,
    pub why: String,
}

/// What the modules that answer [`FOLDERS`] said, together, and those
/// that didn't answer.
#[derive(Debug, Default)]
pub struct Asked {
    pub folders: Folders,
    pub unanswered: Vec<Unanswered>,
}

/// The folders a command that touches files works with, and the modules
/// whose folders are left out of them because they didn't answer.
#[derive(Debug)]
pub struct Found {
    pub folders: Vec<Folder>,
    pub unanswered: Vec<Unanswered>,
}

/// Asked of every module that answers [`FOLDERS`]: none without one.
fn asked() -> Asked {
    let mut asked = Asked::default();
    for answer in FOLDERS.ask(&()) {
        match answer.reply {
            Ok(folders) => asked.folders.owned.extend(folders.owned),
            Err(err) => asked.unanswered.push(Unanswered {
                module: answer.module,
                why: format!("{err:#}"),
            }),
        }
    }
    asked
}

/// Of `asked`, the owners' folders here whose ID, name and folder are
/// plain, but those kept on a cloud account.
pub fn of_owned(asked: &Folders) -> Vec<Folder> {
    asked
        .owned
        .iter()
        .filter(|o| o.account.is_none() && plain(o))
        .map(|o| {
            let running = match o.running {
                Some(true) => Running::Yes,
                Some(false) => Running::No,
                None => Running::Unknown("the module that keeps it didn't say".into()),
            };
            Folder::of_owner(&o.owner, &o.name, o.root.clone(), running)
        })
        .collect()
}

/// Whether `owned`'s ID is one `lnk storage` names folders by, its name
/// a name, its folder absolute, and any account a name.
fn plain(owned: &Owned) -> bool {
    check_owner(&owned.owner).is_ok()
        && link_machine::machine::check_name(&owned.name).is_ok()
        && owned.root.is_absolute()
        && owned
            .account
            .as_deref()
            .is_none_or(|a| link_machine::machine::check_shape(a).is_ok())
}

/// [`folders`], and each owned folder kept on a cloud account
/// ([`of_hosted`]): what `lnk storage push` and `pull` reach. No other
/// command of `lnk storage` takes one: it isn't in a bucket of yours.
pub fn folders_with_hosted() -> anyhow::Result<Found> {
    let asked = asked();
    let mut folders = folders_of(&asked.folders)?;
    let hosted = of_hosted(&asked.folders, &folders);
    folders.extend(hosted);
    Ok(Found {
        folders,
        unanswered: asked.unanswered,
    })
}

/// The owned folders kept on a cloud account, as `asked` says, but
/// those of an owner with a folder of `here`, by ID or by kind and name:
/// each one's folder here. One that names what isn't plain is left out.
pub fn of_hosted(asked: &Folders, here: &[Folder]) -> Vec<Folder> {
    asked
        .owned
        .iter()
        .filter(|o| plain(o))
        .filter_map(|o| {
            let account = o.account.as_deref()?;
            Some(Folder::of_hosted(
                &o.owner,
                &o.name,
                o.root.clone(),
                account,
            ))
        })
        .filter(|hosted| {
            let owner = hosted.owner.as_ref();
            !here.iter().any(|f| {
                f.owner.as_ref().is_some_and(|a| {
                    owner.is_some_and(|h| {
                        a.id == h.id || (a.id.kind() == h.id.kind() && a.name == h.name)
                    })
                })
            })
        })
        .collect()
}

/// Whether the owner whose folder's key is `key` is running now, asked
/// again: unknown when a module that answers [`FOLDERS`] doesn't, which
/// refuses as running does.
pub fn running(key: &str) -> anyhow::Result<Running> {
    for answer in FOLDERS.ask(&()) {
        let Ok(folders) = answer.reply else {
            return Ok(Running::Unknown(format!(
                "the {} module didn't answer",
                answer.module
            )));
        };
        if let Some(found) = of_owned(&folders).into_iter().find(|f| f.id == key) {
            return Ok(found.running);
        }
    }
    Ok(Running::No)
}

/// Your files, then each owner's, after [`check_folders`]: for a
/// command that touches files. A module that doesn't answer [`FOLDERS`]
/// doesn't stop it: its folders are left out, so a path in one is in
/// none of Link's folders and refused, and the module is in
/// [`Found::unanswered`] for the command to warn of.
pub fn folders() -> anyhow::Result<Found> {
    let asked = asked();
    Ok(Found {
        folders: folders_of(&asked.folders)?,
        unanswered: asked.unanswered,
    })
}

/// [`folders`], the owners' as `asked` says.
fn folders_of(asked: &Folders) -> anyhow::Result<Vec<Folder>> {
    let mut folders = vec![personal()?];
    folders.extend(of_owned(asked));
    check_folders(&folders, &link_files::try_home_dir()?, &config_dir()?)?;
    Ok(folders)
}

/// `path` as written and, where it exists, with links resolved.
fn forms(path: &Path) -> Vec<PathBuf> {
    let mut forms = vec![normalize(path)];
    if let Ok(real) = path.canonicalize() {
        forms.push(real);
    }
    forms
}

/// Whether `a` and `b` are one inside the other (or the same), in any of
/// their forms.
pub fn overlap(a: &Path, b: &Path) -> bool {
    let (a, b) = (forms(a), forms(b));
    a.iter()
        .any(|a| b.iter().any(|b| a.starts_with(b) || b.starts_with(a)))
}

/// Refuses folders Link can't back up safely: one inside another (files
/// would go to the wrong bucket, and past its owner's checks), or one
/// that is or holds your home folder or Link's settings (`home`,
/// `config`).
pub fn check_folders(folders: &[Folder], home: &Path, config: &Path) -> anyhow::Result<()> {
    for (i, a) in folders.iter().enumerate() {
        for b in &folders[i + 1..] {
            if overlap(&a.root, &b.root) {
                bail!(
                    "{} ({}) and {} ({}) are one inside the other; each needs a folder of its own",
                    a.label(),
                    a.root.display(),
                    b.label(),
                    b.root.display()
                );
            }
        }
        let holds = |inner: &Path| {
            forms(inner)
                .iter()
                .any(|inner| forms(&a.root).iter().any(|root| inner.starts_with(root)))
        };
        if holds(home) || holds(config) {
            bail!(
                "{} is {}, which holds your home folder or Link's settings: not a folder Link backs up",
                a.label(),
                a.root.display()
            );
        }
    }
    Ok(())
}

/// Held while a command changes which clouds and buckets there are, or
/// their keys, so two can't lose each other's changes (a password Link
/// just made among them).
pub fn config_lock() -> anyhow::Result<std::fs::File> {
    let dir = config_dir()?;
    link_files::create_private_dir(&dir)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let lock = options.open(dir.join("lock-config"))?;
    if lock.try_lock().is_err() {
        said::note(sentence!(
            "Waiting for another lnk storage command to finish."
        ));
        lock.lock()?;
    }
    Ok(lock)
}

/// The module's settings, keys and engine: `~/.lnk/safe/storage`.
pub fn config_dir() -> anyhow::Result<PathBuf> {
    link_files::layout::Hidden::Storage.try_path()
}

/// Names a folder keeps for itself, never pushed: `.link` at the top
/// (Link's own objects in a cloud) and Finder's `.DS_Store`.
fn skipped(relative: &str) -> bool {
    relative == ".link" || relative.starts_with(".link/") || relative.ends_with(".DS_Store")
}

/// What the user typed, as a folder (its index in `folders`) and a path
/// inside it (`""` is all of it). Relative paths are from the current
/// folder `cwd` when it's inside one of them, else from the first (your
/// files). The folder is the one the path ends up in.
pub fn locate(folders: &[Folder], cwd: &Path, arg: &str) -> anyhow::Result<(usize, String)> {
    let deepest = |path: &Path| {
        folders
            .iter()
            .enumerate()
            .filter(|(_, f)| path.starts_with(normalize(&f.root)))
            .max_by_key(|(_, f)| f.root.components().count())
            .map(|(i, _)| i)
    };
    let path = Path::new(arg);
    let cwd = normalize(cwd);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else if deepest(&cwd).is_some() {
        cwd.join(path)
    } else {
        folders[0].root.join(path)
    };
    let full = normalize(&full);
    let Some(index) = deepest(&full) else {
        let roots: Vec<String> = folders
            .iter()
            .map(|f| format!("{} ({})", f.root.display(), f.label()))
            .collect();
        bail!(
            "{arg} is in none of Link's folders, {}: move it into one first",
            roots.join(" or ")
        );
    };
    let inside = full
        .strip_prefix(normalize(&folders[index].root))
        .expect("checked above");
    let relative = inside
        .components()
        .map(|c| c.as_os_str().to_str().within("paths must be UTF-8"))
        .collect::<anyhow::Result<Vec<_>>>()?
        .join("/");
    if skipped(&relative) {
        bail!("{arg} is Link's own, not something to push");
    }
    Ok((index, relative))
}

/// `path` without `.` and with `..` applied, without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// A file in the bucket, here.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    pub path: String,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
    /// Which file it is, exactly: a file replaced or rewritten in the
    /// same second is a different one.
    pub stamp: Stamp,
}

/// A file's identity: its inode, and its modification and change times
/// in nanoseconds (the change time, which no program sets, moves with
/// any write, a modification time put back included).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stamp {
    pub inode: u64,
    pub nanos: u128,
    pub changed: u128,
}

impl Stamp {
    pub fn of(meta: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        let inode = std::os::unix::fs::MetadataExt::ino(meta);
        #[cfg(not(unix))]
        let inode = 0;
        let nanos = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        #[cfg(unix)]
        let changed = {
            use std::os::unix::fs::MetadataExt;
            u128::try_from(meta.ctime()).unwrap_or(0) * 1_000_000_000
                + u128::try_from(meta.ctime_nsec()).unwrap_or(0)
        };
        #[cfg(not(unix))]
        let changed = 0;
        Stamp {
            inode,
            nanos,
            changed,
        }
    }
}

/// Whether rclone would store `name` under another name: control
/// characters, and the look-alikes rclone itself uses for them and for
/// `.` and `/`. Two such files could be taken for one another, so they
/// aren't backed up.
pub fn reencoded(name: &str) -> bool {
    name.chars().any(|c| {
        c.is_control()
            || ('\u{2400}'..='\u{2421}').contains(&c)
            || matches!(c, '\u{201B}' | '\u{FF0E}' | '\u{FF0F}')
    })
}

/// Whether `path` (from a cloud's listing) stays inside a folder:
/// relative, with no empty, `.` or `..` part.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// Whether every folder on the way from `root` to `relative` (not its
/// last part) is a real folder, or not there yet. A link among them
/// would lead outside `root`.
pub fn parents_are_real(root: &Path, relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
    let mut path = root.to_path_buf();
    for part in &parts[..parts.len().saturating_sub(1)] {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
    }
    true
}

/// Every file here under `relative` (a file or folder in the bucket),
/// sorted. Symlinks are skipped, with a warning: the bucket holds files.
pub fn walk(root: &Path, relative: &str) -> anyhow::Result<Vec<Local>> {
    let mut found = Vec::new();
    let start = if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    if !parents_are_real(root, relative) {
        bail!("{relative}: a folder on the way to it is a link, which leads outside the folder");
    }
    match std::fs::symlink_metadata(&start) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(e).within(format!("reading {}", start.display())),
        Ok(_) => {}
    }
    visit(&start, relative, &mut found)?;
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

fn visit(path: &Path, relative: &str, found: &mut Vec<Local>) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(path).within(format!("reading {}", path.display()))?;
    if skipped(relative) {
        return Ok(());
    }
    // Checked on every name, including one given directly.
    if relative.rsplit('/').next().is_some_and(reencoded) {
        link_format::aside(link_format::Report::warning(sentence!(
            "Skipping {}, since its name has characters clouds store differently.",
            Foreign::new(&path.display().to_string())
        )));
        return Ok(());
    }
    if meta.file_type().is_symlink() {
        link_format::aside(link_format::Report::warning(sentence!(
            "Skipping {}, since it's a link, not a file.",
            Foreign::new(relative)
        )));
    } else if meta.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                link_format::aside(link_format::Report::warning(sentence!(
                    "Skipping {}, since its name isn't UTF-8.",
                    Foreign::new(&entry.path().display().to_string())
                )));
                continue;
            };

            let child = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            visit(&entry.path(), &child, found)?;
        }
    } else if meta.is_file() {
        found.push(Local {
            path: relative.to_string(),
            size: meta.len(),
            modified: seconds(meta.modified().ok()),
            stamp: Stamp::of(&meta),
        });
    }
    Ok(())
}

/// Seconds since 1970, 0 when unknown.
pub fn seconds(time: Option<std::time::SystemTime>) -> u64 {
    time.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

pub fn now() -> u64 {
    seconds(Some(std::time::SystemTime::now()))
}

/// Nanoseconds since 1970, for names that mustn't collide.
pub fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Whether `path` (in the bucket) is `pattern` or inside it.
pub use link_files::pattern::matches;

/// A size for people: `2.4 MB`.
pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Deletes the folders the deleted `files` (paths under `root`) leave
/// empty, deepest first, up to `root` (never `root` itself). Only real
/// folders: a link is left alone.
pub fn remove_empty_parents(root: &Path, files: &[&str]) {
    let mut dirs: Vec<String> = Vec::new();
    for file in files {
        let mut parts: Vec<&str> = file.split('/').collect();
        parts.pop();
        while !parts.is_empty() {
            let dir = parts.join("/");
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
            parts.pop();
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    for dir in dirs {
        let path = root.join(&dir);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            // Fails, harmlessly, unless it's empty.
            let _ = std::fs::remove_dir(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An owner a module named is named so; one no module named keeps
    /// its ID, and your files have no owner.
    #[test]
    fn an_owner_is_named_as_its_module_says() {
        let names = std::collections::BTreeMap::from([(OWNER.to_string(), "main".to_string())]);
        assert_eq!(owner_in(OWNER, &names).unwrap().name, "main");
        assert_eq!(owner_in(OWNER, &Default::default()).unwrap().name, OWNER);
        assert_eq!(owner_in(PERSONAL, &names), None);
    }

    #[test]
    fn locates_paths_in_the_right_folder() {
        let folders = [
            Folder::personal("/home/me/Link/Files".into()),
            Folder::of_owner(
                &OWNER.parse().unwrap(),
                "main",
                "/home/me/Link/Jobs/build".into(),
                Running::No,
            ),
        ];
        let at = |cwd: &str, arg: &str| locate(&folders, Path::new(cwd), arg);
        let ok = |i: usize, p: &str| (i, p.to_string());
        // Outside every folder: your files.
        assert_eq!(at("/tmp", "photos/2019").unwrap(), ok(0, "photos/2019"));
        assert_eq!(at("/tmp", "./photos/").unwrap(), ok(0, "photos"));
        assert_eq!(at("/tmp", "").unwrap(), ok(0, ""));
        assert_eq!(at("/tmp", ".").unwrap(), ok(0, ""));
        // An absolute path picks its folder.
        assert_eq!(
            at("/tmp", "/home/me/Link/Files/a/b.txt").unwrap(),
            ok(0, "a/b.txt")
        );
        assert_eq!(
            at("/tmp", "/home/me/Link/Jobs/build/notes.md").unwrap(),
            ok(1, "notes.md")
        );
        // Inside a folder: relative to where you are, in that folder.
        assert_eq!(
            at("/home/me/Link/Files/photos", "2019").unwrap(),
            ok(0, "photos/2019")
        );
        assert_eq!(
            at("/home/me/Link/Files/photos", "../docs").unwrap(),
            ok(0, "docs")
        );
        assert_eq!(
            at("/home/me/Link/Jobs/build", "notes").unwrap(),
            ok(1, "notes")
        );
        assert_eq!(
            at("/home/me/Link/Jobs/build", "../../Files/x").unwrap(),
            ok(0, "x")
        );
        // Anywhere else is refused.
        assert!(at("/tmp", "../secrets").is_err());
        assert!(at("/tmp", "/etc/passwd").is_err());
        assert!(at("/home/me/Link/Jobs/build", "..").is_err());
        assert!(at("/home/me/Link/Files/photos", "../../x").is_err());
        assert!(at("/tmp", ".link/x").is_err());
    }

    #[test]
    fn walks_files_and_skips_links_own() {
        let root = link_test_kit::TempDir::new("bucket-walk");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join(".link")).unwrap();
        std::fs::write(root.join("a/b/c.txt"), "hello").unwrap();
        std::fs::write(root.join("a/.DS_Store"), "x").unwrap();
        std::fs::write(root.join(".link/x"), "x").unwrap();
        std::fs::write(root.join("top.txt"), "hi").unwrap();
        let all: Vec<_> = walk(&root, "")
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(all, ["a/b/c.txt", "top.txt"]);
        let one = walk(&root, "a/b/c.txt").unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].size, 5);
        assert!(walk(&root, "missing").unwrap().is_empty());
        // Only the folders a deleted file leaves empty go.
        std::fs::create_dir_all(root.join("empty-already")).unwrap();
        remove_empty_parents(&root, &["a/b/c.txt"]);
        assert!(root.join("a/b").is_dir());
        std::fs::remove_file(root.join("a/b/c.txt")).unwrap();
        std::fs::remove_file(root.join("a/.DS_Store")).unwrap();
        remove_empty_parents(&root, &["a/b/c.txt"]);
        assert!(!root.join("a").exists());
        assert!(root.join("empty-already").is_dir());
        assert!(root.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn links_and_odd_names_stay_out() {
        let root = link_test_kit::TempDir::new("bucket-links");
        let outside = link_test_kit::TempDir::new("bucket-links-outside");
        std::fs::create_dir_all(outside.join("sub")).unwrap();
        std::fs::write(outside.join("sub/secret"), "s").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("x")).unwrap();
        // A folder on the way that's a link leads outside: refused.
        assert!(!parents_are_real(&root, "x/sub/secret"));
        assert!(walk(&root, "x/sub").is_err());
        assert!(parents_are_real(&root, "y/new/file"));
        assert!(parents_are_real(&root, "top"));
        // Names rclone stores differently are skipped.
        std::fs::write(root.join("a\u{1}b"), "control").unwrap();
        std::fs::write(root.join("a\u{2401}b"), "look-alike").unwrap();
        std::fs::write(root.join("fine name.txt"), "ok").unwrap();
        let all: Vec<_> = walk(&root, "")
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(all, ["fine name.txt"]);
        assert!(reencoded("a\nb"));
        assert!(reencoded("x\u{FF0E}"));
        assert!(!reencoded("photos/2019 été.jpg"));
    }

    const OWNER: &str = "job:a1b2c3d4e5f6";

    /// An owner's name-safe form is one way back to it, and an owner
    /// whose ID can't be read back from it is refused.
    #[test]
    fn an_owners_name_safe_form_reads_back() {
        for owner in [
            "job:k3v9q2x7m1ab",
            "group:t4m9q2x7k1ab",
            "my-kind:a1b2c3d4e5f6",
        ] {
            let parsed = parse_owner(owner).unwrap();
            let safe = safe(owner);
            assert!(!safe.contains(':'), "{safe}");
            assert_eq!(from_safe(&safe), Some(parsed));
        }
        assert_eq!(safe("job:k3v9q2x7m1ab"), "job-k3v9q2x7m1ab");
        assert_eq!(safe(PERSONAL), PERSONAL);
        assert_eq!(from_safe(PERSONAL), None);
        for refused in [
            "job:K3V9",
            "job:a-b",
            "job:a.b",
            "job",
            "Job:a1",
            "my-kind:a1",
        ] {
            assert!(parse_owner(refused).is_err(), "{refused}");
        }
        assert!(parse_owner(&format!("job:{}", "a".repeat(SAFE_MAX))).is_err());
    }

    #[test]
    fn folders_must_stay_apart() {
        let home = Path::new("/home/me");
        let config = Path::new("/home/me/.lnk/safe/storage");
        let folder = |name: &str, root: &str| match name {
            PERSONAL => Folder::personal(root.into()),
            id => Folder::of_owner(&id.parse().unwrap(), "main", root.into(), Running::No),
        };
        let ok = [
            folder(PERSONAL, "/home/me/Link/Files"),
            folder(OWNER, "/home/me/Link/Jobs/build"),
        ];
        assert!(check_folders(&ok, home, config).is_ok());
        for bad in [
            [
                folder(PERSONAL, "/home/me/Link"),
                folder(OWNER, "/home/me/Link/Jobs/build"),
            ],
            [
                folder(PERSONAL, "/home/me/Link/Files"),
                folder(OWNER, "/home/me/Link/Files/job"),
            ],
            [
                folder(PERSONAL, "/home/me/Link/Files"),
                folder(OWNER, "/home/me"),
            ],
            [
                folder(PERSONAL, "/home/me/Link/Files"),
                folder(OWNER, "/home/me/.lnk"),
            ],
            [
                folder(PERSONAL, "/"),
                folder(OWNER, "/home/me/Link/Jobs/build"),
            ],
        ] {
            assert!(check_folders(&bad, home, config).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(human(3), "3 B");
        assert_eq!(human(2_400_000), "2.4 MB");
        assert_eq!(human(1_500_000_000_000), "1.5 TB");
    }
}
