//! A push: the files to send, each sent to every cloud and checked, and
//! what the manifest records of it.

use super::*;

/// How a push or a check went on one cloud.
#[derive(Debug, Default)]
pub struct Checked {
    pub cloud: String,
    pub same: Vec<String>,
    pub missing: Vec<String>,
    pub differs: Vec<String>,
    pub errors: Vec<(String, String)>,
    /// Archived files whose restore hasn't finished (pull).
    pub waiting: Vec<String>,
    /// The engine's error, when sending failed for some files.
    pub failed: Option<String>,
    /// Files push skipped: unchanged here since their copy there was
    /// checked.
    pub unchanged: usize,
}

impl Checked {
    pub fn all_same(&self) -> bool {
        self.missing.is_empty() && self.differs.is_empty() && self.errors.is_empty()
    }
}

/// The files here under each of `paths` (in the bucket), without repeats.
pub fn locals(root: &Path, paths: &[String]) -> anyhow::Result<Vec<Local>> {
    let mut all: Vec<Local> = Vec::new();
    for path in paths {
        let found = files::walk(root, path)?;
        if found.is_empty() {
            let shown = if path.is_empty() { "the bucket" } else { path };
            bail!("nothing to do: {shown} has no files here");
        }
        all.extend(found);
    }
    all.sort_by(|a, b| a.path.cmp(&b.path));
    all.dedup_by(|a, b| a.path == b.path);
    Ok(all)
}

/// Records what a check found for `cloud`. `class` is the storage class
/// files were just sent with, for copies that are new.
/// `locals` is sorted by path.
pub(super) fn record(
    manifest: &mut Manifest,
    cloud: &str,
    locals: &[Local],
    results: &[(String, Found)],
    class: Option<&str>,
) -> Checked {
    let mut checked = Checked {
        cloud: cloud.to_string(),
        ..Checked::default()
    };
    let now = files::now();
    for (path, found) in results {
        let Ok(at) = locals.binary_search_by(|l| l.path.as_str().cmp(path)) else {
            continue;
        };
        let local = &locals[at];
        let entry = manifest.files.entry(path.clone()).or_default();
        let nanos = subsec(local);
        if entry.size != local.size
            || entry.modified != local.modified
            || (entry.nanos != 0 && entry.nanos != nanos)
        {
            // The file here changed: copies elsewhere were checked
            // against what it was, not what it is.
            for (name, copy) in entry.clouds.iter_mut() {
                if name != cloud {
                    copy.checked = None;
                    copy.proven = None;
                }
            }
        }
        entry.size = local.size;
        entry.modified = local.modified;
        entry.nanos = nanos;
        match found {
            Found::Same => {
                let old = entry.clouds.get(cloud);
                let class = match old {
                    Some(old) if old.checked.is_some() || old.class.is_some() => old.class.clone(),
                    _ => class.map(str::to_string),
                };
                entry.clouds.insert(
                    cloud.to_string(),
                    Copy {
                        size: local.size,
                        checked: Some(now),
                        differs: false,
                        class,
                        proven: None,
                    },
                );
                checked.same.push(path.clone());
            }
            Found::Missing => {
                entry.clouds.remove(cloud);
                checked.missing.push(path.clone());
            }
            Found::Differs => {
                let copy = entry.clouds.entry(cloud.to_string()).or_default();
                copy.differs = true;
                copy.checked = None;
                copy.proven = None;
                checked.differs.push(path.clone());
            }
            Found::Error(why) => checked.errors.push((path.clone(), why.clone())),
        }
        if entry.clouds.is_empty() {
            manifest.files.remove(path);
        }
    }
    checked
}

/// How `push` sends files.
#[derive(Debug, Clone, Default)]
pub struct Push {
    /// Replace copies that differ from the files here.
    pub replace: bool,
    /// Store them in each cloud's archive tier.
    pub archive: bool,
    /// More flags for the engine, as the user gave them.
    pub flags: Vec<String>,
    /// Send and check every file, even those unchanged since their copy
    /// was checked.
    pub all: bool,
}

/// The nanoseconds of `local`'s modification time within its second.
fn subsec(local: &Local) -> u32 {
    (local.stamp.nanos % 1_000_000_000) as u32
}

/// Whether `local` is as it was when its copy in `cloud` was checked, by
/// size and modification time to the nanosecond, so push can skip it.
fn unchanged(manifest: &Manifest, cloud: &str, local: &Local) -> bool {
    manifest.files.get(&local.path).is_some_and(|entry| {
        entry.size == local.size
            && entry.modified == local.modified
            && entry.modified != 0
            && entry.nanos == subsec(local)
            && entry.clouds.get(cloud).is_some_and(|copy| {
                copy.checked.is_some() && !copy.differs && copy.size == local.size
            })
    })
}

/// `lnk storage push`: sends the files under `paths` to each of `clouds`,
/// then checks what it sent and records what's there. A file unchanged
/// since its copy in a cloud was checked is skipped there, unless `all`.
pub fn push(
    engine: &dyn Engine,
    root: &Path,
    manifest: &mut Manifest,
    clouds: &[Cloud],
    paths: &[String],
    options: &Push,
) -> anyhow::Result<Vec<Checked>> {
    let locals = locals(root, paths)?;
    if options.archive
        && let Some(cloud) = clouds.iter().find(|c| c.kind.archive_class().is_none())
    {
        bail!(
            "{} has no archive tier: push --archive --cloud one of your aws or gcp clouds",
            cloud.name
        );
    }
    // Each cloud's files: those changed since their copy there was
    // checked, unless all.
    let mut reports = Vec::new();
    // Each part sent, and its report's place in `reports`.
    let mut sending = Vec::new();
    let mut places = Vec::new();
    for cloud in clouds {
        let names: Vec<String> = locals
            .iter()
            .filter(|l| options.all || !unchanged(manifest, &cloud.name, l))
            .map(|l| l.path.clone())
            .collect();
        let unchanged = locals.len() - names.len();
        reports.push(Checked {
            cloud: cloud.name.clone(),
            unchanged,
            ..Checked::default()
        });
        if names.is_empty() {
            continue;
        }
        let class = if options.archive {
            cloud.kind.archive_class()
        } else {
            None
        };
        places.push(reports.len() - 1);
        sending.push(Sending {
            cloud,
            files: names,
            upload: Upload {
                replace: options.replace,
                class: class.map(str::to_string),
                flags: options.flags.clone(),
            },
        });
    }
    let sent = engine.send_and_check(root, &sending);
    if sent.len() != sending.len() {
        bail!(
            "the engine answered for {} of {} clouds",
            sent.len(),
            sending.len()
        );
    }
    // Recorded one cloud after another, each into the same manifest, so
    // no cloud's record is lost; a cloud whose check failed records
    // nothing, and the first such failure is the push's.
    let mut error = None;
    for ((at, one), sent) in places.into_iter().zip(&sending).zip(sent) {
        let results = match sent.checked {
            Ok(results) => results,
            Err(err) => {
                error.get_or_insert(err);
                continue;
            }
        };
        let class = one.upload.class.as_deref();
        let mut report = record(manifest, &one.cloud.name, &locals, &results, class);
        report.failed = sent.failed.map(|e| e.to_string());
        report.unchanged = reports[at].unchanged;
        reports[at] = report;
    }
    match error {
        Some(err) => Err(err),
        None => Ok(reports),
    }
}
