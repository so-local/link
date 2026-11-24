//! A control group's counters, cgroup v2's files read as they are: what
//! the measure module samples an agent's group with, and what a job's
//! profile samples the job's. A file not there (its controller off, or
//! the group gone mid-read) gives none, never zero; a machine with no
//! control groups, as a Mac, gives none of each.

use std::path::Path;

/// Every process in control group `group` and the groups under it.
pub fn group_pids(group: &Path) -> Vec<u32> {
    let mut pids = Vec::new();
    let mut todo = vec![group.to_path_buf()];
    while let Some(dir) = todo.pop() {
        pids.extend(
            std::fs::read_to_string(dir.join("cgroup.procs"))
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|p| p.parse::<u32>().ok()),
        );
        if let Ok(entries) = std::fs::read_dir(&dir) {
            todo.extend(
                entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.path()),
            );
        }
    }
    pids
}

/// The CPU a control group has used, every process it ever held, in
/// core-seconds.
pub fn group_cpu(group: &Path) -> Option<f64> {
    std::fs::read_to_string(group.join("cpu.stat"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("usage_usec ")?.trim().parse::<u64>().ok())
        .map(|usec| usec as f64 / 1e6)
}

/// Bytes a control group has read from disk and written, every device's.
pub fn group_io(group: &Path) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(group.join("io.stat")).ok()?;
    let (mut read, mut written) = (0, 0);
    for field in text.split_whitespace() {
        if let Some(n) = field.strip_prefix("rbytes=") {
            read += n.parse::<u64>().unwrap_or(0);
        } else if let Some(n) = field.strip_prefix("wbytes=") {
            written += n.parse::<u64>().unwrap_or(0);
        }
    }
    Some((read, written))
}

/// The memory a control group uses now, in bytes (`memory.current`).
pub fn group_memory(group: &Path) -> Option<u64> {
    number(&group.join("memory.current"))
}

/// The most memory a control group has used, in bytes (`memory.peak`).
pub fn group_memory_peak(group: &Path) -> Option<u64> {
    number(&group.join("memory.peak"))
}

/// The control group process `pid` is in, from `/proc/<pid>/cgroup`'s
/// v2 line, under `root` (`/sys/fs/cgroup`).
pub fn of_process(pid: u32, root: &Path) -> Option<std::path::PathBuf> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let path = text.lines().find_map(|l| l.strip_prefix("0::"))?;
    Some(root.join(path.trim().trim_start_matches('/')))
}

fn number(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(files: &[(&str, &str)]) -> link_test_kit::TempDir {
        let dir = link_test_kit::TempDir::new("measure-group");
        std::fs::create_dir_all(dir.join("child")).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    /// Each reader over a group's files, its children's processes too.
    #[test]
    fn each_counter_is_read_from_its_file() {
        let dir = group(&[
            ("cgroup.procs", "10\n11\n"),
            ("child/cgroup.procs", "12\n"),
            ("cpu.stat", "usage_usec 2500000\nuser_usec 1\n"),
            (
                "io.stat",
                "8:0 rbytes=100 wbytes=20 rios=1\n8:16 rbytes=1 wbytes=2\n",
            ),
            ("memory.current", "4096\n"),
            ("memory.peak", "8192\n"),
        ]);
        let mut pids = group_pids(&dir);
        pids.sort();
        assert_eq!(pids, [10, 11, 12]);
        assert_eq!(group_cpu(&dir), Some(2.5));
        assert_eq!(group_io(&dir), Some((101, 22)));
        assert_eq!(group_memory(&dir), Some(4096));
        assert_eq!(group_memory_peak(&dir), Some(8192));
    }

    /// A controller that's off, or a group gone mid-read, gives none of
    /// what it would count, never zero.
    #[test]
    fn a_missing_file_gives_none() {
        let dir = group(&[("cpu.stat", "usage_usec 1000000\n")]);
        assert_eq!(group_io(&dir), None);
        assert_eq!(group_memory(&dir), None);
        assert_eq!(group_memory_peak(&dir), None);
        std::fs::remove_dir_all(&*dir).unwrap();
        assert_eq!(group_cpu(&dir), None);
        assert!(group_pids(&dir).is_empty());
    }
}
