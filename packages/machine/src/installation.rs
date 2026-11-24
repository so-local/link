//! This installation: Link under one OS user, its ID and name
//! (`~/.lnk/safe/installation.toml`, yours). The machine it runs on is
//! hardware ([`crate::machine`]).

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::machine::{check_id, check_name, host_name, name_from_host};

mod constants;
pub use constants::{FILE, KIND};

/// This installation, as `installation.toml` says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub id: String,
    pub name: String,
}

link_files::specification!(Installation, include_str!("installation.schema.toml"));

impl Installation {
    /// Its ID as output writes it, `installation:<id>`; its saved form keeps
    /// the bare `id` its file has always had.
    pub fn resource(&self) -> String {
        format!("{KIND}:{}", self.id)
    }
}

/// This installation's ID and name, from `~/.lnk/safe/installation.toml`, written
/// the first time anything asks: a new ID, and the host name made to fit
/// ([`name_from_host`]).
pub fn installation() -> anyhow::Result<Installation> {
    let path = link_files::layout::Hidden::Installation.try_path()?;
    read_or_make(&path, || {
        Ok(Installation {
            id: link_id::candidate()?,
            name: name_from_host(&host_name()),
        })
    })
}

/// The complete identity at `path`, or one created under its writer lock.
fn read_or_make(
    path: &Path,
    make: impl FnOnce() -> anyhow::Result<Installation>,
) -> anyhow::Result<Installation> {
    let lock = link_files::settings::lock(path)?;
    if let Some(installation) = link_files::saved::read_optional::<Installation>(path)? {
        check(&installation, path)?;
        return Ok(installation);
    }
    let installation = make()?;
    check(&installation, path)?;
    lock.write(&installation)?;
    Ok(installation)
}

fn check(installation: &Installation, path: &Path) -> anyhow::Result<()> {
    check_id(&installation.id).with_context(|| format!("in {}", path.display()))?;
    check_name(&installation.name).map_err(|err| {
        anyhow::anyhow!(
            "in {}: {err}; change the name in that file to one it can be",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_identity_creation_keeps_one_complete_identity() {
        let root = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "identity-creation",
        );
        let path = root.join(FILE);
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        let starting = std::sync::Barrier::new(2);
        let identities = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                starting.wait();
                read_or_make(&path, || {
                    Ok(Installation {
                        id: "aaaaaaaaaaaa".into(),
                        name: "other".into(),
                    })
                })
                .unwrap()
            });
            let second = scope.spawn(|| {
                starting.wait();
                read_or_make(&path, || {
                    Ok(Installation {
                        id: "bbbbbbbbbbbb".into(),
                        name: "other".into(),
                    })
                })
                .unwrap()
            });
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(identities.0, identities.1);
        assert_eq!(
            link_files::saved::read_optional::<Installation>(&path)
                .unwrap()
                .unwrap(),
            identities.0
        );
    }

    #[test]
    fn a_installation_is_made_once() {
        let dir = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "installation-identity",
        );
        let path = dir.join(FILE);
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        let made = Installation {
            id: "k3v9q2x7m1ab".into(),
            name: "gcp-1".into(),
        };
        assert_eq!(read_or_make(&path, || Ok(made.clone())).unwrap(), made);
        // Asked again, the same one, whatever another would make.
        let other = || -> anyhow::Result<Installation> { panic!("made twice") };
        assert_eq!(read_or_make(&path, other).unwrap(), made);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "schema = 1\nid = \"k3v9q2x7m1ab\"\nname = \"gcp-1\"\n"
        );
        // One someone broke is an error, not replaced.
        std::fs::write(&path, "id = \"x\"\nname = \"gcp-1\"\n").unwrap();
        assert!(read_or_make(&path, || Ok(made.clone())).is_err());
        // A name that is one of Link's words says which file and what to change.
        std::fs::write(&path, "id = \"k3v9q2x7m1ab\"\nname = \"all\"\n").unwrap();
        let said = format!(
            "{:#}",
            read_or_make(&path, || Ok(made.clone())).unwrap_err()
        );
        assert!(
            said.contains("installation.toml") && said.contains("\"all\""),
            "{said}"
        );
        assert!(said.contains("pick another name"), "{said}");
        assert!(said.contains("change the name in that file"), "{said}");
        for incomplete in [
            "",
            "name = 'edge'\n",
            "id = 'k3v9q2x7m1ab'\n",
            "id = 'k3v9q2x7m1ab'\nname = 'edge'\nunsandboxed = false\n",
        ] {
            std::fs::write(&path, incomplete).unwrap();
            assert!(read_or_make(&path, other).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), incomplete);
        }
    }
}
