//! Buckets' IDs, `bucket:<id>`: each minted once, when Link adds the
//! bucket here, and never given again. The ledger,
//! `~/.lnk/safe/storage/ids.toml`, keeps every ID minted or brought here
//! and every one retired, when Link deletes its bucket, under its own
//! lock. A cloud disconnected leaves its buckets, so their IDs stay. A
//! bucket that arrives with an owner's move keeps the ID it had.

use std::path::{Path, PathBuf};

use link_format::WithinExt;
use link_id::Ledger;

/// The ledger as its file keeps it, naming its schema.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Kept(link_id::Table);

link_files::specification!(Kept, include_str!("ids.schema.toml"));

/// The bucket ID ledger, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Kept>()
}

/// Where the ledger is: beside `clouds.toml`.
fn path() -> anyhow::Result<PathBuf> {
    Ok(crate::files::config_dir()?.join("ids.toml"))
}

/// `change` on the ledger at `file`, under its lock, kept when it
/// succeeds.
fn with_at<T>(
    file: &Path,
    change: impl FnOnce(&mut link_id::Table) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    if let Some(parent) = file.parent() {
        link_files::create_private_dir(parent).within(format!("making {}", parent.display()))?;
    }
    let _lock = link_files::lock_file(&file.with_extension("lock"))?;
    let mut kept: Kept = match std::fs::read_to_string(file) {
        Ok(text) => {
            link_files::saved::parse(&text).within(format!("reading {}", file.display()))?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Kept::default(),
        Err(err) => return Err(err).within(format!("reading {}", file.display())),
    };
    let done = change(&mut kept.0)?;
    link_files::write_private(file, link_files::saved::print(&kept)?.as_bytes())
        .within(format!("writing {}", file.display()))?;
    Ok(done)
}

/// A new bucket's ID, reserved: never one minted or retired here.
pub fn reserve() -> anyhow::Result<String> {
    reserve_at(&path()?)
}

fn reserve_at(file: &Path) -> anyhow::Result<String> {
    with_at(file, |ids| Ok(link_id::reserve(ids)?))
}

/// The ID a bucket brought here has, held: refused when it's retired
/// here. One live here already is the same bucket, come back.
pub fn arrive(id: &str) -> anyhow::Result<()> {
    arrive_at(&path()?, id)
}

fn arrive_at(file: &Path, id: &str) -> anyhow::Result<()> {
    link_machine::machine::check_id(id)?;
    with_at(file, |ids| {
        anyhow::ensure!(
            !ids.is_retired(id),
            "bucket:{id} was deleted here and is never given again"
        );
        ids.hold(id);
        Ok(())
    })
}

/// Retires `retired`, buckets Link deleted: none is given again.
pub fn retire<'a>(retired: impl IntoIterator<Item = &'a str>) -> anyhow::Result<()> {
    retire_at(&path()?, retired)
}

fn retire_at<'a>(file: &Path, retired: impl IntoIterator<Item = &'a str>) -> anyhow::Result<()> {
    with_at(file, |ids| {
        for id in retired {
            ids.retire(id);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bucket's ID is never given twice: a retired one is never minted
    /// or brought here again, and one that comes back is held.
    #[test]
    fn a_buckets_id_is_minted_once() {
        let dir = link_test_kit::TempDir::new("bucket-ids");
        let file = dir.join("storage").join("ids.toml");
        let first = reserve_at(&file).unwrap();
        let second = reserve_at(&file).unwrap();
        assert_ne!(first, second);
        assert!(link_id::is_candidate(&first));
        retire_at(&file, [first.as_str()]).unwrap();
        assert!(arrive_at(&file, &first).is_err());
        // Come back after a move: the same bucket, held.
        arrive_at(&file, &second).unwrap();
        let other = link_id::candidate().unwrap();
        arrive_at(&file, &other).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("schema = 1"), "{text}");
        let kept: Kept = link_files::saved::parse(&text).unwrap();
        assert!(kept.0.is_retired(&first));
        assert!(kept.0.is_live(&second) && kept.0.is_live(&other));
        assert!(arrive_at(&file, "not-an-id").is_err());
    }
}
