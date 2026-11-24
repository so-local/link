//! Deleting from a cloud: never a file's last copy.

use super::*;

/// What `delete` would delete from which cloud.
#[derive(Debug, Default)]
pub struct RemovePlan {
    pub delete: Vec<(Cloud, Vec<String>)>,
    /// Files that would then have no copy anywhere, here or in a cloud.
    pub last_copies: Vec<String>,
    /// Archived copies, charged for their minimum storage period anyway.
    pub archived: Vec<(String, String)>,
}

/// `lnk storage remove`, first half. A copy left in another cloud counts
/// only if a listing of that cloud, now, has it at its recorded size: the
/// manifest alone could be stale.
pub fn plan_remove(
    engine: &dyn Engine,
    root: &Path,
    manifest: &Manifest,
    all_clouds: &[Cloud],
    from: &[Cloud],
    patterns: &[String],
) -> anyhow::Result<RemovePlan> {
    let mut plan = RemovePlan::default();
    let mut any = false;
    let mut listings: std::collections::HashMap<String, std::collections::HashMap<String, u64>> =
        Default::default();
    for (path, entry) in &manifest.files {
        if !patterns.iter().any(|p| matches(p, path)) {
            continue;
        }
        let targets: Vec<&Cloud> = from
            .iter()
            .filter(|c| entry.clouds.contains_key(&c.name))
            .collect();
        if targets.is_empty() {
            continue;
        }
        any = true;
        let here = files::safe_path(path)
            && files::parents_are_real(root, path)
            && std::fs::symlink_metadata(root.join(path)).is_ok_and(|m| m.is_file());
        let mut kept_elsewhere = false;
        for cloud in all_clouds {
            let Some(copy) = entry.clouds.get(&cloud.name) else {
                continue;
            };
            if targets.iter().any(|t| t.name == cloud.name) || copy.differs {
                continue;
            }
            if !listings.contains_key(&cloud.name) {
                let listed = engine
                    .list(cloud)?
                    .into_iter()
                    .map(|l| (l.path, l.size))
                    .collect();
                listings.insert(cloud.name.clone(), listed);
            }
            if listings[&cloud.name].get(path) == Some(&copy.size) {
                kept_elsewhere = true;
                break;
            }
        }
        if !here && !kept_elsewhere {
            plan.last_copies.push(path.clone());
        }
        for cloud in targets {
            if let Some(class) = entry.clouds[&cloud.name]
                .class
                .as_deref()
                .filter(|c| is_archive(c))
            {
                plan.archived
                    .push((cloud.name.clone(), format!("{path} ({class})")));
            }
            match plan.delete.iter_mut().find(|(c, _)| c.name == cloud.name) {
                Some((_, files)) => files.push(path.clone()),
                None => plan.delete.push((cloud.clone(), vec![path.clone()])),
            }
        }
    }
    if !any {
        bail!("no copies in those clouds match {}", patterns.join(" "));
    }
    Ok(plan)
}

/// `lnk storage remove`, second half.
pub fn apply_remove(
    engine: &dyn Engine,
    manifest: &mut Manifest,
    plan: &RemovePlan,
) -> anyhow::Result<()> {
    for (cloud, names) in &plan.delete {
        engine.delete(cloud, names)?;
        for name in names {
            if let Some(entry) = manifest.files.get_mut(name) {
                entry.clouds.remove(&cloud.name);
                if entry.clouds.is_empty() {
                    manifest.files.remove(name);
                }
            }
        }
    }
    Ok(())
}
