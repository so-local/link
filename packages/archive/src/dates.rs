//! Deleted things' archives, and the day each was deleted: only they
//! expire.

use std::path::{Path, PathBuf};

/// The day `date` (`2026-11-10`) is, counted from 1970-01-01; none when
/// it isn't a date.
pub fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if date.len() != 10 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Whether `stem` is the name a deletion on day `deleted` gave an archive
/// of `manifest`'s one thing: `<name>-<date>`, or `<name>-<date>-<n>`.
fn as_deleted(stem: &str, manifest: &crate::Manifest, deleted: &str) -> bool {
    let [thing] = manifest.things.keys().collect::<Vec<_>>()[..] else {
        return false;
    };
    let Ok(thing) = crate::Selection::parse(thing) else {
        return false;
    };
    let base = format!("{}-{deleted}", thing.name());
    stem == base
        || stem
            .strip_prefix(&format!("{base}-"))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The archives in `dir` a deletion made more than `keep_days` before
/// `today` (`2026-11-10`), as each one's `archive.toml` says, and still
/// under the name the deletion gave it, `<name>-<date>.tar.gz` (or
/// `-<n>` after it). An archive you exported, renamed or put there
/// yourself never expires; one that can't be read is left alone.
pub fn expired(dir: &Path, keep_days: u64, today: &str) -> Vec<PathBuf> {
    let Some(today) = day_number(today) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut gone: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|path| path.to_string_lossy().ends_with(".tar.gz"))
        .filter(|path| {
            let Some(manifest) = crate::read_manifest(path).ok() else {
                return false;
            };
            let Some(deleted) = manifest.deleted.as_deref() else {
                return false;
            };
            let named = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".tar.gz"))
                .is_some_and(|stem| as_deleted(stem, &manifest, deleted));
            named && day_number(deleted).is_some_and(|day| today - day > keep_days as i64)
        })
        .collect();
    gone.sort();
    gone
}
