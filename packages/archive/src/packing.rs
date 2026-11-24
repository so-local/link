//! An archive kept as a folder, as Link keeps its own agents in its
//! repository: `archive.toml` at its top and each thing's folders laid
//! out as its map says, written as an archive by the same writer export
//! uses.

use std::path::Path;

use anyhow::{Context, bail, ensure};

use crate::{Writer, limits, reading::Plan};

/// Writes archive folder `folder` to a new archive at `out`: its
/// `archive.toml` read and checked as import reads it, then its root
/// files and each thing's folders. A file anywhere else in it is
/// refused, so nothing in the folder is left out unsaid.
pub fn pack(folder: &Path, out: &Path) -> anyhow::Result<()> {
    let limits = limits();
    let manifest_path = folder.join(&limits.manifest);
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest = link_files::saved::parse(&text)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let plan = Plan::of(&manifest).with_context(|| format!("in {}", manifest_path.display()))?;
    let folders: Vec<String> = plan.folders().map(str::to_string).collect();
    unsaid(folder, folder, &folders)?;
    if let Some(parent) = out.parent() {
        crate::reading::make_dirs(parent)?;
    }
    if out.symlink_metadata().is_ok() {
        bail!(
            "{} is there already, and an archive never overwrites a file",
            out.display()
        );
    }
    let beside = out.with_file_name(link_files::layout::temporary_name());
    let written = (|| -> anyhow::Result<()> {
        use std::io::Write;
        let file = std::fs::File::create_new(&beside)
            .with_context(|| format!("creating {}", beside.display()))?;
        let mut writer = Writer::new(std::io::BufWriter::new(file));
        writer.file(&limits.manifest, text.as_bytes())?;
        for root in limits.root_files.iter().filter(|f| **f != limits.manifest) {
            let path = folder.join(root);
            if path.is_file() {
                writer.file(root, &std::fs::read(&path)?)?;
            }
        }
        for at in &folders {
            let from = folder.join(at);
            ensure!(
                from.is_dir(),
                "{} names {at}, which isn't a folder there",
                manifest_path.display()
            );
            writer.tree(at, &from)?;
        }
        writer.finish()?.flush()?;
        std::fs::hard_link(&beside, out).with_context(|| format!("writing {}", out.display()))
    })();
    let _ = std::fs::remove_file(&beside);
    written
}

/// An error for a file under `dir` that's neither a root file nor in one
/// of `folders`, each relative to `top`.
fn unsaid(top: &Path, dir: &Path, folders: &[String]) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let rel = path
            .strip_prefix(top)?
            .to_str()
            .with_context(|| format!("{} isn't UTF-8", path.display()))?
            .to_string();
        let inside = folders
            .iter()
            .any(|f| rel == *f || rel.starts_with(&format!("{f}/")));
        let leads = folders.iter().any(|f| f.starts_with(&format!("{rel}/")));
        let root_file = limits().root_files.contains(&rel);
        if inside || root_file {
            continue;
        }
        if leads && path.is_dir() {
            unsaid(top, &path, folders)?;
            continue;
        }
        bail!(
            "{} is in the archive folder but in no thing archive.toml names",
            path.display()
        );
    }
    Ok(())
}
