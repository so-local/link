//! A clean: the local copies it may delete, each proven on every cloud
//! first, and deleting them.

use super::*;

/// What `clean` would delete, after checking every cloud.
#[derive(Debug)]
pub struct CleanPlan {
    pub files: Vec<Local>,
    /// Each cloud's check; the plan is safe only if every one is all the same.
    pub checks: Vec<Checked>,
}

impl CleanPlan {
    /// Every cloud checked, and each found every file the same: not only
    /// nothing wrong, but nothing left out.
    pub fn safe(&self) -> bool {
        !self.checks.is_empty()
            && self.checks.iter().all(|check| {
                let same: std::collections::HashSet<&str> =
                    check.same.iter().map(String::as_str).collect();
                check.all_same()
                    && self
                        .files
                        .iter()
                        .all(|file| same.contains(file.path.as_str()))
            })
    }

    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// The deepest folder every path in `paths` (in the bucket, with `/`)
/// is in: "" for the bucket itself.
fn common_folder<'a>(paths: impl Iterator<Item = &'a str>) -> String {
    let mut common: Option<Vec<&str>> = None;
    for path in paths {
        let folder: Vec<&str> = path.split('/').collect();
        let folder = &folder[..folder.len().saturating_sub(1)];
        common = Some(match common {
            None => folder.to_vec(),
            Some(common) => common
                .iter()
                .zip(folder)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    common.unwrap_or_default().join("/")
}

/// At most this many folders a clean lists apart in a cloud; past it, the
/// one folder all its files are in.
const CLEAN_FOLDERS: usize = 8;

/// The folders a clean of `paths` (which found `files`) lists in a cloud:
/// for each path, the folder its files share, leaving out any inside
/// another. The folder every file shares instead, when that would be more
/// than [`CLEAN_FOLDERS`] or leave a file out.
fn clean_folders(paths: &[String], files: &[Local]) -> Vec<String> {
    let inside = |path: &str, folder: &str| {
        folder.is_empty()
            || path
                .strip_prefix(folder)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    let mut folders: Vec<String> = paths
        .iter()
        .map(|asked| {
            let asked = asked.trim_end_matches('/');
            common_folder(
                files
                    .iter()
                    .map(|l| l.path.as_str())
                    .filter(|path| *path == asked || inside(path, asked)),
            )
        })
        .collect();
    folders.sort_by_key(String::len);
    folders.dedup();
    let mut kept: Vec<String> = Vec::new();
    for folder in folders {
        if !kept.iter().any(|k| folder == *k || inside(&folder, k)) {
            kept.push(folder);
        }
    }
    let covered = files
        .iter()
        .all(|l| kept.iter().any(|folder| inside(&l.path, folder)));
    if kept.is_empty() || kept.len() > CLEAN_FOLDERS || !covered {
        return vec![common_folder(files.iter().map(|l| l.path.as_str()))];
    }
    kept
}

/// [`Engine::written`] for each of `folders`, together.
fn written_in(
    engine: &dyn Engine,
    cloud: &Cloud,
    folders: &[String],
) -> anyhow::Result<Option<std::collections::HashMap<String, Written>>> {
    let mut all: Option<std::collections::HashMap<String, Written>> = None;
    for folder in folders {
        match engine.written(cloud, folder)? {
            Some(listed) => all.get_or_insert_with(Default::default).extend(listed),
            None => return Ok(None),
        }
    }
    Ok(all)
}

/// One cloud's part of a clean: what it found of each file, its copies
/// as listed before the check (none where nothing is read back), and the
/// files checked now whose proof isn't kept: an upload in parts was
/// begun on their name when the check ended, or that couldn't be told.
type CleanCheck = (
    Vec<(String, Found)>,
    Option<std::collections::HashMap<String, Written>>,
    std::collections::HashSet<String>,
);

/// `lnk storage clean`, first half: checks every one of `clouds` now,
/// and records what it found. On S3 a copy an earlier clean proved,
/// unchanged here and there since, isn't read back again.
pub fn plan_clean(
    engine: &dyn Engine,
    root: &Path,
    manifest: &mut Manifest,
    clouds: &[Cloud],
    paths: &[String],
) -> anyhow::Result<CleanPlan> {
    if clouds.is_empty() {
        bail!("no clouds connected, so nothing is safe to delete");
    }
    let files = locals(root, paths)?;
    let under = clean_folders(paths, &files);
    // Each cloud on its own, side by side: its listings and its check.
    let found: Vec<std::sync::Mutex<Option<anyhow::Result<CleanCheck>>>> =
        clouds.iter().map(|_| Default::default()).collect();
    let seen: &Manifest = manifest;
    engine.each_cloud(clouds, &|engine, i| {
        let checked = check_to_clean(engine, root, seen, &clouds[i], &files, &under);
        if let Ok(mut slot) = found[i].lock() {
            *slot = Some(checked);
        }
    });
    let mut checks = Vec::new();
    for (cloud, found) in clouds.iter().zip(found) {
        let (results, before, pending) =
            found.into_inner().ok().flatten().unwrap_or_else(|| {
                Err(anyhow::anyhow!(
                    "checking {} stopped unexpectedly",
                    cloud.name
                ))
            })?;
        checks.push(record(manifest, &cloud.name, &files, &results, None));
        // What was proved now, kept for the next clean.
        for (path, found) in &results {
            let Ok(at) = files.binary_search_by(|l| l.path.as_str().cmp(path)) else {
                continue;
            };
            let local = &files[at];
            let Some(copy) = manifest
                .files
                .get_mut(path)
                .and_then(|entry| entry.clouds.get_mut(&cloud.name))
            else {
                continue;
            };
            // A copy with an upload in parts begun on its name, which
            // would take the second it began when it finished (as early as
            // the copy proved), is proved again next time.
            copy.proven = match (found, before.as_ref().and_then(|b| b.get(path))) {
                (Found::Same, Some(written)) if !pending.contains(path) => Some(Proven {
                    size: local.size,
                    inode: local.stamp.inode,
                    nanos: u64::try_from(local.stamp.nanos).unwrap_or(u64::MAX),
                    changed: u64::try_from(local.stamp.changed).unwrap_or(u64::MAX),
                    copy: written.clone(),
                }),
                _ => None,
            };
        }
    }
    Ok(CleanPlan { files, checks })
}

/// `at` in UTC to the second, as S3 writes a time
/// (`2026-10-11T00:31:53`).
fn utc_second(at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970 as a civil date (Howard Hinnant's).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// [`plan_clean`]'s check of `files` in `cloud`, with `engine`. Each copy
/// is listed first where a check reads copies back, from the folders
/// `under` (each path's own, as [`clean_folders`] chooses): one proved before, unchanged here and there
/// since, isn't read again. Listed again after, when any was checked: a
/// copy written while it was checked may not be the one checked.
fn check_to_clean(
    engine: &dyn Engine,
    root: &Path,
    manifest: &Manifest,
    cloud: &Cloud,
    files: &[Local],
    under: &[String],
) -> anyhow::Result<CleanCheck> {
    let before = written_in(engine, cloud, under)?;
    let (proven, wanted): (Vec<&Local>, Vec<&Local>) = files.iter().partition(|local| {
        before.as_ref().is_some_and(|before| {
            still_proven(manifest, &cloud.name, local, before.get(&local.path))
        })
    });
    let wanted: Vec<String> = wanted.into_iter().map(|l| l.path.clone()).collect();
    // Junk finishing an upload in parts begun the second the copy was
    // written would list as the copy does. Asked before the check: one
    // finished before it is read back as junk; one still open, or begun
    // since in a second the copy may have been written in, keeps the copy
    // from being proved.
    let mut pending = std::collections::HashSet::new();
    if let Some(before) = before.as_ref().filter(|_| !wanted.is_empty()) {
        let asked = utc_second(std::time::SystemTime::now());
        pending = engine
            .pending(cloud, &wanted)?
            .unwrap_or_else(|| wanted.iter().cloned().collect());
        pending.extend(
            wanted
                .iter()
                .filter(|path| {
                    before.get(path.as_str()).is_some_and(|written| {
                        written.at.get(..19).is_some_and(|at| at >= asked.as_str())
                    })
                })
                .cloned(),
        );
    }
    let mut results = engine.check(cloud, root, &wanted, Proof::ToDelete)?;
    if let Some(before) = before.as_ref().filter(|_| !wanted.is_empty()) {
        let after = written_in(engine, cloud, under)?.unwrap_or_default();
        for (path, found) in &mut results {
            if *found == Found::Same
                && (before.get(path.as_str()).is_none()
                    || before.get(path.as_str()) != after.get(path.as_str()))
            {
                *found = Found::Error(format!(
                    "its copy in {} was written while it was checked: check again",
                    cloud.name
                ));
            }
        }
    }
    results.extend(proven.into_iter().map(|l| (l.path.clone(), Found::Same)));
    Ok((results, before, pending))
}

/// Whether `local`'s copy in `cloud` was proved the same by an earlier
/// clean, with the file here and the copy (`now`, as its cloud lists it)
/// both as they were then.
fn still_proven(manifest: &Manifest, cloud: &str, local: &Local, now: Option<&Written>) -> bool {
    let Some(now) = now else {
        return false;
    };
    manifest
        .files
        .get(&local.path)
        .and_then(|entry| entry.clouds.get(cloud))
        .filter(|copy| copy.checked.is_some() && !copy.differs)
        .and_then(|copy| copy.proven.as_ref())
        .is_some_and(|proven| {
            local.stamp.inode != 0
                && proven.size == local.size
                && proven.inode == local.stamp.inode
                && u128::from(proven.nanos) == local.stamp.nanos
                && proven.changed != 0
                && u128::from(proven.changed) == local.stamp.changed
                && proven.copy == *now
                && now.size == local.size
        })
}

/// `lnk storage clean`, second half: deletes the local copies, only if
/// the plan is safe. A file that changed since the check (or was replaced,
/// or is now reached through a link) is kept. Returns what was kept.
pub fn apply_clean(root: &Path, plan: &CleanPlan) -> anyhow::Result<Vec<String>> {
    if !plan.safe() {
        bail!("not every cloud has a checked copy of every file; nothing deleted");
    }
    let mut kept = Vec::new();
    let mut deleted = Vec::new();
    for file in &plan.files {
        let path = root.join(&file.path);
        let unchanged = files::parents_are_real(root, &file.path)
            && std::fs::symlink_metadata(&path).is_ok_and(|meta| {
                meta.is_file() && meta.len() == file.size && files::Stamp::of(&meta) == file.stamp
            });
        if !unchanged {
            kept.push(file.path.clone());
            continue;
        }
        std::fs::remove_file(&path)?;
        deleted.push(file.path.as_str());
    }
    files::remove_empty_parents(root, &deleted);
    Ok(kept)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_clean_lists_only_the_folder_its_files_share() {
        let common = |paths: &[&str]| super::common_folder(paths.iter().copied());
        assert_eq!(common(&["photos/2019/a.jpg"]), "photos/2019");
        assert_eq!(
            common(&["photos/2019/a.jpg", "photos/2020/b.jpg"]),
            "photos"
        );
        assert_eq!(common(&["photos/a.jpg", "notes.txt"]), "");
        assert_eq!(common(&["photosx/a", "photos/b"]), "");
        assert_eq!(common(&[]), "");
    }

    #[test]
    fn a_clean_lists_each_path_s_own_folder() {
        let folders = |asked: &[&str], found: &[&str]| {
            let files: Vec<super::Local> = found
                .iter()
                .map(|path| super::Local {
                    path: path.to_string(),
                    size: 0,
                    modified: 0,
                    stamp: Default::default(),
                })
                .collect();
            let asked: Vec<String> = asked.iter().map(|p| p.to_string()).collect();
            super::clean_folders(&asked, &files)
        };
        // Two folders apart: each listed, not the whole bucket.
        assert_eq!(
            folders(
                &["photos/2019", "docs/old.txt"],
                &["docs/old.txt", "photos/2019/a.jpg", "photos/2019/b/c.jpg"]
            ),
            ["docs", "photos/2019"]
        );
        // One inside another: only the outer one.
        assert_eq!(
            folders(
                &["photos/2019", "photos"],
                &["photos/2019/a.jpg", "photos/x.jpg"]
            ),
            ["photos"]
        );
        // A file at the top, or the whole bucket: the bucket.
        assert_eq!(
            folders(&["photos", "notes.txt"], &["notes.txt", "photos/a.jpg"]),
            [""]
        );
        // A path the files aren't named under: all their common folder.
        assert_eq!(folders(&["./photos"], &["photos/a.jpg"]), [""]);
        // More than CLEAN_FOLDERS: their common folder, once.
        let many: Vec<String> = (0..=super::CLEAN_FOLDERS)
            .map(|i| format!("p/{i}/f"))
            .collect();
        let found: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(folders(&found, &found), ["p"]);
    }
}
