//! What only a thing's owner knows, which it answers from files alone:
//! the five questions an archive asks of each thing it carries.

use std::path::{Path, PathBuf};

use link_files::layout::Home;

/// A thing and its folders, by the location each is a folder of
/// (`Home::AgentSettings`): where it is here, or where it's staged.
#[derive(Clone, Debug)]
pub struct ThingFolders {
    pub kind: String,
    pub name: String,
    pub folders: Vec<(Home, PathBuf)>,
}

impl ThingFolders {
    /// Its folder of location `home`.
    pub fn folder(&self, home: Home) -> Option<&PathBuf> {
        self.folders
            .iter()
            .find(|(h, _)| *h == home)
            .map(|(_, f)| f)
    }
}

/// What an owner said of a thing it adopted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adopted {
    /// What to tell you of it.
    pub notes: Vec<String>,
    /// The command that starts it, once its requirements are met.
    pub next: String,
}

/// A thing that's running, so its files may change while they're copied.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    /// The command that stops it (`lnk agent stop --agent programmer`).
    pub stop: String,
}

/// Something an imported thing needs before it runs here, and the one
/// command that sets it up, which import names and never runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Missing {
    pub what: String,
    pub setup: String,
}

/// A file of a thing's that an export carries in place of its own, or
/// leaves out.
#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    /// The thing's file, inside one of its folders.
    pub file: PathBuf,
    /// What goes in its place, inside the staging folder: none to leave
    /// it out.
    pub with: Option<PathBuf>,
}

/// A kind's owner, answering from files alone.
pub trait Owner {
    /// Reads the thing's files with their owning types, refusing what
    /// they refuse, or what its files alone don't carry whole.
    fn check(&self, thing: &ThingFolders) -> anyhow::Result<()>;
    /// Whether it's running here, so export refuses it.
    fn running(&self, thing: &ThingFolders) -> anyhow::Result<Option<Running>>;
    /// The files an export carries in place of the thing's own, or leaves
    /// out, so the archive holds nothing of this machine's, each written
    /// into `staging`, an empty folder: none when its files go as they
    /// are.
    fn carry(&self, thing: &ThingFolders, staging: &Path) -> anyhow::Result<Vec<Replaced>>;
    /// What it still needs to run here, read from its files.
    fn missing(&self, thing: &ThingFolders) -> anyhow::Result<Vec<Missing>>;
    /// Writes the owner's own records in `~/.lnk` for a thing arrived in
    /// its folders, under the name chosen (`was` its name in the archive):
    /// what to tell you of it and the command that starts it.
    fn adopt(&self, thing: &ThingFolders, was: &str) -> anyhow::Result<Adopted>;
}

/// The owners of the kinds an archive can carry here.
pub trait Owners {
    /// Kind `kind`'s owner, when one here exports and imports it.
    fn of(&self, kind: &str) -> Option<&dyn Owner>;
}
