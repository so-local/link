//! Bind sources opened at launch, one component at a time without
//! following links. Bubblewrap mounts the held inode, even if a name is
//! changed before it mounts it. No host pathname is resolved twice.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path};

use anyhow::{Context, ensure};

pub struct Pins(Vec<File>);

impl Pins {
    /// Pin every mount source before starting bubblewrap. A missing
    /// optional source stays absent, even if it appears after this check.
    pub fn of(argv: &mut Vec<String>) -> anyhow::Result<Self> {
        let mut pins = Self(Vec::new());
        let mut at = 1;
        while at < argv.len() && argv[at] != "--" {
            let flag = argv[at].as_str();
            let bind = matches!(
                flag,
                "--bind" | "--bind-try" | "--ro-bind" | "--ro-bind-try"
            );
            let count = arity(flag).with_context(|| {
                format!("unknown sandbox mount option {flag:?}; target not started")
            })?;
            ensure!(
                at + count < argv.len(),
                "incomplete sandbox mount option {flag}"
            );
            // A mount a later one of the same place covers shows nothing,
            // but bubblewrap would still resolve its source: left out.
            if bind && covered(argv, at + count + 1, &argv[at + 2]) {
                argv.drain(at..at + 3);
                continue;
            }
            if bind {
                let source = Path::new(&argv[at + 1]);
                match open(source).and_then(|file| Held::of(file, flag, source)) {
                    Ok(Held::Named(file)) => {
                        argv[at + 1] = format!("/proc/self/fd/{}", file.as_raw_fd());
                        pins.0.push(file);
                    }
                    Ok(Held::Copied(file, mode)) => {
                        let dest = argv[at + 2].clone();
                        let fd = file.as_raw_fd().to_string();
                        let mode = format!("{:o}", mode & 0o777);
                        argv.splice(
                            at..at + 3,
                            ["--perms".into(), mode, "--ro-bind-data".into(), fd, dest],
                        );
                        pins.0.push(file);
                        at += 5;
                        continue;
                    }
                    Ok(Held::Gone) if flag.ends_with("-try") => {
                        argv.drain(at..at + 3);
                        continue;
                    }
                    Ok(Held::Gone) => anyhow::bail!(
                        "sandbox source {} was removed after it was pinned; target not started",
                        source.display()
                    ),
                    Err(error)
                        if flag.ends_with("-try")
                            && error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        argv.drain(at..at + 3);
                        continue;
                    }
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!(
                                "pinning sandbox source {}; target not started",
                                source.display()
                            )
                        });
                    }
                }
            }
            at += count + 1;
        }
        ensure!(at < argv.len(), "sandbox mount command has no --");
        Ok(pins)
    }

    /// Only bubblewrap inherits the descriptors. The confiner closes
    /// inherited descriptors before the target can run.
    pub fn pass_to(&self, command: &mut std::process::Command) {
        let fds: Vec<_> = self.0.iter().map(AsRawFd::as_raw_fd).collect();
        // SAFETY: only async-signal-safe fcntl calls in the child, on the
        // descriptors kept alive by this Pins until spawn returns.
        unsafe {
            command.pre_exec(move || {
                for fd in &fds {
                    if libc::fcntl(*fd, libc::F_SETFD, 0) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
}

/// A pinned source, as bubblewrap is given it. Bubblewrap mounts a
/// source by its name: it resolves `/proc/self/fd/N` to the path the
/// kernel gives the held inode, and mounts what that path reaches. A
/// source whose inode no name reaches any more (a file replaced by
/// renaming another over it, as `useradd` replaces `/etc/group`, or a
/// file an outer sandbox shows that the machine has since replaced) it
/// can't mount, and the kernel won't bind a mount from another mount
/// namespace's descriptor.
///
/// A read-only file from the machine's `/etc` is always given by its
/// contents: the machine's tools replace those by renaming at any moment
/// (`useradd`, run by a Link box's workers for every launch, replaces
/// `/etc/passwd` and `/etc/group`), so a name that reaches the held inode
/// now may reach another by the time bubblewrap resolves it.
enum Held {
    /// Reached by its name: mounted by `/proc/self/fd/N`.
    Named(File),
    /// A read-only file no name reaches: its contents, read from the
    /// held inode (`--ro-bind-data`), with its mode.
    Copied(File, u32),
    /// A folder, or a writable file, no name reaches: removed.
    Gone,
}

impl Held {
    fn of(file: File, flag: &str, source: &Path) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let held = file.metadata()?;
        let at = format!("/proc/self/fd/{}", file.as_raw_fd());
        let read_only_file = held.is_file() && flag.starts_with("--ro-bind");
        let renamed_any_time = source.starts_with(crate::system::LINUX_ETC);
        // What bubblewrap does with the source: its realpath, then the
        // inode that path reaches.
        let named = || {
            std::fs::canonicalize(&at)
                .and_then(std::fs::metadata)
                .is_ok_and(|now| now.dev() == held.dev() && now.ino() == held.ino())
        };
        if !(read_only_file && renamed_any_time) && named() {
            return Ok(Self::Named(file));
        }
        if !read_only_file {
            return Ok(Self::Gone);
        }
        let read = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(&at)?;
        Ok(Self::Copied(read, held.mode()))
    }
}

/// Whether a mount from `from` on, before bubblewrap's `--`, surely puts
/// something at `dest`: a bind that isn't optional, or data.
fn covered(argv: &[String], from: usize, dest: &str) -> bool {
    let mut at = from;
    while at < argv.len() && argv[at] != "--" {
        let Some(count) = arity(&argv[at]) else {
            return false;
        };
        let sure = matches!(argv[at].as_str(), "--bind" | "--ro-bind" | "--ro-bind-data");
        if sure && argv.get(at + 2).is_some_and(|d| d == dest) {
            return true;
        }
        at += count + 1;
    }
    false
}

/// How many values bubblewrap's option `flag` takes: None for one this
/// launch never gives it, which is refused.
fn arity(flag: &str) -> Option<usize> {
    Some(match flag {
        "--bind" | "--bind-try" | "--ro-bind" | "--ro-bind-try" | "--ro-bind-data" | "--setenv"
        | "--symlink" => 2,
        "--dev" | "--proc" | "--tmpfs" | "--remount-ro" | "--dir" | "--cap-drop" | "--info-fd"
        | "--block-fd" | "--cap-add" | "--chdir" | "--perms" => 1,
        "--die-with-parent"
        | "--new-session"
        | "--disable-userns"
        | "--unshare-pid"
        | "--unshare-ipc"
        | "--unshare-uts"
        | "--unshare-cgroup-try"
        | "--unshare-net"
        | "--unshare-user" => 0,
        _ => return None,
    })
}

/// Where each of bubblewrap's options is in `argv` (bubblewrap first),
/// before its `--`: refused where one is unknown or incomplete, or
/// there's no `--`.
pub fn options(argv: &[String]) -> anyhow::Result<Vec<usize>> {
    let mut at = 1;
    let mut found = Vec::new();
    while at < argv.len() && argv[at] != "--" {
        let flag = argv[at].as_str();
        let count = arity(flag).with_context(|| {
            format!("unknown sandbox mount option {flag:?}; target not started")
        })?;
        ensure!(
            at + count < argv.len(),
            "incomplete sandbox mount option {flag}"
        );
        found.push(at);
        at += count + 1;
    }
    ensure!(at < argv.len(), "sandbox mount command has no --");
    Ok(found)
}

fn open(path: &Path) -> std::io::Result<File> {
    if !path.is_absolute() {
        return Err(std::io::Error::other("a sandbox source must be absolute"));
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open("/")?;
    let mut parts = path.components().skip(1).peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(name) = part else {
            return Err(std::io::Error::other("a sandbox source must be canonical"));
        };
        let name = CString::new(name.as_encoded_bytes())?;
        let flags = libc::O_PATH
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if parts.peek().is_some() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: an owned directory descriptor and a NUL-terminated
        // single component; the resulting descriptor is owned once.
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a fresh descriptor.
        file = unsafe { File::from_raw_fd(fd) };
    }
    if file.metadata()?.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "a sandbox source became a symbolic link",
        ));
    }
    Ok(file)
}

