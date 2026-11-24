//! Deterministic faults in owned fixtures. Cases assert recovery separately.

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::TempDir;

/// Which side of an operation loses its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// Refuse the operation before it changes anything.
    Before,
    /// Complete the operation, then lose its successful reply.
    After,
}

/// One boundary failure, followed by ordinary operation on subsequent calls.
/// The caller selects the boundary in its own protocol, without a clock or RNG.
#[derive(Debug)]
pub struct Once {
    moment: Moment,
    injected: bool,
}

impl Once {
    pub fn new(moment: Moment) -> Self {
        Self {
            moment,
            injected: false,
        }
    }

    /// An after fault consumes the first successful result. An operation that
    /// fails itself leaves that fault armed, rather than claiming an injection.
    pub fn run<T, E>(
        &mut self,
        operation: impl FnOnce() -> Result<T, E>,
        failure: impl FnOnce() -> E,
    ) -> Result<T, E> {
        if self.injected {
            return operation();
        }
        if self.moment == Moment::After {
            operation()?;
        }
        self.injected = true;
        Err(failure())
    }

    pub fn injected(&self) -> bool {
        self.injected
    }

    /// A case must prove it reached its fault, as well as its recovery oracle.
    pub fn assert_injected(&self) {
        assert!(
            self.injected,
            "the {:?} fault was never injected",
            self.moment
        );
    }
}

/// File damage inside a temporary folder owned by this test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFault {
    Remove,
    Truncate { bytes: u64 },
}

/// Observed file damage, without retaining the file's potentially secret bytes.
#[derive(Debug, PartialEq, Eq)]
pub struct FileEffect {
    pub path: PathBuf,
    pub before: u64,
    /// None means the file is absent.
    pub after: Option<u64>,
}

impl FileFault {
    /// Damage one existing regular file. Absolute paths, traversal, symlinks,
    /// multiply linked files and truncation that removes no bytes are refused.
    /// Fixtures must not concurrently replace ancestors during the injection.
    /// There is no automatic restore that could mask the product's recovery.
    pub fn apply(self, root: &TempDir, relative: impl AsRef<Path>) -> io::Result<FileEffect> {
        let relative = relative.as_ref();
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fault needs an owned regular file",
            )
        };
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(invalid());
        }
        let mut path = root.path().to_path_buf();
        for component in relative.components() {
            path.push(component.as_os_str());
            if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Err(invalid());
            }
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file() {
            return Err(invalid());
        }
        #[cfg(unix)]
        if std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
            return Err(invalid());
        }
        let before = metadata.len();
        let after = match self {
            Self::Remove => {
                std::fs::remove_file(&path)?;
                match std::fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => return Err(error),
                    Ok(_) => return Err(io::Error::other("removed fault file still exists")),
                }
            }
            Self::Truncate { bytes } => {
                if bytes >= before {
                    return Err(invalid());
                }
                let file = std::fs::OpenOptions::new().write(true).open(&path)?;
                file.set_len(bytes)?;
                file.sync_all()?;
                let actual = file.metadata()?.len();
                if actual != bytes {
                    return Err(io::Error::other(
                        "fault file did not reach its truncated size",
                    ));
                }
                Some(actual)
            }
        };
        Ok(FileEffect {
            path,
            before,
            after,
        })
    }
}

#[cfg(test)]
mod tests;
