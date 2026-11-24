//! What's used on disk: a folder's bytes and files ([`walk`]), a
//! filesystem's, and a Mac's memory as `vm_stat` says.

use super::*;

/// The bytes on disk and the files under `folder`, links not followed,
/// counting at most `most` entries.
pub(super) fn walk(folder: &Path, most: u64) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let (mut used, mut files, mut seen) = (0, 0, 0u64);
    let mut todo = vec![folder.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > most {
                return (used, files);
            }
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            if meta.is_dir() {
                todo.push(entry.path());
            } else {
                files += 1;
                used += meta.blocks() * 512;
            }
        }
    }
    (used, files)
}

/// What the filesystem holding `path` has in use.
pub fn filesystem_used(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: zeroed is a valid statvfs, which the call fills.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let blocks = (stat.f_blocks as u64).saturating_sub(stat.f_bfree as u64);
    Some(blocks * stat.f_frsize as u64)
}

/// Memory in use from macOS's `vm_stat`: anonymous, wired and compressed
/// pages, but purgeable ones, at its page size.
pub fn vm_stat_used(vm_stat: &str) -> Option<u64> {
    let page: u64 = vm_stat
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |name: &str| -> u64 {
        vm_stat
            .lines()
            .find_map(|l| {
                let rest = l.strip_prefix(name)?.strip_prefix(':')?;
                rest.trim().trim_end_matches('.').parse::<u64>().ok()
            })
            .unwrap_or(0)
    };
    let used = pages("Anonymous pages")
        + pages("Pages wired down")
        + pages("Pages occupied by compressor");
    Some(used.saturating_sub(pages("Pages purgeable")) * page)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_stat_is_read() {
        let out = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                   Pages free:                               10.\n\
                   Pages purgeable:                           2.\n\
                   Pages wired down:                          3.\n\
                   Anonymous pages:                           5.\n\
                   Pages occupied by compressor:              4.\n";
        assert_eq!(vm_stat_used(out), Some(10 * 16384));
        assert_eq!(vm_stat_used("nothing"), None);
    }
}
