//! What Link sets up for the probes to reach for.

use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::Stdio;

use super::DotFiles;

/// What Link sets up for the probes to reach for.
pub struct Bait {
    pub home: PathBuf,
    /// The folders the policy may write.
    pub writable: Vec<PathBuf>,
    pub secret: String,
    /// A file in the home folder holding the secret.
    pub canary: PathBuf,
    /// Where a probe tries to write in the home folder.
    pub write: PathBuf,
    /// A file with the secret in Link's settings folder.
    pub settings: PathBuf,
    /// A file with the secret in each folder the policy hides (that
    /// exists), with that folder.
    pub hidden: Vec<(PathBuf, PathBuf)>,
    /// A file with the secret in each folder a caller asks to be read, or
    /// not ([`Expected::reads`], [`Expected::unread`]): the folder, the
    /// file, and whether it should be read.
    pub reads: Vec<(PathBuf, PathBuf, bool)>,
    /// A file with the secret outside the home folder (`/var/tmp`).
    pub elsewhere: PathBuf,
    /// A localhost port that isn't the harness's.
    pub port: u16,
    /// This machine's own network address, if it has one.
    pub lan: Option<std::net::IpAddr>,
    /// A program with the secret in its environment.
    pub pid: u32,
    /// A Keychain item Link added (macOS), by its service name.
    pub keychain: Option<String>,
    /// An abstract unix socket Link listens on (Linux, where python3 can
    /// try it), by its name: X11 and some session buses use these.
    pub abstract_socket: Option<String>,
    /// Whether python3 is there to try system calls (Linux).
    pub python: bool,
    /// This machine's ports the policy lets it reach (a local model's),
    /// but for those it serves itself.
    pub ports: Vec<u16>,
    /// `lnk-sandbox`, which looks up macOS services from inside.
    pub sandbox: PathBuf,
    /// Of `ports`, a model runtime's the proxy filters to inference (Linux).
    pub models: Vec<u16>,
    /// A dynamic program that exits 0, to try to run around `[commands]`
    /// (Linux, when a spec lists programs).
    pub payload: Option<PathBuf>,
    /// The dynamic loader, for the `ld.so` way (Linux).
    pub loader: Option<PathBuf>,
    /// A system library to try to preload from a write folder (Linux).
    pub lib: Option<PathBuf>,
    /// The policy's `[lan]` hosts, which its proxy may reach.
    pub lan_hosts: Vec<String>,
    /// Whether its proxy may reach the internet's 1.1.1.1.
    pub internet: bool,
    /// Whether it has DNS, its proxy's (Linux, where it names endpoints).
    pub dns: bool,
    /// The folders it writes that are there, and whether their dot files
    /// that run outside should be written.
    pub dot_files: Vec<DotFiles>,
    /// What this run's files are named after (`lnk-check-<tag>`).
    pub tag: u32,
}

/// Adds a throwaway item to the login Keychain (macOS), for a probe to
/// look for: "not found" alone can't tell a search that ran from one that
/// reached no keychain. None where there is no Keychain, or where a test's
/// stand-in (`LNK_SECURITY`) takes its place: the bait is the login
/// Keychain's, through Apple's own `security`.
pub(super) fn keychain_item(tag: u32) -> Option<String> {
    if !cfg!(target_os = "macos") || link_secretive::test_var("LNK_SECURITY").is_some() {
        return None;
    }
    let service = format!("lnk-check-{tag}");
    link_secretive::process::command("security")
        .args([
            "add-generic-password",
            "-U",
            "-a",
            "lnk",
            "-w",
            "bait",
            "-s",
        ])
        .arg(&service)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
        .then_some(service)
}

/// An abstract unix socket listening (Linux, with python3 to try it), its
/// name and the listener, which stops when dropped.
#[cfg(target_os = "linux")]
pub(super) fn abstract_socket(
    tag: u32,
) -> (Option<String>, Option<std::os::unix::net::UnixListener>) {
    use std::os::linux::net::SocketAddrExt;
    let name = format!("lnk-check-{tag}");
    let python = ["/usr/bin/python3", "/bin/python3"]
        .iter()
        .any(|p| std::path::Path::new(p).is_file());
    let listener = std::os::unix::net::SocketAddr::from_abstract_name(&name)
        .and_then(|addr| std::os::unix::net::UnixListener::bind_addr(&addr))
        .ok()
        .filter(|_| python);
    (listener.as_ref().map(|_| name), listener)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn abstract_socket(
    _tag: u32,
) -> (Option<String>, Option<std::os::unix::net::UnixListener>) {
    (None, None)
}

/// A dynamic program that exits 0, for the `[commands]` ways to try to
/// run around the list (Linux). A dynamic one needs the loader, so the
/// `ld.so` and library ways are meaningful.
pub(super) fn bypass_payload() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    ["true", "echo"]
        .into_iter()
        .find_map(|name| link_files::find_program(name, &path))
        .map(|p| link_sandbox_confine::resolve(&p))
}

/// A system shared library to copy into a write folder and try to
/// preload (Linux). Any `.so` with a version suffix in the usual folders.
pub(super) fn bypass_lib() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    for dir in [
        "/lib/x86_64-linux-gnu",
        "/lib/aarch64-linux-gnu",
        "/usr/lib",
        "/lib",
    ] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // A real, loadable library (not the loader, not a linker
            // script): a `.so.N` file.
            if name.contains(".so.") && !name.starts_with("ld-") && entry.path().is_file() {
                return Some(entry.path());
            }
        }
    }
    None
}

/// This machine's address on its network (no packet is sent).
pub(super) fn own_address() -> Option<std::net::IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}
