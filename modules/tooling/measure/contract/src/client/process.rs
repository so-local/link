//! A process now, as Linux's `/proc` says it, read as this user: what the
//! measure module samples processes with, and what a job's profile sums
//! when the job has no control group of its own.

#[cfg(target_os = "linux")]
use std::path::Path;

/// A process now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Its program's name, as the system keeps it: whatever it chose.
    pub name: String,
    /// When it started, in nanoseconds since 1970.
    pub started: Option<u64>,
    /// In use: PSS on Linux, the footprint on a Mac; none when not read
    /// at this look.
    pub memory: Option<u64>,
    /// Core-seconds, with its children it waited for.
    pub cpu: f64,
    pub read: u64,
    pub written: u64,
}

/// Every process under `roots`, `roots` among them, as `alive` and
/// `children` find them: this machine's own readers, Linux's or a Mac's.
pub fn tree(
    roots: &[u32],
    alive: impl Fn(u32) -> bool,
    children: impl Fn(u32) -> Vec<u32>,
) -> Vec<u32> {
    let (mut seen, mut todo) = (Vec::new(), roots.to_vec());
    while let Some(pid) = todo.pop() {
        if seen.contains(&pid) || !alive(pid) {
            continue;
        }
        seen.push(pid);
        todo.extend(children(pid));
    }
    seen
}

#[cfg(target_os = "linux")]
fn read(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn page() -> u64 {
    // SAFETY: sysconf reads a constant of the system.
    u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).unwrap_or(4096)
}

/// The clock ticks a second `/proc` counts CPU in.
#[cfg(target_os = "linux")]
pub fn ticks() -> f64 {
    // SAFETY: as above.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 { ticks as f64 } else { 100.0 }
}

/// `/proc/<pid>/stat`'s fields after the name, from its state on.
#[cfg(target_os = "linux")]
fn stat(pid: u32) -> Vec<String> {
    stat_at(Path::new(&format!("/proc/{pid}/stat")))
}

#[cfg(target_os = "linux")]
fn stat_at(path: &Path) -> Vec<String> {
    let line = read(path);
    match line.rfind(')') {
        Some(end) => line[end + 1..]
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    }
}

/// Whether process `pid` runs.
#[cfg(target_os = "linux")]
pub fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// The kernel's current process state, including `Z` for an exited
/// process awaiting its parent. Unreadable state remains unavailable.
#[cfg(target_os = "linux")]
pub fn state(pid: u32) -> Option<char> {
    stat(pid).first()?.chars().next()
}

/// Whether an owned child exited, without reaping its final counters.
/// This is only for callers that own the child and will subsequently wait it.
#[cfg(target_os = "linux")]
pub fn exited_without_reaping(pid: u32) -> std::io::Result<bool> {
    // SAFETY: waitid fills this output for the caller's child. WNOWAIT leaves
    // the child available to its owner's normal wait and to the final sample.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        loop {
            if libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            ) == 0
            {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        Ok(info.si_pid() != 0)
    }
}

/// Process `pid` now, if it runs and is this user's to read; its memory
/// only when `memory`, which walks every mapping it has.
#[cfg(target_os = "linux")]
pub fn process(pid: u32, memory: bool) -> Option<Proc> {
    let fields = stat(pid);
    if fields.len() < 20 {
        return None;
    }
    let number = |i: usize| fields[i].parse::<u64>().unwrap_or(0);
    let cpu = (number(11) + number(12) + number(13) + number(14)) as f64 / ticks();
    let started =
        crate::client::booted().map(|boot| boot + (number(19) as f64 / ticks() * 1e9) as u64);
    let memory = memory.then(|| {
        let rollup = read(format!("/proc/{pid}/smaps_rollup"));
        let pss = rollup.lines().find_map(|l| {
            let kb = l.strip_prefix("Pss:")?.trim().strip_suffix("kB")?;
            kb.trim().parse::<u64>().ok()
        });
        // Its resident memory where PSS can't be read.
        match pss {
            Some(kb) => kb * 1024,
            None => {
                read(format!("/proc/{pid}/statm"))
                    .split_whitespace()
                    .nth(1)
                    .and_then(|p| p.parse::<u64>().ok())
                    .unwrap_or(0)
                    * page()
            }
        }
    });
    let io = read(format!("/proc/{pid}/io"));
    let io_field = |name: &str| {
        io.lines()
            .find_map(|l| l.strip_prefix(name)?.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    Some(Proc {
        pid,
        parent: fields[1].parse().unwrap_or(0),
        name: read(format!("/proc/{pid}/comm")).trim().to_string(),
        started,
        memory,
        cpu,
        read: io_field("read_bytes:"),
        written: io_field("write_bytes:"),
    })
}

/// `pid`'s children.
#[cfg(target_os = "linux")]
pub fn children(pid: u32) -> Vec<u32> {
    children_at(Path::new("/proc"), pid)
}

#[cfg(target_os = "linux")]
fn children_at(root: &Path, pid: u32) -> Vec<u32> {
    Children::new(root).of(pid)
}

/// One observation shares its fallback inventory across every ancestor.
/// Kernel task lists remain the fast path; an exited process has no live tree.
#[cfg(target_os = "linux")]
pub fn live_tree(roots: &[u32]) -> Vec<u32> {
    let children = Children::new(Path::new("/proc"));
    tree(
        roots,
        |pid| state(pid).is_some_and(|state| state != 'Z'),
        |pid| children.of(pid),
    )
}

#[cfg(target_os = "linux")]
struct Children<'a> {
    root: &'a Path,
    parents: std::cell::OnceCell<std::collections::BTreeMap<u32, Vec<u32>>>,
}

#[cfg(target_os = "linux")]
impl<'a> Children<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            parents: std::cell::OnceCell::new(),
        }
    }

    fn of(&self, pid: u32) -> Vec<u32> {
        let root = self.root;
        let mut found = Vec::new();
        let mut complete = false;
        if let Ok(tasks) = std::fs::read_dir(root.join(pid.to_string()).join("task")) {
            complete = true;
            for task in tasks.flatten() {
                match std::fs::read_to_string(task.path().join("children")) {
                    Ok(children) => found.extend(
                        children
                            .split_whitespace()
                            .filter_map(|c| c.parse::<u32>().ok()),
                    ),
                    Err(_) => complete = false,
                }
            }
        }
        // The per-thread lists require CONFIG_CHECKPOINT_RESTORE. Some Linux
        // kernels omit them; parent IDs in stat remain available there.
        if !complete {
            found.extend(
                self.parents
                    .get_or_init(|| parent_inventory(root))
                    .get(&pid)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
        }
        found.sort_unstable();
        found.dedup();
        found
    }
}

