//! Machine capacity and the resources available to this execution. Unknown
//! observations remain absent; host capacity and container limits are separate.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cpu {
    pub model: Option<String>,
    pub logical_cores: Option<u64>,
    pub physical_cores: Option<u64>,
    pub affinity_cores: Option<u64>,
    pub parallelism_cores: Option<u64>,
    pub quota_cores: Option<f64>,
    pub available_cores: Option<f64>,
    pub base_hz: Option<u64>,
    pub observed_hz: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    pub machine_total_bytes: Option<u64>,
    pub machine_available_bytes: Option<u64>,
    pub limit_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filesystem {
    /// A caller's fixed label, never file contents or a folder walk.
    pub label: String,
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    /// Space this user can allocate, excluding reserved blocks.
    pub available_bytes: Option<u64>,
    pub read_only: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub id: String,
    pub unix_ns: u64,
    pub os: String,
    pub architecture: String,
    pub kernel: Option<String>,
    pub cpu: Cpu,
    pub memory: Memory,
    pub filesystems: Vec<Filesystem>,
}

impl Snapshot {
    /// Observe OS resource interfaces only. No installation, inventory query,
    /// directory traversal, credentials or network access is involved.
    pub fn capture(paths: &[(&str, &Path)]) -> Self {
        #[cfg(target_os = "linux")]
        let (cpu, memory) = linux::capture(
            Path::new("/proc"),
            Path::new("/sys"),
            affinity(),
            std::thread::available_parallelism()
                .ok()
                .map(|n| n.get() as u64),
        );
        #[cfg(target_os = "macos")]
        let (cpu, memory) = mac::capture();
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let (cpu, memory) = (Cpu::default(), Memory::default());
        Self {
            version: 1,
            id: identity(),
            unix_ns: crate::now(),
            os: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            kernel: kernel(),
            cpu,
            memory,
            filesystems: paths
                .iter()
                .map(|(label, path)| filesystem(label, path))
                .collect(),
        }
    }
}

/// A machine fingerprint, independent of the profile or current user.
/// Cloned machines must have distinct OS identities or hostnames.
pub fn identity() -> String {
    let os_id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    #[cfg(unix)]
    let host = unsafe {
        // SAFETY: uname writes this initialized fixed-size output.
        let mut value: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut value) == 0 {
            std::ffi::CStr::from_ptr(value.nodename.as_ptr())
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        }
    };
    #[cfg(not(unix))]
    let host = String::new();
    link_digest::sha256(format!("{}\0{}", os_id.trim(), host).as_bytes())
}

#[cfg(target_os = "linux")]
fn affinity() -> Option<u64> {
    // SAFETY: sched_getaffinity fills this process's correctly sized mask.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        (libc::sched_getaffinity(0, std::mem::size_of_val(&set), &mut set) == 0)
            .then(|| libc::CPU_COUNT(&set) as u64)
    }
}

fn kernel() -> Option<String> {
    #[cfg(unix)]
    {
        // SAFETY: uname fills its output, including a NUL-terminated release.
        unsafe {
            let mut value: libc::utsname = std::mem::zeroed();
            if libc::uname(&mut value) != 0 {
                return None;
            }
            Some(
                std::ffi::CStr::from_ptr(value.release.as_ptr())
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn filesystem(label: &str, path: &Path) -> Filesystem {
    let mut out = Filesystem {
        label: label.into(),
        ..Filesystem::default()
    };
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return out;
        };
        // SAFETY: statvfs fills an initialized output for a valid C string.
        unsafe {
            let mut value: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(path.as_ptr(), &mut value) == 0 {
                let block = value.f_frsize as u64;
                out.total_bytes = (value.f_blocks as u64).checked_mul(block);
                out.free_bytes = (value.f_bfree as u64).checked_mul(block);
                out.available_bytes = (value.f_bavail as u64).checked_mul(block);
                out.read_only = Some(value.f_flag & libc::ST_RDONLY != 0);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_filesystems_and_unavailable_paths_keep_their_own_observations() {
        let dir = link_test_kit::TempDir::new("machine-capacity");
        let missing = dir.join("missing");
        let got = Snapshot::capture(&[("fixture", &dir), ("missing", &missing)]);
        assert!(
            got.filesystems[0]
                .total_bytes
                .is_some_and(|bytes| bytes > 0)
        );
        assert!(got.filesystems[0].available_bytes <= got.filesystems[0].free_bytes);
        assert_eq!(got.filesystems[1].available_bytes, None);
        assert!(got.unix_ns > 0);
        assert_eq!(
            serde_json::from_slice::<Snapshot>(&serde_json::to_vec(&got).unwrap()).unwrap(),
            got
        );
    }
}