use std::os::unix::fs::OpenOptionsExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_pinned_and_links_at_every_depth_are_refused() {
        let temp = link_test_kit::TempDir::new("sandbox-source-pins");
        let temp = temp.canonicalize().unwrap();
        let folder = temp.join("grant");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("value"), "granted").unwrap();
        let pinned = open(&folder.join("value")).unwrap();
        std::fs::rename(&folder, temp.join("moved")).unwrap();
        std::os::unix::fs::symlink(temp.join("moved"), &folder).unwrap();
        assert!(open(&folder).is_err());
        assert!(open(&folder.join("value")).is_err());
        assert_eq!(
            std::fs::read_to_string(format!("/proc/self/fd/{}", pinned.as_raw_fd())).unwrap(),
            "granted"
        );
    }

    #[test]
    fn a_source_no_name_reaches_is_copied_when_read_only_and_gone_otherwise() {
        use std::io::Read;
        let temp = link_test_kit::TempDir::new("sandbox-source-unnamed");
        let temp = temp.canonicalize().unwrap();
        let file = temp.join("group");
        std::fs::write(&file, "before").unwrap();
        let folder = temp.join("folder");
        std::fs::create_dir(&folder).unwrap();
        let dest = Path::new("/home/group");
        assert!(matches!(
            Held::of(open(&file).unwrap(), "--ro-bind", dest).unwrap(),
            Held::Named(_)
        ));
        let (pinned, pinned_folder) = (open(&file).unwrap(), open(&folder).unwrap());
        // Replaced as `useradd` replaces `/etc/group`, and removed.
        std::fs::write(temp.join("group.new"), "after").unwrap();
        std::fs::rename(temp.join("group.new"), &file).unwrap();
        std::fs::remove_dir(&folder).unwrap();
        let Held::Copied(mut read, _) =
            Held::of(pinned.try_clone().unwrap(), "--ro-bind", dest).unwrap()
        else {
            panic!("a replaced read-only file is copied");
        };
        let mut text = String::new();
        read.read_to_string(&mut text).unwrap();
        assert_eq!(text, "before");
        assert!(matches!(
            Held::of(pinned, "--bind", dest).unwrap(),
            Held::Gone
        ));
        assert!(matches!(
            Held::of(pinned_folder, "--ro-bind-try", dest).unwrap(),
            Held::Gone
        ));
    }

    #[test]
    fn a_read_only_file_in_etc_is_copied_though_its_name_reaches_it() {
        let temp = link_test_kit::TempDir::new("sandbox-source-etc");
        let temp = temp.canonicalize().unwrap();
        let file = temp.join("passwd");
        std::fs::write(&file, "root:x:0:0").unwrap();
        let folder = temp.join("ld.so.conf.d");
        std::fs::create_dir(&folder).unwrap();
        let etc = Path::new(crate::system::LINUX_ETC);
        assert!(matches!(
            Held::of(open(&file).unwrap(), "--ro-bind", &etc.join("passwd")).unwrap(),
            Held::Copied(..)
        ));
        // Only a read-only file: a folder there, or a writable file, is
        // mounted by its name as anywhere else.
        assert!(matches!(
            Held::of(
                open(&folder).unwrap(),
                "--ro-bind",
                &etc.join("ld.so.conf.d")
            )
            .unwrap(),
            Held::Named(_)
        ));
        assert!(matches!(
            Held::of(open(&file).unwrap(), "--bind", &etc.join("passwd")).unwrap(),
            Held::Named(_)
        ));
        // A file of Link's own mounted at `/etc` (the sandbox's
        // resolv.conf) isn't the machine's: mounted by its name.
        assert!(matches!(
            Held::of(open(&file).unwrap(), "--ro-bind", &file).unwrap(),
            Held::Named(_)
        ));
    }

    #[test]
    fn a_mount_a_later_one_covers_is_left_out() {
        let temp = link_test_kit::TempDir::new("sandbox-source-covered");
        let temp = temp.canonicalize().unwrap();
        let (machine, own) = (temp.join("hosts"), temp.join("own-hosts"));
        std::fs::write(&machine, "machine").unwrap();
        std::fs::write(&own, "own").unwrap();
        let (machine, own) = (machine.display().to_string(), own.display().to_string());
        let mut argv: Vec<String> = [
            "bwrap",
            "--ro-bind",
            &machine,
            "/etc/hosts",
            "--ro-bind-try",
            &own,
            "/x",
            "--ro-bind",
            &own,
            "/etc/hosts",
            "--",
            "sh",
        ]
        .map(String::from)
        .to_vec();
        let _pins = Pins::of(&mut argv).unwrap();
        assert_eq!(
            argv.iter().filter(|a| *a == "/etc/hosts").count(),
            1,
            "{argv:?}"
        );
        // An optional one covers nothing: the earlier mount stays.
        let mut argv: Vec<String> = [
            "bwrap",
            "--ro-bind",
            &machine,
            "/x",
            "--ro-bind-try",
            &own,
            "/x",
            "--",
            "sh",
        ]
        .map(String::from)
        .to_vec();
        let _pins = Pins::of(&mut argv).unwrap();
        assert_eq!(argv.iter().filter(|a| *a == "/x").count(), 2, "{argv:?}");
    }

    #[test]
    fn absent_optional_mounts_are_removed_and_only_options_are_parsed() {
        let temp = link_test_kit::TempDir::new("sandbox-source-options");
        let missing = temp.join("missing").display().to_string();
        let mut argv: Vec<_> = [
            "bwrap",
            "--setenv",
            "VALUE",
            "--bind",
            "--bind-try",
            &missing,
            "/missing",
            "--",
            "--bind",
            "/target",
            "/target",
        ]
        .map(String::from)
        .into();
        let _pins = Pins::of(&mut argv).unwrap();
        assert_eq!(
            argv,
            [
                "bwrap", "--setenv", "VALUE", "--bind", "--", "--bind", "/target", "/target"
            ]
        );
        assert!(Pins::of(&mut vec!["bwrap".into(), "--unknown".into(), "--".into()]).is_err());
    }
}
