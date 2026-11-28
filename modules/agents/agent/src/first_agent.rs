//! The agent the first `lnk agent start` creates on a machine with none:
//! Onboarding, whose archive folder the build carries into this module
//! (`first_agent.toml`), imported as any archive is.

use std::path::Path;

use anyhow::Context;
use link_archive::{Adopted, Missing, Replaced, Running, ThingFolders};

include!(concat!(env!("OUT_DIR"), "/first_agent.rs"));

/// Creates the first agent here from the archive carried in: its folder
/// written out, packed and imported as `lnk archive import` imports one,
/// answered by this module's own archive answers. It takes [`NAME`].
pub fn create() -> anyhow::Result<link_archive::ImportedThing> {
    let roots = link_archive::Roots::here()?;
    let staging = roots.staging();
    link_files::create_private_dir(&staging)
        .with_context(|| format!("creating {}", staging.display()))?;
    let folder = staging.join(link_files::layout::temporary_name());
    let archive = folder.with_extension("tar.gz");
    let imported = write_out(&folder).and_then(|()| {
        link_archive::pack(&folder, &archive)?;
        let pick = link_archive::Pick {
            thing: link_archive::Selection::new(crate::archives::KIND, NAME)?,
            to: None,
        };
        link_archive::import(
            &archive,
            &roots,
            Some(std::slice::from_ref(&pick)),
            &Answers,
            None,
        )
    });
    let _ = std::fs::remove_dir_all(&folder);
    let _ = std::fs::remove_file(&archive);
    imported?
        .things
        .pop()
        .context("the first agent's archive holds no agent")
}

/// The archive folder carried in, written to `folder`.
fn write_out(folder: &Path) -> anyhow::Result<()> {
    for (path, bytes) in FILES {
        let to = folder.join(path);
        if let Some(parent) = to.parent() {
            link_files::create_private_dir(parent)?;
        }
        std::fs::write(&to, bytes).with_context(|| format!("writing {}", to.display()))?;
    }
    Ok(())
}

/// This module's answers to the archive's questions, asked in-process
/// rather than through `lnk-agent answer`.
struct Answers;

/// A thing as the archive's questions name it: its folders by their
/// locations' keys.
fn thing(folders: &ThingFolders) -> link_archive_contract::Thing {
    link_archive_contract::Thing {
        kind: folders.kind.clone(),
        name: folders.name.clone(),
        folders: folders
            .folders
            .iter()
            .map(|(home, path)| {
                (
                    link_files::layout::map::Entry::Home(*home).name(),
                    path.clone(),
                )
            })
            .collect(),
    }
}

impl link_archive::Owner for Answers {
    fn check(&self, folders: &ThingFolders) -> anyhow::Result<()> {
        crate::archives::check(&thing(folders)).map(|_| ())
    }
    fn running(&self, _: &ThingFolders) -> anyhow::Result<Option<Running>> {
        Ok(None)
    }
    fn carry(&self, folders: &ThingFolders, _: &Path) -> anyhow::Result<Vec<Replaced>> {
        anyhow::bail!("{} is being imported, not exported", folders.name)
    }
    fn missing(&self, folders: &ThingFolders) -> anyhow::Result<Vec<Missing>> {
        Ok(crate::archives::missing(&thing(folders))?
            .needs
            .into_iter()
            .map(|need| Missing {
                what: need.what,
                setup: need.setup,
            })
            .collect())
    }
    fn adopt(&self, folders: &ThingFolders, was: &str) -> anyhow::Result<Adopted> {
        let adopted = crate::archives::adopt(&link_archive_contract::Adopt {
            thing: thing(folders),
            was: was.into(),
        })?;
        Ok(Adopted {
            notes: adopted.notes,
            next: adopted.next,
        })
    }
}

impl link_archive::Owners for Answers {
    fn of(&self, kind: &str) -> Option<&dyn link_archive::Owner> {
        (kind == crate::archives::KIND).then_some(self as &dyn link_archive::Owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the build carried in is the folder `first_agent.toml` names:
    /// an archive whose manifest holds the agent [`NAME`].
    #[test]
    fn the_carried_folder_is_an_archive_of_the_first_agent() {
        let manifest = FILES
            .iter()
            .find(|(path, _)| *path == link_archive::limits().manifest)
            .expect("the folder carried in has its archive.toml");
        let manifest: link_archive::Manifest =
            link_files::saved::parse(std::str::from_utf8(manifest.1).unwrap()).unwrap();
        let selection = format!("{}.{NAME}", crate::archives::KIND);
        assert!(manifest.things.contains_key(&selection), "{selection}");
    }
}
