//! Export: the things you select written to one archive, each thing's
//! folders under `~/Link` copied whole, laid out at the default paths a
//! new machine has, whatever this machine's map says. A file its owner
//! carries in place of its own ([`crate::Owner::carry`]) goes instead,
//! and one it leaves out stays out, so the archive holds nothing of this
//! machine's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use link_files::layout::{Home, map::Entry};

use crate::{Manifest, Owners, Replaced, Roots, Selection, Thing, ThingFolders, Writer};

/// What an export wrote.
#[derive(Debug)]
pub struct Exported {
    pub path: PathBuf,
    /// Each thing and the bytes of its files.
    pub things: Vec<(Selection, u64)>,
}

/// Where an export of `things` goes when no file is named:
/// `~/Link/Archive/<name>.tar.gz` for one, `<date>.tar.gz` for several.
pub fn default_name(roots: &Roots, things: &[Selection], today: &str) -> PathBuf {
    let base = match things {
        [one] => one.name().to_string(),
        _ => today.to_string(),
    };
    roots.archive().join(format!("{base}.tar.gz"))
}

/// Every thing here of every kind an owner here exports, sorted.
pub fn everything(roots: &Roots, owners: &dyn Owners) -> anyhow::Result<Vec<Selection>> {
    let mut found = Vec::new();
    for (word, owner) in link_files::layout::owners() {
        let Some(kind) = &owner.kind else { continue };
        if owners.of(kind).is_none() {
            continue;
        }
        let mut names = std::collections::BTreeSet::new();
        for (_, home) in link_files::layout::state_locations(word) {
            let dir = home.at(&roots.link);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|t| t.is_dir())
                    && link_machine::machine::check_name(&name).is_ok()
                    && !link_files::layout::reserved(word).contains(&name)
                {
                    names.insert(name);
                }
            }
        }
        for name in names {
            found.push(Selection::new(kind, &name)?);
        }
    }
    Ok(found)
}

/// Thing `selection`'s folders here, each there as a folder.
fn here(roots: &Roots, selection: &Selection) -> anyhow::Result<(ThingFolders, Vec<Home>)> {
    let Some((word, _)) = link_files::layout::owner_of_kind(selection.kind()) else {
        bail!(
            "{selection} is a {}, which no kind here is",
            selection.kind()
        );
    };
    let mut folders = Vec::new();
    let mut homes = Vec::new();
    for (_, home) in link_files::layout::state_locations(word) {
        let folder = roots.folder(home, selection.name());
        let is_dir = folder.symlink_metadata().is_ok_and(|m| m.is_dir());
        ensure!(
            is_dir,
            "{selection} isn't here: {} isn't a folder",
            folder.display()
        );
        folders.push((home, folder));
        homes.push(home);
    }
    Ok((
        ThingFolders {
            kind: selection.kind().into(),
            name: selection.name().into(),
            folders,
        },
        homes,
    ))
}

