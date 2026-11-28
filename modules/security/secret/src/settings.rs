//! This machine's secret settings, independent of installation identity.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    keeper: Option<bool>,
}

link_files::specification!(Settings, include_str!("settings.schema.toml"));

/// The secret settings' file, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Settings>()
}

/// The keeper setting's document, separate from the secret store.
pub fn path() -> anyhow::Result<PathBuf> {
    link_files::layout::Hidden::SecretSettings.try_path()
}

fn load(path: &Path) -> anyhow::Result<Settings> {
    link_files::saved::read(path)
}

pub(crate) fn keeper_refused() -> anyhow::Result<bool> {
    Ok(load(&path()?)?.keeper == Some(false))
}

pub(crate) fn refuse_keeper() -> anyhow::Result<()> {
    refuse_keeper_at(&path()?)
}

fn refuse_keeper_at(path: &Path) -> anyhow::Result<()> {
    // Written where a link leads, under that file's lock.
    let real = if path.exists() {
        std::fs::canonicalize(path)?
    } else {
        path.to_owned()
    };
    let lock = link_files::settings::lock(&real)?;
    let mut settings = load(&real)?;
    settings.keeper = Some(false);
    lock.write(&settings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keeper_refusal_is_saved_only_in_its_settings() {
        let dir = link_test_kit::TempDir::under(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("target"),
            "keeper-settings",
        );
        let path = dir.join(link_files::layout::Hidden::SecretSettings.relative());
        let store = dir.join("secrets.toml");
        link_files::write_private(&store, b"existing secret store").unwrap();
        if !link_test_kit::gate::settings(&path) {
            return;
        }
        assert_eq!(load(&path).unwrap().keeper, None);
        refuse_keeper_at(&path).unwrap();
        assert_eq!(load(&path).unwrap().keeper, Some(false));
        link_files::write_private(&path, b"keeper = 'false'\n").unwrap();
        assert!(refuse_keeper_at(&path).is_err());
        assert_eq!(std::fs::read(store).unwrap(), b"existing secret store");
    }
}
