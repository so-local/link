//! Pinned state clones and the temporary namespace that maps ownership.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use anyhow::ensure;
use link_sandbox_contract::box_workers::Controller;

use super::account::Account;

/// Never recursively remove a directory that held a mount: detach each
/// private clone first, then remove only its empty mount point.
pub(super) struct Views {
    pub(super) dir: PathBuf,
    pub(super) paths: Vec<PathBuf>,
}

impl Views {
    pub(super) fn existing(dir: PathBuf) -> anyhow::Result<Self> {
        let paths = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .map(|entry| entry.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { dir, paths })
    }

    pub(super) fn clean(&mut self) -> anyhow::Result<()> {
        use std::os::unix::ffi::OsStrExt;
        while let Some(path) = self.paths.last() {
            let text = CString::new(path.as_os_str().as_bytes())?;
            // SAFETY: this mount is a clone in this launcher's private namespace.
            if unsafe { libc::umount2(text.as_ptr(), libc::MNT_DETACH) } != 0 {
                let error = std::io::Error::last_os_error();
                ensure!(
                    matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOENT)),
                    "detaching a worker mount: {error}"
                );
            }
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_dir() => std::fs::remove_dir(path)?,
                Ok(_) => std::fs::remove_file(path)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            self.paths.pop();
        }
        match std::fs::remove_dir(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

impl Drop for Views {
    fn drop(&mut self) {
        if let Err(err) = self.clean() {
            link_format::aside(
                link_format::Report::warning(link_format::sentence!(
                    "Worker mount cleanup failed."
                ))
                .why(link_format::Prose::from(link_format::Sentence::adopt(
                    &format!("{err:#}"),
                ))),
            );
        }
    }
}

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

pub(super) fn source(path: &Path) -> std::io::Result<File> {
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes())?;
    let how = OpenHow {
        flags: (libc::O_PATH | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: 0x02 | 0x04,
    };
    // SAFETY: the path and open_how outlive the syscall. No links or magic links.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::AT_FDCWD,
            path.as_ptr(),
            &how,
            std::mem::size_of_val(&how),
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a new descriptor, owned by nothing else.
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}

fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 fills two descriptors, exclusively owned from here.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// A short-lived namespace maker. Never runs user code or joins the worker.
struct NamespaceChild {
    pid: i32,
    hold: Option<OwnedFd>,
}
impl Drop for NamespaceChild {
    fn drop(&mut self) {
        self.hold.take();
        // SAFETY: this direct child is unreaped, so its PID cannot be reused.
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            while libc::waitpid(self.pid, std::ptr::null_mut(), 0) < 0 {
                if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }
}

pub(super) fn mapping(controller: &Controller, account: &Account) -> anyhow::Result<File> {
    use std::io::Read;
    let (ready_read, ready_write) = pipe()?;
    let (hold_read, hold_write) = pipe()?;
    // Keep this direct child unreaped until its guard waits for it, even
    // when a caller inherited an ignored SIGCHLD disposition.
    ensure!(
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) } != libc::SIG_ERR,
        "resetting child reaping"
    );
    // SAFETY: the launcher is single threaded. The child uses only raw
    // syscalls, no allocator, stdio, locks or workload-selected program.
    let pid = unsafe { libc::fork() };
    ensure!(
        pid >= 0,
        "making an identity namespace: {}",
        std::io::Error::last_os_error()
    );
    if pid == 0 {
        unsafe {
            libc::close(ready_read.as_raw_fd());
            libc::close(hold_write.as_raw_fd());
            let error = if libc::unshare(libc::CLONE_NEWUSER) == 0 {
                0i32
            } else {
                *libc::__errno_location()
            };
            libc::write(ready_write.as_raw_fd(), (&error as *const i32).cast(), 4);
            libc::close(ready_write.as_raw_fd());
            let mut byte = 0u8;
            libc::read(hold_read.as_raw_fd(), (&mut byte as *mut u8).cast(), 1);
            libc::_exit(0);
        }
    }
    let _child = NamespaceChild {
        pid,
        hold: Some(hold_write),
    };
    drop((ready_write, hold_read));
    let mut error = [0u8; 4];
    File::from(ready_read).read_exact(&mut error)?;
    let error = i32::from_ne_bytes(error);
    ensure!(
        error == 0,
        "creating an identity namespace: {}",
        std::io::Error::from_raw_os_error(error)
    );
    // In a mount idmap the stored ID maps down to the exposed host ID.
    std::fs::write(
        format!("/proc/{pid}/uid_map"),
        format!("{} {} 1\n", controller.uid, account.uid),
    )?;
    std::fs::write(format!("/proc/{pid}/setgroups"), "deny")?;
    let groups: String = controller
        .groups
        .iter()
        .zip(&account.groups)
        .map(|(from, (_, to))| format!("{from} {to} 1\n"))
        .collect();
    std::fs::write(format!("/proc/{pid}/gid_map"), groups)?;
    Ok(File::open(format!("/proc/{pid}/ns/user"))?)
}

