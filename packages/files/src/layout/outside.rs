//! A folder in `~/.lnk` of an owner outside this catalogue: a module that
//! isn't public declares its own, as [`HiddenPlace`]s in its own
//! `locations.toml` beside its code, read with
//! [`super::system_places`] as its system places are. Its path is checked
//! as it's read: under the part its kind is in (`safe`, `data` or
//! `exec`), in a folder of its owner's that no owner of `[owners]` has.
//! It's not on the map, which pins the catalogue's locations: it's at its
//! declared path in this machine's `~/.lnk`.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, ensure};
use serde::Deserialize;

use super::{Kind, LAYOUT, Root};

/// One folder or file of an owner outside the catalogue, in `~/.lnk`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "Declared")]
pub struct HiddenPlace {
    path: PathBuf,
    kind: Kind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declared {
    path: PathBuf,
    kind: Kind,
}

impl TryFrom<Declared> for HiddenPlace {
    type Error = anyhow::Error;

    fn try_from(declared: Declared) -> anyhow::Result<Self> {
        check(&declared.path, declared.kind)?;
        Ok(HiddenPlace {
            path: declared.path,
            kind: declared.kind,
        })
    }
}

/// Refused unless `path` is under the part of `~/.lnk` its `kind` is in,
/// in a folder of an owner the catalogue doesn't have.
fn check(path: &Path, kind: Kind) -> anyhow::Result<()> {
    super::relative(path)?;
    let words: Vec<&str> = path
        .components()
        .map(|c| match c {
            Component::Normal(word) => word.to_str().unwrap_or_default(),
            _ => "",
        })
        .collect();
    let boundaries = &LAYOUT.boundaries;
    let part = match kind {
        Kind::Control => &boundaries.hidden_safe,
        Kind::Data => &boundaries.data,
        Kind::Exec => &boundaries.exec,
    };
    let [first, owner, ..] = words.as_slice() else {
        bail!(
            "{}: a place is in its owner's folder in {part}/",
            path.display()
        );
    };
    ensure!(
        first == part,
        "{}: a {kind:?} place is in {part}/",
        path.display()
    );
    ensure!(
        !owner.is_empty() && *owner != boundaries.core && !LAYOUT.owners.contains_key(*owner),
        "{}: {owner} is an owner of layout.toml's, whose places it declares",
        path.display()
    );
    Ok(())
}

impl HiddenPlace {
    /// Its trust boundary.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Its path relative to `~/.lnk`.
    pub fn relative(&self) -> &Path {
        &self.path
    }

    /// It in installation folder `hidden`.
    pub fn at(&self, hidden: &Path) -> PathBuf {
        hidden.join(&self.path)
    }

    /// This process's, refusing an unavailable installation folder.
    pub fn try_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.at(&Root::Hidden.try_path()?))
    }

    /// This process's, with the installation folder's missing-HOME fallback.
    pub fn path(&self) -> PathBuf {
        self.at(&Root::Hidden.path())
    }

    /// It in an explicit user's home, independent of `LNK_INSTALLATION_DIR`.
    pub fn in_home(&self, home: &Path) -> PathBuf {
        self.at(&home.join(&LAYOUT.roots.hidden))
    }

    /// It relative to an explicit user's home, for remote recipes.
    pub fn relative_to_home(&self) -> PathBuf {
        self.in_home(Path::new(""))
    }
}
