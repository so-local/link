//! Archives: a thing's state carried between machines and people as one
//! `.tar.gz`, by reading and writing files alone.
//!
//! A thing's state is its folders under `~/Link`, which `layout.toml`'s
//! `[owners]` names as its owner's `state` parts. An archive holds any
//! number of things, each selected by kind and name (`agent.programmer`,
//! [`Selection`]), laid out as a new machine's `~/Link` is and recorded
//! in `archive.toml` ([`Manifest`]). Export copies each thing's folders
//! in ([`export()`]); import reads the whole archive as untrusted data into
//! staging in `~/.lnk` ([`Staged`]), checks it, settles each thing's name
//! ([`names`]) and moves it into place. What only a thing's owner knows
//! it answers through [`Owner`]. Nothing here knows what an agent is.

mod dates;
mod export;
mod import;
mod limits;
mod manifest;
pub mod names;
mod owner;
mod packing;
mod reading;
mod selection;
mod writing;

pub use dates::{day_number, expired};
pub use export::{Exported, default_name, everything, export};
pub use import::{Imported, ImportedThing, import};
pub use limits::{Limits, limits};
pub use manifest::{Manifest, Thing};
pub use owner::{Adopted, Missing, Owner, Owners, Replaced, Running, ThingFolders};
pub use packing::pack;
pub use reading::{Folder, Staged, StagedThing, read_manifest, stage};
pub use selection::{Pick, Selection, parse_picks};
pub use writing::Writer;

/// Where an archive's things are and go on this machine: the Link folder
/// and the installation folder, `~/.lnk`.
#[derive(Clone, Debug)]
pub struct Roots {
    pub link: std::path::PathBuf,
    pub hidden: std::path::PathBuf,
}

impl Roots {
    /// This machine's, as its map says.
    pub fn here() -> anyhow::Result<Self> {
        Ok(Self {
            link: link_files::try_link_dir()?,
            hidden: link_files::try_installation_dir()?,
        })
    }

    /// `~/Link/Archive`, where archives are kept.
    pub fn archive(&self) -> std::path::PathBuf {
        link_files::layout::Home::Archive.at(&self.link)
    }

    /// Where an import is staged, in `~/.lnk`, never in `~/Link`.
    pub fn staging(&self) -> std::path::PathBuf {
        link_files::layout::Hidden::ArchiveStaging.at(&self.hidden)
    }

    /// The folder of thing `name` in state location `home`.
    fn folder(&self, home: link_files::layout::Home, name: &str) -> std::path::PathBuf {
        home.at(&self.link).join(name)
    }

    /// Whether `name` is taken here for owner `word`: any of its folders,
    /// in any part, is there.
    pub fn taken(&self, word: &str, name: &str) -> bool {
        link_files::layout::thing_folders(word, name, &self.link, &self.hidden)
            .iter()
            .any(|folder| folder.symlink_metadata().is_ok())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);
