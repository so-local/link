//! Import: an archive read and checked whole, then each thing named,
//! moved into place under that name and adopted by its owner as a new
//! thing. Nothing is installed, connected or started.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};

use crate::names::{self, Ask};
use crate::{Missing, Owners, Pick, Roots, Selection, StagedThing, ThingFolders, stage};

/// One thing imported.
#[derive(Debug)]
pub struct ImportedThing {
    /// As the archive named it.
    pub was: Selection,
    /// The name it took here.
    pub name: String,
    /// What it still needs here, each with its setup command.
    pub missing: Vec<Missing>,
    /// What its owner said of adopting it.
    pub notes: Vec<String>,
    /// The command that starts it.
    pub next: String,
}

/// What an import brought.
#[derive(Debug)]
pub struct Imported {
    pub things: Vec<ImportedThing>,
}

/// Imports archive `archive`: the things `picks` name, else all of them.
pub fn import(
    archive: &Path,
    roots: &Roots,
    picks: Option<&[Pick]>,
    owners: &dyn Owners,
    mut ask: Option<&mut dyn Ask>,
) -> anyhow::Result<Imported> {
    let staged = stage(archive, &roots.staging())?;
    let chosen: Vec<(&StagedThing, Option<&str>)> = match picks {
        None => staged.things.iter().map(|t| (t, None)).collect(),
        Some(picks) => picks
            .iter()
            .map(|pick| match staged.thing(&pick.thing) {
                Some(thing) => Ok((thing, pick.to.as_deref())),
                None => bail!("{} isn't in {}", pick.thing, archive.display()),
            })
            .collect::<anyhow::Result<_>>()?,
    };
    // Every thing checked before anything is asked or moved.
    for (thing, _) in &chosen {
        let Some(owner) = owners.of(thing.selection.kind()) else {
            bail!(
                "no module here imports {}s, so {} can't be imported",
                thing.selection.kind(),
                thing.selection
            );
        };
        owner.check(&staged_folders(thing)).with_context(|| {
            format!(
                "{} in {} can't be imported",
                thing.selection,
                archive.display()
            )
        })?;
    }
    let mut names: Vec<(Selection, String)> = Vec::new();
    for (thing, given) in &chosen {
        let kind = thing.selection.kind();
        let reserved = link_files::layout::reserved(&thing.owner);
        let taken = |name: &str| {
            reserved.contains(name)
                || roots.taken(&thing.owner, name)
                || names.iter().any(|(s, n)| s.kind() == kind && n == name)
        };
        let name = names::choose(
            kind,
            thing.selection.name(),
            *given,
            &taken,
            ask.as_mut().map(|a| &mut **a as &mut dyn Ask),
        )?;
        names.push((thing.selection.clone(), name));
    }
    let mut things: Vec<ImportedThing> = Vec::new();
    // What's imported stays when a later thing fails, and the error says
    // which.
    let so_far = |things: &[ImportedThing], err: anyhow::Error| match things {
        [] => err,
        done => err.context(format!(
            "imported {} before it stopped",
            done.iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    };
    for ((thing, _), (_, name)) in chosen.iter().zip(&names) {
        let owner = owners.of(thing.selection.kind()).expect("checked above");
        let placed = place(roots, thing, name).map_err(|e| so_far(&things, e))?;
        let folders = ThingFolders {
            kind: thing.selection.kind().into(),
            name: name.clone(),
            folders: placed.clone(),
        };
        let adopted = match owner.adopt(&folders, thing.selection.name()) {
            Ok(adopted) => adopted,
            Err(err) => {
                for (_, folder) in &placed {
                    let _ = std::fs::remove_dir_all(folder);
                }
                return Err(so_far(
                    &things,
                    err.context(format!("{} wasn't imported", thing.selection)),
                ));
            }
        };
        let missing = owner.missing(&folders).unwrap_or_else(|err| {
            vec![Missing {
                what: format!("What it needs couldn't be read: {err:#}"),
                setup: adopted.next.clone(),
            }]
        });
        things.push(ImportedThing {
            was: thing.selection.clone(),
            name: name.clone(),
            missing,
            notes: adopted.notes,
            next: adopted.next,
        });
    }
    Ok(Imported { things })
}

fn staged_folders(thing: &StagedThing) -> ThingFolders {
    ThingFolders {
        kind: thing.selection.kind().into(),
        name: thing.selection.name().into(),
        folders: thing
            .folders
            .iter()
            .map(|f| (f.home, f.staged.clone()))
            .collect(),
    }
}

/// Moves staged `thing`'s folders to their places here under `name`, all
/// or none: a folder already there refuses it, and what moved is moved
/// back.
fn place(
    roots: &Roots,
    thing: &StagedThing,
    name: &str,
) -> anyhow::Result<Vec<(link_files::layout::Home, PathBuf)>> {
    let mut placed: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut out = Vec::new();
    for folder in &thing.folders {
        let to = roots.folder(folder.home, name);
        let moved = (|| -> anyhow::Result<()> {
            ensure!(
                to.symlink_metadata().is_err(),
                "{} is there already: nothing is ever overwritten or merged",
                to.display()
            );
            if let Some(parent) = to.parent() {
                crate::reading::make_dirs(parent)?;
            }
            move_tree(&folder.staged, &to)
        })();
        if let Err(err) = moved {
            for (from, to) in placed.iter().rev() {
                let _ = move_tree(to, from);
            }
            return Err(err);
        }
        placed.push((folder.staged.clone(), to.clone()));
        out.push((folder.home, to));
    }
    Ok(out)
}

/// Moves folder `from` to `to`, copying it when they're on two disks.
fn move_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(libc::EXDEV) => {
            // Copied beside its place under a name of Link's own, then
            // renamed into it: never a folder copied in part in its place.
            let beside = to.with_file_name(link_files::layout::temporary_name());
            let copied = copy_tree(from, &beside).and_then(|()| {
                std::fs::rename(&beside, to)
                    .with_context(|| format!("moving {} to {}", beside.display(), to.display()))
            });
            if let Err(err) = copied {
                let _ = std::fs::remove_dir_all(&beside);
                return Err(err);
            }
            std::fs::remove_dir_all(from).with_context(|| format!("removing {}", from.display()))
        }
        Err(err) => {
            Err(err).with_context(|| format!("moving {} to {}", from.display(), to.display()))
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> anyhow::Result<()> {
    crate::reading::make_dirs(to)?;
    for entry in std::fs::read_dir(from).with_context(|| format!("reading {}", from.display()))? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying to {}", target.display()))?;
        } else {
            bail!("{} isn't a plain file", entry.path().display());
        }
    }
    Ok(())
}