#[cfg(target_os = "linux")]
fn parent_inventory(root: &Path) -> std::collections::BTreeMap<u32, Vec<u32>> {
    let mut parents = std::collections::BTreeMap::<u32, Vec<u32>>::new();
    if let Ok(processes) = std::fs::read_dir(root) {
        for process in processes.flatten() {
            let Some(child) = process
                .file_name()
                .to_str()
                .and_then(|p| p.parse::<u32>().ok())
            else {
                continue;
            };
            // Parent discovery needs two borrowed fields, not an allocated
            // String for every CPU, address and counter in every process.
            let stat = read(process.path().join("stat"));
            if let Some(parent) = stat
                .rsplit_once(')')
                .and_then(|(_, fields)| fields.split_whitespace().nth(1))
                .and_then(|parent| parent.parse::<u32>().ok())
            {
                parents.entry(parent).or_default().push(child);
            }
        }
    }
    parents
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn fallback_inventory_is_shared_within_an_observation_and_refreshed_next_time() {
        let root = link_test_kit::TempDir::new("measure-parent-snapshot");
        for (pid, parent) in [(10, 1), (20, 10), (30, 20)] {
            let folder = root.join(pid.to_string());
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join("stat"),
                format!("{pid} (child) S {parent} 0 0\n"),
            )
            .unwrap();
        }
        let observation = Children::new(&root);
        assert_eq!(observation.of(10), [20]);
        std::fs::write(root.join("30/stat"), "30 (child) S 10 0 0\n").unwrap();
        assert_eq!(
            observation.of(20),
            [30],
            "the same inventory serves the next ancestor"
        );
        assert_eq!(
            Children::new(&root).of(10),
            [20, 30],
            "the next observation reads fresh parentage"
        );
    }

    #[test]
    fn children_are_found_when_the_kernel_omits_its_per_thread_lists() {
        let root = link_test_kit::TempDir::new("measure-parent-ids");
        for (pid, parent, state) in [(10, 1, 'S'), (20, 10, 'S'), (21, 10, 'Z'), (30, 20, 'S')] {
            let dir = root.join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("stat"),
                format!("{pid} (name with ) brackets) {state} {parent} 0 0\n"),
            )
            .unwrap();
        }
        let malformed = root.join("31");
        std::fs::create_dir(&malformed).unwrap();
        std::fs::write(malformed.join("stat"), "31 (bad) S not-a-parent\n").unwrap();
        let task = root.join("10/task/10");
        std::fs::create_dir_all(&task).unwrap();
        assert_eq!(children_at(&root, 10), [20, 21]);
        assert_eq!(children_at(&root, 20), [30]);
        assert!(children_at(&root, 99).is_empty());
        // An available, empty list is authoritative and needs no full scan.
        std::fs::write(task.join("children"), "").unwrap();
        assert!(children_at(&root, 10).is_empty());
        // Missing one thread's list still discovers all children once.
        std::fs::write(task.join("children"), "20 20\n").unwrap();
        std::fs::create_dir_all(root.join("10/task/11")).unwrap();
        assert_eq!(children_at(&root, 10), [20, 21]);
    }

    #[test]
    fn this_process_is_read() {
        let me = process(std::process::id(), true).unwrap();
        assert!(me.memory > Some(0));
        assert_eq!(me.parent, std::os::unix::process::parent_id());
        assert!(!me.name.is_empty());
        assert!(me.started.is_some_and(|s| s <= crate::client::now()));
        assert_eq!(process(std::process::id(), false).unwrap().memory, None);
        assert!(alive(std::process::id()));
        assert!(!alive(u32::MAX));
    }
}
