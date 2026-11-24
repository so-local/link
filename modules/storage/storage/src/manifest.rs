//! `~/.lnk/safe/storage/manifest-<folder>.json`, one per folder, by
//! its key name-safe ([`crate::files::safe`]): what
//! went where. A cache, never the truth (the clouds are): `tree` reads
//! it so it's instant and works offline, and `clean` checks the clouds
//! again, taking from it only what an earlier clean proved of a copy the
//! cloud says is unchanged ([`Proven`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::said;
use link_format::{WithinExt, sentence};
use serde::{Deserialize, Serialize};

/// A folder's manifest, naming its schema.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub files: BTreeMap<String, Entry>,
}

link_files::specification!(Manifest, include_str!("manifest.schema.toml"));

/// One file in the bucket, by its path.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The file's size and modification time here when it was last
    /// checked; a local file that differs from them changed since.
    pub size: u64,
    #[serde(default)]
    pub modified: u64,
    /// The nanoseconds of that modification time, where the file system
    /// keeps them: with the size, what push compares to skip a file.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub nanos: u32,
    /// The copies, by cloud name.
    #[serde(default)]
    pub clouds: BTreeMap<String, Copy>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A copy in one cloud.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Copy {
    pub size: u64,
    /// When its checksum last matched the file here (seconds since 1970).
    /// `None` for a copy only seen in a listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<u64>,
    /// Its checksum didn't match the file here, the last time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub differs: bool,
    /// Its storage class (`DEEP_ARCHIVE`), when the cloud has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// What the last check `clean` takes proved, in a cloud whose
    /// checksums can be its uploader's: while the file here and the copy
    /// are both as they were then, the copy isn't read back again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proven: Option<Proven>,
}

/// A file here and its copy, as they were when a check proved them the
/// same.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proven {
    /// The file here: its size, inode, and modification time in
    /// nanoseconds.
    pub size: u64,
    pub inode: u64,
    pub nanos: u64,
    /// Its change time, in nanoseconds; 0 in a proof kept before it was,
    /// which then holds no more.
    #[serde(default)]
    pub changed: u64,
    /// The copy, as its cloud listed it.
    pub copy: crate::engine::Written,
}

/// The manifest, held while one `lnk storage` command runs: a second one
/// waits, so they can't overwrite each other's changes.
pub struct Locked {
    pub manifest: Manifest,
    path: PathBuf,
    _lock: std::fs::File,
}

impl Manifest {
    /// The manifest of the folder whose key is `folder` (`personal`, an
    /// owner's ID).
    pub fn open(folder: &str) -> anyhow::Result<Locked> {
        Self::open_in(&crate::files::config_dir()?, folder)
    }

    pub fn open_in(dir: &Path, folder: &str) -> anyhow::Result<Locked> {
        let folder = crate::files::safe(folder);
        link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let lock = options.open(dir.join(format!("lock-{folder}")))?;
        if lock.try_lock().is_err() {
            said::note(sentence!(
                "Waiting for another lnk storage command to finish."
            ));
            lock.lock()?;
        }
        let path = dir.join(format!("manifest-{folder}.json"));
        let manifest = match std::fs::read(&path) {
            Ok(bytes) => link_files::saved::parse_json(&bytes)
                .within(format!("reading {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Manifest::default(),
            Err(e) => return Err(e).within(format!("reading {}", path.display())),
        };
        Ok(Locked {
            manifest,
            path,
            _lock: lock,
        })
    }

    /// Forgets `cloud`'s copies, and files left with no copy anywhere.
    pub fn forget_cloud(&mut self, cloud: &str) {
        for entry in self.files.values_mut() {
            entry.clouds.remove(cloud);
        }
        self.files.retain(|_, entry| !entry.clouds.is_empty());
    }
}

impl Locked {
    /// Writes it back, atomically, as compact JSON (one written pretty
    /// reads the same).
    pub fn save(&self) -> anyhow::Result<()> {
        let json = link_files::saved::print_json(&self.manifest)?;
        link_files::write_private(&self.path, &json)
            .within(format!("writing {}", self.path.display()))
    }
}

impl std::ops::Deref for Locked {
    type Target = Manifest;
    fn deref(&self) -> &Manifest {
        &self.manifest
    }
}

impl std::ops::DerefMut for Locked {
    fn deref_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads() {
        let dir = link_test_kit::TempDir::new("manifest");
        {
            let mut m = Manifest::open_in(&dir, "personal").unwrap();
            m.files.insert(
                "a.txt".into(),
                Entry {
                    size: 3,
                    modified: 10,
                    nanos: 0,
                    clouds: [(
                        "aws".to_string(),
                        Copy {
                            size: 3,
                            checked: Some(20),
                            ..Copy::default()
                        },
                    )]
                    .into(),
                },
            );
            m.save().unwrap();
        }
        let mut m = Manifest::open_in(&dir, "personal").unwrap();
        assert_eq!(m.files["a.txt"].clouds["aws"].checked, Some(20));
        m.forget_cloud("aws");
        assert!(m.files.is_empty());
    }
}
