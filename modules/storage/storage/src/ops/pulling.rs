//! A pull: each file read from the cheapest cloud that has it, archives
//! restored first, never over a file changed here.

use super::*;

/// Where the files `pull` gets come from.
#[derive(Debug, Default)]
pub struct PullPlan {
    /// Files to download, by cloud.
    pub download: Vec<(Cloud, Vec<String>)>,
    /// Archived files to restore first, by cloud.
    pub restore: Vec<(Cloud, Vec<String>)>,
    /// Matching files already here, unchanged since they were checked.
    pub here: Vec<String>,
    /// Matching files not pulled: a folder on the way to them here is a
    /// link, which would lead outside the folder.
    pub refused: Vec<String>,
    pub bytes: u64,
}

/// `lnk storage pull`, first half: for each file matching `patterns`, the
/// cheapest cloud to read it from (`from`, if given).
pub fn plan_pull(
    root: &Path,
    manifest: &Manifest,
    clouds: &[Cloud],
    patterns: &[String],
) -> anyhow::Result<PullPlan> {
    let mut plan = PullPlan::default();
    let mut any = false;
    for (path, entry) in &manifest.files {
        if !patterns.iter().any(|p| matches(p, path)) {
            continue;
        }
        any = true;
        if !files::safe_path(path) || !files::parents_are_real(root, path) {
            plan.refused.push(path.clone());
            continue;
        }
        if unchanged_here(root, path, entry) && entry.clouds.values().any(|c| c.checked.is_some()) {
            plan.here.push(path.clone());
            continue;
        }
        let best = clouds
            .iter()
            .enumerate()
            .filter_map(|(i, cloud)| {
                let copy = entry.clouds.get(&cloud.name).filter(|c| !c.differs)?;
                let restore = copy.class.as_deref().is_some_and(needs_restore);
                Some(((restore, cloud.kind.read_order(), i), cloud, restore))
            })
            .min_by_key(|(order, ..)| *order);
        let Some((_, cloud, restore)) = best else {
            continue;
        };
        let list = if restore {
            &mut plan.restore
        } else {
            plan.bytes += entry.size;
            &mut plan.download
        };
        match list.iter_mut().find(|(c, _)| c.name == cloud.name) {
            Some((_, files)) => files.push(path.clone()),
            None => list.push((cloud.clone(), vec![path.clone()])),
        }
    }
    if !any {
        bail!(
            "no files in your clouds match {} (lnk storage list --tree shows what's there; --refresh lists the clouds again)",
            patterns.join(" ")
        );
    }
    Ok(plan)
}

fn unchanged_here(root: &Path, path: &str, entry: &Entry) -> bool {
    std::fs::symlink_metadata(root.join(path)).is_ok_and(|meta| {
        meta.is_file()
            && meta.len() == entry.size
            && files::seconds(meta.modified().ok()) == entry.modified
    })
}

/// `lnk storage pull`, second half: asks for archived files to be
/// restored (a no-op for those restored already) and tries to read them,
/// downloads the rest, and checks what arrived. Returns each cloud's
/// check.
pub fn apply_pull(
    engine: &dyn Engine,
    root: &Path,
    manifest: &mut Manifest,
    plan: &PullPlan,
    options: &Download,
) -> anyhow::Result<Vec<Checked>> {
    // Checked again right before: the plan may be old. The folder's
    // owner can't change it meanwhile (pull refuses while its owner runs).
    for (_, names) in plan.restore.iter().chain(&plan.download) {
        if let Some(name) = names.iter().find(|n| !files::parents_are_real(root, n)) {
            bail!("{name}: a folder on the way to it is now a link; nothing pulled");
        }
    }
    let mut reports = Vec::new();
    for (cloud, names) in &plan.restore {
        if let Err(err) = engine.restore(cloud, names) {
            crate::said::warn(
                link_format::sentence!("Restoring files in {} didn't work.", cloud.name.as_str()),
                &err,
            );
        }
        // Fails for those still archived; they're waiting, not lost.
        let _ = engine.download(cloud, root, names, options);
        let (mut report, absent) = arrived(engine, root, manifest, cloud, names)?;
        report.waiting = absent;
        reports.push(report);
    }
    for (cloud, names) in &plan.download {
        let failed = engine.download(cloud, root, names, options).err();
        let (mut report, absent) = arrived(engine, root, manifest, cloud, names)?;
        report.failed = failed.map(|e| e.to_string());
        for name in absent {
            report.errors.push((name, "didn't arrive".to_string()));
        }
        reports.push(report);
    }
    Ok(reports)
}

/// Checks which of `names` are here now against `cloud`, and which
/// aren't.
fn arrived(
    engine: &dyn Engine,
    root: &Path,
    manifest: &mut Manifest,
    cloud: &Cloud,
    names: &[String],
) -> anyhow::Result<(Checked, Vec<String>)> {
    let mut locals = Vec::new();
    let mut absent = Vec::new();
    for name in names {
        // Only the file itself: a folder of that name here isn't it.
        match files::walk(root, name)?
            .into_iter()
            .find(|local| &local.path == name)
        {
            Some(local) => locals.push(local),
            None => absent.push(name.clone()),
        }
    }
    // `record` looks them up by path.
    locals.sort_by(|a, b| a.path.cmp(&b.path));
    let present: Vec<String> = locals.iter().map(|l| l.path.clone()).collect();
    let results = engine.check(cloud, root, &present, Proof::Checksum)?;
    Ok((
        record(manifest, &cloud.name, &locals, &results, None),
        absent,
    ))
}