pub(super) fn mapped(source: File, at: &Path, mapping: Option<&File>) -> anyhow::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    if source.metadata()?.is_dir() {
        std::fs::create_dir(at)?;
    } else {
        File::create(at)?;
    }
    let at = CString::new(at.as_os_str().as_bytes())?;
    // SAFETY: clone this pinned inode, never reopen its pathname. A detached
    // clone owns its mount until move_mount, or closes on every error.
    let tree = unsafe {
        libc::syscall(
            libc::SYS_open_tree,
            source.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH | libc::AT_RECURSIVE | 1 | libc::O_CLOEXEC,
        )
    };
    ensure!(
        tree >= 0,
        "cloning worker state: {}",
        std::io::Error::last_os_error()
    );
    let tree = unsafe { OwnedFd::from_raw_fd(tree as i32) };
    if let Some(mapping) = mapping {
        #[repr(C)]
        struct Attributes {
            set: u64,
            clear: u64,
            propagation: u64,
            userns: u64,
        }
        let attributes = Attributes {
            set: 0x0010_0000,
            clear: 0,
            propagation: 0,
            userns: mapping.as_raw_fd() as u64,
        };
        ensure!(
            unsafe {
                libc::syscall(
                    libc::SYS_mount_setattr,
                    tree.as_raw_fd(),
                    c"".as_ptr(),
                    libc::AT_EMPTY_PATH | libc::AT_RECURSIVE,
                    &attributes,
                    std::mem::size_of_val(&attributes),
                )
            } == 0,
            "the filesystem cannot map worker identities; no work was run: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: attach only the successfully prepared clone in this private NS.
    ensure!(
        unsafe {
            libc::syscall(
                libc::SYS_move_mount,
                tree.as_raw_fd(),
                c"".as_ptr(),
                libc::AT_FDCWD,
                at.as_ptr(),
                4, /* MOVE_MOUNT_F_EMPTY_PATH */
            )
        } == 0,
        "attaching worker state: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

pub(super) fn empty_home(at: &Path, account: &Account) -> anyhow::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    std::fs::create_dir(at)?;
    let path = CString::new(at.as_os_str().as_bytes())?;
    let options = CString::new(format!(
        "uid={},gid={},mode=0700",
        account.uid, account.groups[0].1
    ))?;
    // SAFETY: a new private namespace mount; every argument lives through it.
    ensure!(
        unsafe {
            libc::mount(
                c"tmpfs".as_ptr(),
                path.as_ptr(),
                c"tmpfs".as_ptr(),
                libc::MS_NOSUID | libc::MS_NODEV,
                options.as_ptr().cast(),
            )
        } == 0,
        "creating the worker's empty home: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

/// Create traversable synthetic parents before binds. `--dir` never changes
/// the permissions of an existing directory, including a mounted state inode.
pub(super) fn directories(argv: &[String], home: Option<(&Path, &Path)>) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if matches!(
            argv[i].as_str(),
            "--bind" | "--ro-bind" | "--bind-try" | "--ro-bind-try"
        ) {
            parents(Path::new(&argv[i + 2]), &mut out);
            out.extend_from_slice(&argv[i..i + 3]);
            i += 3;
        } else if argv[i] == "--tmpfs" {
            let path = Path::new(&argv[i + 1]);
            if let Some((original, view)) = home.filter(|(original, _)| *original == path) {
                // A bind's missing parent is made 0700, root's: the
                // worker couldn't cross it into its home.
                parents(original, &mut out);
                out.extend([
                    "--bind".into(),
                    view.display().to_string(),
                    original.display().to_string(),
                ]);
            } else {
                if matches!(
                    path.to_str(),
                    Some("/tmp" | "/var/tmp" | "/dev/shm" | "/run")
                ) {
                    out.extend(["--perms".into(), "1777".into()]);
                }
                out.extend_from_slice(&argv[i..i + 2]);
            }
            i += 2;
        } else {
            out.push(argv[i].clone());
            i += 1;
        }
    }
    out
}

/// `--dir` for each of `path`'s parents but `/`, outermost first, each
/// 0755: bubblewrap makes a bind's missing parent 0700.
fn parents(path: &Path, out: &mut Vec<String>) {
    let parents: Vec<_> = path
        .ancestors()
        .skip(1)
        .filter(|p| *p != Path::new("/"))
        .collect();
    for parent in parents.into_iter().rev() {
        out.extend([
            "--perms".into(),
            "0755".into(),
            "--dir".into(),
            parent.display().to_string(),
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, symlink};

    #[test]
    fn state_sources_are_pinned_and_links_are_refused() {
        let temp = link_test_kit::TempDir::new("worker-source");
        let path = temp.join("state");
        std::fs::write(&path, "original").unwrap();
        let pinned = source(&path).unwrap();
        std::fs::rename(&path, temp.join("old")).unwrap();
        std::fs::write(&path, "replacement").unwrap();
        assert_ne!(
            pinned.metadata().unwrap().ino(),
            source(&path).unwrap().metadata().unwrap().ino()
        );
        symlink(&path, temp.join("link")).unwrap();
        assert!(source(&temp.join("link")).is_err());
        symlink(&*temp, temp.join("parent")).unwrap();
        assert!(source(&temp.join("parent/state")).is_err());
    }

    #[test]
    fn synthetic_parents_are_created_before_binds_without_chmod() {
        let argv = [
            "bwrap",
            "--tmpfs",
            "/home/c",
            "--ro-bind",
            "/view",
            "/work/private/file",
        ]
        .map(str::to_string);
        let args = directories(
            &argv,
            Some((Path::new("/home/c"), Path::new("/runtime/home"))),
        );
        assert_eq!(
            args,
            [
                "bwrap",
                "--perms",
                "0755",
                "--dir",
                "/home",
                "--bind",
                "/runtime/home",
                "/home/c",
                "--perms",
                "0755",
                "--dir",
                "/work",
                "--perms",
                "0755",
                "--dir",
                "/work/private",
                "--ro-bind",
                "/view",
                "/work/private/file"
            ]
        );
        assert!(!args.iter().any(|a| a == "--chmod"));
    }
}
