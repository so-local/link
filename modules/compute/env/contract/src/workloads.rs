//! The workload IDs of environments and jobs, `env:<id>` and `job:<id>`:
//! each minted once for a spec where it is, the first time it's needed (a
//! box made from an environment, a job's run, a secret granted to it),
//! and kept in its module's state (`~/.lnk/safe/envs/ids.toml`,
//! `~/.lnk/safe/jobs/ids.toml`) by the spec's real path, never in the spec.
//! So every later run of the spec is the same workload, while a copy or a
//! move of it is another, granted nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use link_format::WithinExt;
use link_id::{Ledger, Workload};
use serde::{Deserialize, Serialize};

/// An owner's IDs, as its `ids.toml` keeps them: every ID minted here,
/// live or retired, and each spec's, by its real path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kept {
    #[serde(default)]
    ids: link_id::Table,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    spec: BTreeMap<PathBuf, Workload>,
}

/// Where the environment module (`env`) keeps its specs' IDs. A module
/// outside the catalogue keeps its own in a folder it declares, and names
/// it to [`of_at`].
pub fn ids_of(kind: &str) -> anyhow::Result<PathBuf> {
    match kind {
        "env" => Ok(link_files::layout::Hidden::Envs
            .try_path()?
            .join("ids.toml")),
        other => anyhow::bail!(
            "{:?} keeps its own IDs: name its file to of_at",
            link_format::clean(other)
        ),
    }
}

link_files::specification!(Kept, include_str!("workloads.schema.toml"));

/// The workload ledger, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Kept>()
}

/// The workload of `kind` whose spec is `spec`, minted and kept now when
/// it has none: as [`of_at`], in the state of the module that keeps it.
pub fn of(kind: &str, spec: &Path) -> anyhow::Result<Workload> {
    of_at(kind, &ids_of(kind)?, spec)
}

/// The workload of `kind` whose spec is `spec`, as `ids` keeps them,
/// under its lock: the one kept for the spec's real path, else a new one,
/// reserved there (never one minted here before).
pub fn of_at(kind: &str, ids: &Path, spec: &Path) -> anyhow::Result<Workload> {
    let real = std::fs::canonicalize(spec).within(format!("finding {}", spec.display()))?;
    if let Some(parent) = ids.parent() {
        link_files::create_private_dir(parent).within(format!("making {}", parent.display()))?;
    }
    let _lock = link_files::lock_file(&ids.with_extension("lock"))?;
    let mut kept: Kept = match std::fs::read_to_string(ids) {
        Ok(text) => link_files::saved::parse(&text).within(format!("reading {}", ids.display()))?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Kept::default(),
        Err(err) => return Err(err).within(format!("reading {}", ids.display())),
    };
    if let Some(workload) = kept.spec.get(&real) {
        anyhow::ensure!(
            workload.kind().to_string() == kind && kept.ids.is_live(workload.id()),
            "{} names {workload} for {}, not a live {kind}'s ID",
            ids.display(),
            real.display()
        );
        return Ok(workload.clone());
    }
    let id = link_id::reserve(&mut kept.ids)?;
    let workload = Workload::of(kind, &id)?;
    kept.spec.insert(real, workload.clone());
    link_files::write_private(ids, link_files::saved::print(&kept)?.as_bytes())
        .within(format!("writing {}", ids.display()))?;
    Ok(workload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec's ID is minted once and kept by where it is: the same spec
    /// is the same workload; a copy of its folder is another, and so is
    /// the spec moved.
    #[test]
    fn a_spec_is_one_workload_where_it_is() {
        let dir = link_test_kit::TempDir::new("env-workloads");
        let ids = dir.join("state").join("ids.toml");
        let first = dir.join("a");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::write(first.join("env.toml"), "name = \"app\"\n").unwrap();
        let app = of_at("env", &ids, &first.join("env.toml")).unwrap();
        assert_eq!(app.kind().to_string(), "env");
        assert!(link_id::is_candidate(app.id()));
        assert_eq!(of_at("env", &ids, &first.join("env.toml")).unwrap(), app);
        // Its folder copied: another workload.
        let copy = dir.join("b");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::copy(first.join("env.toml"), copy.join("env.toml")).unwrap();
        let copied = of_at("env", &ids, &copy.join("env.toml")).unwrap();
        assert_ne!(copied, app);
        // Moved: another again, and the first's stays with where it was.
        let moved = dir.join("c");
        std::fs::rename(&first, &moved).unwrap();
        let after = of_at("env", &ids, &moved.join("env.toml")).unwrap();
        assert!(after != app && after != copied);
        // Kept in the owner's state, never in the spec.
        let spec = std::fs::read_to_string(moved.join("env.toml")).unwrap();
        assert!(!spec.contains(app.id()), "{spec}");
        let text = std::fs::read_to_string(&ids).unwrap();
        assert!(text.contains(&app.to_string()), "{text}");
        // A job's are its own kind.
        let job = of_at("job", &dir.join("job-ids.toml"), &copy.join("env.toml")).unwrap();
        assert_eq!(job.kind().to_string(), "job");
        assert!(of_at("env", &ids, &dir.join("none.toml")).is_err());
    }
}
