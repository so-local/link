//! Trusted control paths are walked through handles, never mutable aliases.

use super::{Distrust, Trust, Untrusted};
use crate::nofollow::{Dir, Kind};
use anyhow::{Context, ensure};
use std::os::unix::fs::MetadataExt;
use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsString,
    fs::File,
    path::{Component, Path, PathBuf},
};

enum Step {
    Root,
    Parent,
    Name(OsString),
    Resolved(PathBuf),
}

fn steps(path: &Path) -> Vec<Step> {
    path.components()
        .filter_map(|part| match part {
            Component::RootDir => Some(Step::Root),
            Component::Normal(_) => Some(Step::Name(part.as_os_str().to_owned())),
            Component::ParentDir => Some(Step::Parent),
            _ => None,
        })
        .collect()
}

fn check(dir: &Dir, trust: Trust<'_>, folder: &Path, path: &Path) -> anyhow::Result<()> {
    let meta = dir.metadata()?;
    let why = if !trust.owners.is_empty() && !trust.owners.contains(&meta.uid()) {
        Distrust::Owner
    } else if meta.mode() & trust.deny_mode != 0 && meta.mode() & 0o1000 == 0 {
        Distrust::Written
    } else {
        return Ok(());
    };
    Err(Untrusted {
        file: path.to_path_buf(),
        folder: folder.to_path_buf(),
        owner: meta.uid(),
        why,
    }
    .into())
}

/// The roots a link above Link's folders may lead through: the user's
/// home as `HOME` names it (`/home` is `var/home` on Fedora Atomic, and
/// a network home is often linked in), and the system's roots.
fn roots() -> Vec<PathBuf> {
    use crate::layout::{SystemOs, SystemRoot};
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute());
    let system = [SystemRoot::Settings, SystemRoot::State, SystemRoot::Exec]
        .into_iter()
        .flat_map(|root| std::iter::once(root.path()).chain(root.aliases_for(SystemOs::here())));
    home.into_iter().chain(system).collect()
}

/// `path` with the root it's under ([`roots`]) resolved, once: what's
/// to the root is followed to where it really is, and the walk then
/// checks every folder on that real way, from `/`, and refuses any link
/// below the root, such as one in a Safe.
fn rooted(path: &Path) -> PathBuf {
    for root in roots() {
        if let Ok(rest) = path.strip_prefix(&root)
            && let Ok(real) = root.canonicalize()
        {
            return real.join(rest);
        }
    }
    path.to_path_buf()
}

pub(super) fn parent(
    path: &Path,
    trust: Trust<'_>,
    missing: bool,
) -> anyhow::Result<Option<(Dir, PathBuf)>> {
    let absolute = rooted(&std::path::absolute(path)?);
    let parent = absolute.parent().context("a settings file has a parent")?;
    let mut pending: VecDeque<_> = steps(parent).into();
    let mut active = BTreeSet::new();
    let mut dir = Dir::root_to_pass()?;
    let mut real = PathBuf::new();
    let mut ancestors = Vec::new();
    while let Some(step) = pending.pop_front() {
        match step {
            Step::Root => {
                dir = Dir::root_to_pass()?;
                real = PathBuf::from("/");
                ancestors.clear();
                check(&dir, trust, &real, path)?;
            }
            Step::Parent => {
                if let Some((parent, path)) = ancestors.pop() {
                    dir = parent;
                    real = path;
                }
            }
            Step::Resolved(alias) => {
                active.remove(&alias);
            }
            Step::Name(name) => {
                let next = match dir.passing(&name, false, 0) {
                    Ok(next) => next,
                    Err(error) => {
                        if error.kind() == std::io::ErrorKind::NotFound {
                            return if missing { Ok(None) } else { Err(error.into()) };
                        }
                        let stat = match dir.stat(&name) {
                            Ok(stat) => stat,
                            Err(error)
                                if missing && error.kind() == std::io::ErrorKind::NotFound =>
                            {
                                return Ok(None);
                            }
                            Err(error) => return Err(error.into()),
                        };
                        let alias = real.join(&name);
                        let parent = dir.metadata()?;
                        ensure!(
                            stat.kind == Kind::Link
                                && stat.uid == 0
                                && parent.uid() == 0
                                && parent.mode() & 0o022 == 0
                                && crate::layout::LAYOUT
                                    .trusted_read
                                    .system_links
                                    .contains(&alias),
                            "{} has a linked or invalid settings ancestor: {error}",
                            path.display()
                        );
                        ensure!(
                            active.insert(alias.clone()),
                            "a system settings alias contains a cycle"
                        );
                        let target = dir.read_link(&name)?;
                        pending.push_front(Step::Resolved(alias));
                        for step in steps(&target).into_iter().rev() {
                            pending.push_front(step);
                        }
                        continue;
                    }
                };
                check(&next, trust, &real.join(&name), path)?;
                ancestors.push((dir, real.clone()));
                real.push(&name);
                dir = next;
            }
        }
    }
    Ok(Some((dir, real)))
}

pub(super) fn open(path: &Path, trust: Trust<'_>) -> anyhow::Result<(File, PathBuf)> {
    let (dir, parent) = parent(path, trust, false)?.context("a settings parent exists")?;
    let name = path.file_name().context("a settings file has a name")?;
    let file = dir.open_file(name, libc::O_RDONLY, 0)?;
    ensure!(
        file.metadata()?.nlink() <= 1,
        "a control file has no other hard links"
    );
    Ok((file, parent.join(name)))
}

pub(super) fn folders(path: &Path, trust: Trust<'_>) -> anyhow::Result<()> {
    parent(path, trust, true)?;
    Ok(())
}
