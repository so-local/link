//! What `lnk agent delete` keeps of an agent: its state as one archive
//! in `~/Link/Archive`, `<name>-<date>.tar.gz`, written through the
//! archive package before anything of it goes, recorded as a deletion
//! so it expires; or, when a file no archive carries is in it, its
//! folders put aside instead.

use std::path::PathBuf;

use crate::moved::{Kept, date, now};
use crate::service;
use crate::settings::Settings;

/// What a deletion keeps of an agent's state, made by [`preserve`]
/// before anything of it goes.
pub enum Preserved {
    /// Nothing: its files go too.
    Nothing,
    /// One archive in `~/Link/Archive`.
    Archive(PathBuf),
    /// Its folders, put aside, since they couldn't be one archive (why).
    Folder(String),
}

/// Keeps agent `agent`'s state as `kept` asks, before anything of it
/// goes: stopped, then written to one archive, or, when a file in it is
/// one no archive carries (a link, a hard link), left to be put aside as
/// a folder.
pub fn preserve(agent: &Settings, kept: Kept) -> anyhow::Result<Preserved> {
    if matches!(kept, Kept::Nothing) {
        return Ok(Preserved::Nothing);
    }
    service::stop(&agent.name)?;
    Ok(match archive_deleted(agent) {
        Ok(archive) => Preserved::Archive(archive),
        Err(err) => Preserved::Folder(format!("{err:#}")),
    })
}

/// What a deletion asks of an agent it archives: nothing, since it's
/// stopped and its files are what they are, on whichever harness.
struct Deleting;

impl link_archive::Owner for Deleting {
    fn check(&self, _: &link_archive::ThingFolders) -> anyhow::Result<()> {
        Ok(())
    }
    fn running(
        &self,
        _: &link_archive::ThingFolders,
    ) -> anyhow::Result<Option<link_archive::Running>> {
        Ok(None)
    }
    fn carry(
        &self,
        _: &link_archive::ThingFolders,
        _: &std::path::Path,
    ) -> anyhow::Result<Vec<link_archive::Replaced>> {
        Ok(Vec::new())
    }
    fn missing(
        &self,
        _: &link_archive::ThingFolders,
    ) -> anyhow::Result<Vec<link_archive::Missing>> {
        Ok(Vec::new())
    }
    fn adopt(
        &self,
        thing: &link_archive::ThingFolders,
        _: &str,
    ) -> anyhow::Result<link_archive::Adopted> {
        anyhow::bail!("{} is being deleted, not imported", thing.name)
    }
}

impl link_archive::Owners for Deleting {
    fn of(&self, kind: &str) -> Option<&dyn link_archive::Owner> {
        (kind == crate::archives::KIND).then_some(self as &dyn link_archive::Owner)
    }
}

/// Writes agent `agent`'s state to `~/Link/Archive/<name>-<date>.tar.gz`
/// (`-2`, `-3`, ... when that's taken), recording that a deletion made
/// it, so it expires after [`crate::moved::keep_days`]; returns where.
fn archive_deleted(agent: &Settings) -> anyhow::Result<PathBuf> {
    let roots = link_archive::Roots::here()?;
    let today = date(now());
    let base = format!("{}-{today}", agent.name);
    let mut to = roots.archive().join(format!("{base}.tar.gz"));
    let mut n = 2;
    while to.symlink_metadata().is_ok() {
        to = roots.archive().join(format!("{base}-{n}.tar.gz"));
        n += 1;
    }
    let thing = link_archive::Selection::new(crate::archives::KIND, &agent.name)?;
    link_archive::export(
        &roots,
        std::slice::from_ref(&thing),
        &Deleting,
        &to,
        &format!("{thing}, deleted on {today}"),
        Some(&today),
    )?;
    Ok(to)
}
