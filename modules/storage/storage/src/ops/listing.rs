//! What the clouds hold: the manifest rebuilt from their listings, and
//! the tree of each file, here and in each cloud.

use super::*;

/// Lists `cloud` again and makes the manifest match: copies no longer
/// there are forgotten, new ones added (not checked yet), and a copy
/// whose size changed loses its check.
pub fn refresh(
    engine: &dyn Engine,
    manifest: &mut Manifest,
    cloud: &Cloud,
) -> anyhow::Result<usize> {
    // A name that could climb out of the folder is never recorded.
    let listed: Vec<_> = engine
        .list(cloud)?
        .into_iter()
        .filter(|l| files::safe_path(&l.path))
        .collect();
    let names: std::collections::HashSet<&str> = listed.iter().map(|l| l.path.as_str()).collect();
    manifest.files.retain(|path, entry| {
        if !names.contains(path.as_str()) {
            entry.clouds.remove(&cloud.name);
        }
        !entry.clouds.is_empty()
    });
    for item in &listed {
        let entry = manifest
            .files
            .entry(item.path.clone())
            .or_insert_with(|| Entry {
                size: item.size,
                ..Entry::default()
            });
        let copy = entry.clouds.entry(cloud.name.clone()).or_default();
        if copy.size != item.size {
            copy.checked = None;
            copy.proven = None;
        }
        copy.size = item.size;
        copy.class = item.class.clone();
    }
    Ok(listed.len())
}

/// Files whose only good copy is in `cloud` (not here, nowhere else).
/// Without the folder's `root`, where it is here isn't known, so a file
/// no other cloud has counts, here or not.
pub fn only_in(root: Option<&Path>, manifest: &Manifest, cloud: &str) -> Vec<String> {
    manifest
        .files
        .iter()
        .filter(|(path, entry)| {
            entry.clouds.get(cloud).is_some_and(|c| !c.differs)
                && !entry
                    .clouds
                    .iter()
                    .any(|(name, c)| name != cloud && !c.differs)
                && !root.is_some_and(|root| {
                    files::parents_are_real(root, path)
                        && std::fs::symlink_metadata(root.join(path)).is_ok_and(|m| m.is_file())
                })
        })
        .map(|(path, _)| path.clone())
        .collect()
}

/// Where a file is, on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Here {
    /// Here, as it was last checked.
    Here,
    /// Here, but changed since it was last pushed.
    Changed,
    /// Here, never pushed.
    New,
    /// Only in the clouds.
    Offloaded,
}

/// A copy in one cloud, as `tree` shows it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Checked,
    Archived,
    Unchecked,
    Differs,
    Missing,
}

impl Mark {
    pub fn symbol(self) -> &'static str {
        match self {
            Mark::Checked => "✓",
            Mark::Archived => "A",
            Mark::Unchecked => "?",
            Mark::Differs => "≠",
            Mark::Missing => "✗",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub path: String,
    pub size: u64,
    pub here: Here,
    /// By cloud, in the order they were connected.
    pub clouds: Vec<(String, Mark)>,
}

/// `lnk storage list --tree`: every file under `prefix`, here or in a cloud.
pub fn tree(
    root: &Path,
    manifest: &Manifest,
    clouds: &[Cloud],
    prefix: &str,
) -> anyhow::Result<Vec<Row>> {
    let locals = files::walk(root, prefix)?;
    let here: std::collections::HashSet<&str> = locals.iter().map(|l| l.path.as_str()).collect();
    let mut rows = Vec::new();
    let marks = |entry: Option<&Entry>| -> Vec<(String, Mark)> {
        clouds
            .iter()
            .map(|cloud| {
                let mark = match entry.and_then(|e| e.clouds.get(&cloud.name)) {
                    None => Mark::Missing,
                    Some(copy) if copy.differs => Mark::Differs,
                    Some(copy) if copy.checked.is_none() => Mark::Unchecked,
                    Some(copy) if copy.class.as_deref().is_some_and(is_archive) => Mark::Archived,
                    Some(_) => Mark::Checked,
                };
                (cloud.name.clone(), mark)
            })
            .collect()
    };
    for local in &locals {
        let entry = manifest.files.get(&local.path);
        // A copy only seen in a listing (`connect`, `tree --refresh`) has
        // no modification time to compare: only its size says it changed.
        let here = match entry {
            None => Here::New,
            Some(e)
                if e.size == local.size && (e.modified == local.modified || e.modified == 0) =>
            {
                Here::Here
            }
            Some(_) => Here::Changed,
        };
        rows.push(Row {
            path: local.path.clone(),
            size: local.size,
            here,
            clouds: marks(entry),
        });
    }
    for (path, entry) in &manifest.files {
        if matches(prefix, path) && !here.contains(path.as_str()) {
            rows.push(Row {
                path: path.clone(),
                size: entry.size,
                here: Here::Offloaded,
                clouds: marks(Some(entry)),
            });
        }
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}