/// Writes `things` to a new archive at `out`, refusing a file already
/// there. Each is checked by its owner and refused while it runs, before
/// anything is written.
pub fn export(
    roots: &Roots,
    things: &[Selection],
    owners: &dyn Owners,
    out: &Path,
    description: &str,
    deleted: Option<&str>,
) -> anyhow::Result<Exported> {
    ensure!(!things.is_empty(), "nothing to export");
    // Where each owner writes the files it carries in place of its own,
    // removed however the export ends.
    let staging = Carrying::new(roots)?;
    let mut found = Vec::new();
    let mut manifest = Manifest {
        title: String::new(),
        description: description.into(),
        deleted: deleted.map(str::to_string),
        map: BTreeMap::new(),
        things: BTreeMap::new(),
    };
    for selection in things {
        let Some(owner) = owners.of(selection.kind()) else {
            bail!(
                "no module here exports {}s, so {selection} can't be exported",
                selection.kind()
            );
        };
        let (thing, homes) = here(roots, selection)?;
        owner
            .check(&thing)
            .with_context(|| format!("{selection} can't be exported"))?;
        if let Some(running) = owner.running(&thing)? {
            bail!(
                "{selection} is running, so its files may change while they're copied: {} stops it",
                running.stop
            );
        }
        let replaced = staging.ask(owner, &thing, selection)?;
        for home in &homes {
            manifest
                .map
                .insert(Entry::Home(*home).name(), home.relative().to_string());
        }
        manifest.things.insert(selection.to_string(), Thing {});
        found.push((selection.clone(), thing, homes, replaced));
    }
    if let Some(parent) = out.parent() {
        crate::reading::make_dirs(parent)?;
    }
    // Written beside it under a name of Link's own, then linked to its
    // name, which refuses one there: never an archive written in part
    // under its name.
    if out.symlink_metadata().is_ok() {
        bail!(
            "{} is there already, and an archive never overwrites a file: name another with --to",
            out.display()
        );
    }
    let beside = out.with_file_name(link_files::layout::temporary_name());
    let file = create_new(&beside)?;
    let written = (|| -> anyhow::Result<Vec<(Selection, u64)>> {
        let mut writer = Writer::new(std::io::BufWriter::new(file));
        writer.file(
            &crate::limits().manifest,
            link_files::saved::print(&manifest)?.as_bytes(),
        )?;
        let mut sizes = Vec::new();
        for (selection, thing, homes, replaced) in &found {
            let mut bytes = 0;
            for ((_, folder), home) in thing.folders.iter().zip(homes) {
                let at = format!("{}/{}", home.relative(), selection.name());
                bytes += writer.tree_replacing(&at, folder, replaced)?;
            }
            sizes.push((selection.clone(), bytes));
        }
        use std::io::Write;
        writer.finish()?.flush()?;
        Ok(sizes)
    })();
    let linked = written.and_then(|things| match std::fs::hard_link(&beside, out) {
        Ok(()) => Ok(things),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => bail!(
            "{} is there already, and an archive never overwrites a file: name another with --to",
            out.display()
        ),
        Err(err) => Err(err).with_context(|| format!("writing {}", out.display())),
    });
    let _ = std::fs::remove_file(&beside);
    linked.map(|things| Exported {
        path: out.into(),
        things,
    })
}

/// An export's staging folder in `~/.lnk`, a folder in it for each thing
/// whose owner carries files in place of its own: removed when
/// dropped.
struct Carrying {
    root: PathBuf,
}

impl Carrying {
    fn new(roots: &Roots) -> anyhow::Result<Self> {
        let root = roots.staging().join(link_files::layout::temporary_name());
        crate::reading::make_dirs(&root)?;
        Ok(Self { root })
    }

    /// What `owner` carries in place of `thing`'s own folders: each
    /// inside one of its folders, each replacement inside the folder it
    /// was given, else refused.
    fn ask(
        &self,
        owner: &dyn crate::Owner,
        thing: &ThingFolders,
        selection: &Selection,
    ) -> anyhow::Result<Vec<Replaced>> {
        let given = self.root.join(selection.to_string());
        crate::reading::make_dirs(&given)?;
        let replaced = owner
            .carry(thing, &given)
            .with_context(|| format!("{selection} can't be exported"))?;
        for r in &replaced {
            ensure!(
                thing
                    .folders
                    .iter()
                    .any(|(_, folder)| r.file.starts_with(folder) && &r.file != folder),
                "{selection}'s owner carries a file in place of {}, which isn't one of its own",
                r.file.display()
            );
            if let Some(with) = &r.with {
                ensure!(
                    with.starts_with(&given) && with != &given,
                    "{selection}'s owner carries {}, which isn't in the folder it was given",
                    with.display()
                );
            }
        }
        Ok(replaced)
    }
}

impl Drop for Carrying {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A new file at `path`, refusing one already there.
fn create_new(path: &Path) -> anyhow::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => bail!(
            "{} is there already, and an archive never overwrites a file: name another with --to",
            path.display()
        ),
        Err(err) => Err(err).with_context(|| format!("creating {}", path.display())),
    }
}
