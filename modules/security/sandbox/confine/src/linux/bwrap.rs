//! bubblewrap, which makes the sandbox on Linux: where it is, whether it
//! works here (and Ubuntu's rule that stops it, with the AppArmor profile
//! that lets it), and its arguments for a sandbox's [`Rules`].

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::linux::{etc, kept};
use crate::{Common, Linux};

/// Whether bubblewrap at `bwrap` can refuse user namespaces inside
/// (`--disable-userns`, bubblewrap 0.8 and newer). Asked of bubblewrap
/// once per boot and copy of it, and kept in Link's settings
/// ([`kept`]): each wrap is a new process.
pub fn disables_userns(bwrap: &str) -> bool {
    static ANSWER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ANSWER.get_or_init(|| {
        if let Some(answer) = kept::kept(bwrap, DISABLES_USERNS).and_then(|a| a.parse().ok()) {
            return answer;
        }
        let output = link_secretive::process::command(bwrap)
            .arg("--version")
            .output()
            .ok();
        match version_disables_userns(output.as_ref().map(|o| o.stdout.as_slice())) {
            // Kept only as bubblewrap said it: a version not read (a
            // spawn refused for want of processes or memory) is asked
            // again by the next sandbox, never kept as "can't".
            Some(answer) => {
                kept::keep(bwrap, DISABLES_USERNS, &answer.to_string());
                answer
            }
            None => false,
        }
    })
}

/// Whether bubblewrap's `--version` output says 0.8 or newer; None when
/// there is no version in it to read.
fn version_disables_userns(output: Option<&[u8]>) -> Option<bool> {
    let text = String::from_utf8_lossy(output?);
    let version = text
        .split_whitespace()
        .nth(1)?
        .parse::<link_version::Version>()
        .ok()?;
    Some(version >= link_version::Version::new(0, 8, 0))
}

/// The name [`disables_userns`] keeps its answer by.
const DISABLES_USERNS: &str = "disables-userns";

/// bubblewrap, which makes the sandbox on Linux.
pub fn bwrap() -> anyhow::Result<&'static str> {
    link_files::bubblewrap().ok_or_else(|| {
        anyhow::anyhow!(
            "the sandbox needs bubblewrap: sudo apt install bubblewrap (or your \
                 system's package for it)"
        )
    })
}

/// Why bubblewrap can't make a sandbox on this machine.
#[derive(Debug, PartialEq)]
pub enum Blocked {
    /// Ubuntu 23.10+ lets only programs with an AppArmor profile make the
    /// user namespaces a sandbox needs
    /// (`kernel.apparmor_restrict_unprivileged_userns`).
    AppArmor,
    /// Anything else, with bubblewrap's own words.
    Other(String),
}

/// [`works`], unless it worked before on this machine as it is now: the
/// same boot, kernel, user and bubblewrap ([`kept`]). Only its working
/// is kept, so a machine it fails on is asked every time.
pub fn worked(bwrap: &str) -> Result<(), Blocked> {
    kept::checked(bwrap, "works", || works(bwrap))
}

/// Whether bubblewrap at `bwrap` can make a sandbox here: it tries an
/// empty one.
pub fn works(bwrap: &str) -> Result<(), Blocked> {
    let output = link_secretive::process::command(bwrap)
        .args(["--ro-bind", "/", "/", "--unshare-pid", "--", "/bin/true"])
        .output()
        .map_err(|err| Blocked::Other(err.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let restricted =
        std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
            .is_ok_and(|v| v.trim() == "1");
    Err(blocked(
        restricted,
        String::from_utf8_lossy(&output.stderr).trim(),
    ))
}

pub(crate) fn blocked(restricted: bool, stderr: &str) -> Blocked {
    // "setting up uid map: Permission denied", "No permissions to create
    // a new namespace": what the AppArmor rule looks like from inside.
    let namespace = stderr.contains("uid map") || stderr.contains("namespace");
    match restricted && namespace {
        true => Blocked::AppArmor,
        false => Blocked::Other(stderr.to_string()),
    }
}

/// Whether readying the machine must still add the profile though
/// bubblewrap worked: run as root, which Ubuntu's rule leaves its
/// capabilities, the try speaks for no user who runs a sandbox. A box
/// readied by its boot, as root, runs its agents as users of their own.
pub fn unproven_for_users() -> bool {
    // SAFETY: geteuid can't fail.
    let root = unsafe { libc::geteuid() } == 0;
    let restricted =
        std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
            .is_ok_and(|v| v.trim() == "1");
    needs_profile(root, restricted, Path::new(APPARMOR_PROFILE).is_file())
}

pub(crate) fn needs_profile(root: bool, restricted: bool, profiled: bool) -> bool {
    root && restricted && !profiled
}

/// Where Link's AppArmor profile for bubblewrap goes.
pub const APPARMOR_PROFILE: &str = "/etc/apparmor.d/lnk-bwrap";

/// The AppArmor profile that lets bubblewrap at `bwrap` make user
/// namespaces, and nothing else: it stays unconfined otherwise, as
/// without the rule.
pub fn apparmor_profile(bwrap: &str) -> String {
    format!(
        "# Written by lnk: lets bubblewrap make the sandbox Link runs your agent in.\n\
         abi <abi/4.0>,\n\
         include <tunables/global>\n\
         \n\
         profile lnk-bwrap {bwrap} flags=(unconfined) {{\n\
         \x20 userns,\n\
         \x20 include if exists <local/lnk-bwrap>\n\
         }}\n"
    )
}

/// The commands that install [`apparmor_profile`], for the user to run
/// or for [`allow_bwrap`] to run with sudo.
pub fn apparmor_commands(bwrap: &str) -> String {
    format!(
        "printf '%s' '{}' | sudo tee {APPARMOR_PROFILE} >/dev/null\n\
         sudo apparmor_parser -r {APPARMOR_PROFILE}",
        apparmor_profile(bwrap).replace('\'', "'\\''")
    )
}

/// Installs [`apparmor_profile`] with sudo (which asks for the password).
pub fn allow_bwrap(bwrap: &str) -> anyhow::Result<()> {
    allow_bwrap_with(bwrap, &[])
}

/// [`allow_bwrap`] where sudo needs no password (`sudo -n`, a box's).
pub fn allow_bwrap_unasked(bwrap: &str) -> anyhow::Result<()> {
    allow_bwrap_with(bwrap, &["-n"])
}

fn allow_bwrap_with(bwrap: &str, sudo: &[&str]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut tee = link_secretive::process::command("sudo")
        .args(sudo)
        .args(["tee", APPARMOR_PROFILE])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .context("running sudo")?;
    tee.stdin
        .take()
        .context("no input to sudo tee")?
        .write_all(apparmor_profile(bwrap).as_bytes())?;
    anyhow::ensure!(tee.wait()?.success(), "writing {APPARMOR_PROFILE} failed");
    let status = link_secretive::process::command("sudo")
        .args(sudo)
        .args(["apparmor_parser", "-r", APPARMOR_PROFILE])
        .status()
        .context("running sudo apparmor_parser")?;
    anyhow::ensure!(status.success(), "loading {APPARMOR_PROFILE} failed");
    Ok(())
}

/// bubblewrap arguments running `argv` under the file and program grants
/// every backend shares (`common`) and the parts only Linux enforces
/// (`linux`): never anything a Seatbelt profile would.
pub fn args(bwrap: &str, common: &Common, linux: &Linux, argv: &[String]) -> Vec<String> {
    let mut args: Vec<String> = [bwrap, "--die-with-parent"].map(String::from).to_vec();
    if !linux.terminal {
        // A session of its own: it can't type into the caller's terminal
        // (TIOCSTI), which the caller's shell reads once it's done.
        args.push("--new-session".into());
    }
    args.extend(
        [
            // Its own process list (no reading other processes' /proc), IPC
            // and hostname.
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup-try",
        ]
        .map(String::from),
    );
    // The system's programs, libraries and settings, read only; nothing
    // else of the machine (other disks, /srv, /var, other homes).
    for dir in etc::SYSTEM {
        let path = Path::new(dir);
        match path.symlink_metadata() {
            // /bin -> usr/bin on merged-/usr systems: the same link.
            Ok(meta) if meta.file_type().is_symlink() => {
                if let Ok(target) = std::fs::read_link(path) {
                    args.extend([
                        "--symlink".into(),
                        target.display().to_string(),
                        dir.to_string(),
                    ]);
                }
            }
            Ok(meta) if meta.is_dir() => {
                args.extend(["--ro-bind".into(), dir.to_string(), dir.to_string()]);
            }
            _ => {}
        }
    }
    cover_sockets(&mut args, crate::system::LINUX_SYSTEM_SOCKETS);
    cover_sockets(&mut args, crate::system::LINUX_ROOT_ONLY);
    etc::args(Path::new(crate::system::LINUX_ETC), &mut args);
    if let Some(hosts) = &linux.etc_hosts {
        args.extend([
            "--ro-bind".into(),
            hosts.display().to_string(),
            "/etc/hosts".into(),
        ]);
    }
    // Its own /dev and /proc, and the fresh, empty, writable folders it
    // gets, each made the way the shared baseline names
    // ([`crate::system::LINUX_EPHEMERAL`], where the why of each lives).
    for dir in crate::system::LINUX_EPHEMERAL {
        args.extend([dir.flag.into(), dir.at.into()]);
    }
    if let Some(resolv) = &linux.resolv_conf {
        // Where /etc/resolv.conf is a link (into /run, systemd-resolved's),
        // over what it links to, after /run is the sandbox's own.
        let at = std::fs::canonicalize("/etc/resolv.conf")
            .unwrap_or_else(|_| PathBuf::from("/etc/resolv.conf"));
        args.extend([
            "--ro-bind".into(),
            resolv.display().to_string(),
            at.display().to_string(),
        ]);
    }
    if !linux.host {
        args.push("--unshare-net".into());
    } else if let Ok(resolv) = std::fs::canonicalize("/etc/resolv.conf")
        && resolv.starts_with("/run")
    {
        // DNS, when resolv.conf points into /run (systemd-resolved).
        let resolv = resolv.display().to_string();
        args.extend(["--ro-bind-try".into(), resolv.clone(), resolv]);
    }
    let home = common.home.display().to_string();
    if common.read_home {
        args.extend(["--ro-bind-try".into(), home.clone(), home]);
    } else {
        // An empty home folder, then only the allowed folders in it.
        args.extend(["--tmpfs".into(), home]);
    }
    // Its folders, then what's hidden over them (Link's own settings,
    // wherever they are, and what the policy hides that exists), then its
    // folders inside what's hidden: mounted later, a folder holding a
    // hidden one would show it again.
    let hidden: Vec<&PathBuf> = common
        .hide
        .iter()
        .chain(common.programs.iter().filter(|path| path.exists()))
        .chain(common.closed.iter().filter(|c| c.exists()))
        .collect();
    let inside = |p: &PathBuf| hidden.iter().any(|h| p.starts_with(h) && p != *h);
    // No later bind can reopen a Safe or write installed programs,
    // including an internal write of their parent.
    let settings = |p: &PathBuf, flag: &str| {
        common.hide.iter().any(|h| p.starts_with(h))
            || (flag == "--bind-try"
                && common
                    .hide
                    .iter()
                    .chain(&common.programs)
                    .any(|h| p.starts_with(h) || h.starts_with(p)))
    };
    let binds = |args: &mut Vec<String>, later: bool| {
        for (path, flag) in common
            .read
            .iter()
            .map(|p| (p, "--ro-bind-try"))
            // -try: a folder the program creates later isn't there yet.
            .chain(common.write.iter().map(|p| (p, "--bind-try")))
            .filter(|(p, flag)| !settings(p, flag))
        {
            if inside(path) == later {
                let path = path.display().to_string();
                args.extend([flag.into(), path.clone(), path]);
            }
        }
    };
    binds(&mut args, false);
    for hidden in &hidden {
        args.extend(["--tmpfs".into(), hidden.display().to_string()]);
    }
    binds(&mut args, true);
    // Other programs' sockets in the home folder it reads: a read-only
    // mount doesn't stop a connection.
    #[cfg(target_os = "linux")]
    args.extend(super::home_sockets::args(
        &common.home,
        common.read_home,
        &common.read,
    ));
    // What runs outside the sandbox at the top of a folder it writes,
    // read only over it, but in what's hidden (unless the folder is in
    // there too), or where the policy writes that very path. A mount
    // there can't be renamed or removed, so `.git` can't be moved aside
    // and made again. A folder it writes inside one is bound again after.
    let mut covered = Vec::new();
    for entry in &common.outside {
        let path = entry.path();
        let in_hidden = hidden
            .iter()
            .any(|h| path.starts_with(h) && !entry.folder.starts_with(h));
        if !entry.folder.is_dir() || !entry.mounted() || in_hidden || common.write.contains(&path) {
            continue;
        }
        let text = path.display().to_string();
        // In Link's data folders another run's sweep may remove one at
        // any moment ([`crate::dotfiles::Entry::sweeps`]): one gone by
        // launch has nothing left to keep.
        let (bind, ro_bind) = match entry.swept {
            true => ("--bind-try", "--ro-bind-try"),
            false => ("--bind", "--ro-bind"),
        };
        if entry.pin {
            // Bound over itself, writable: a mount can't be renamed.
            args.extend([bind.into(), text.clone(), text]);
            continue;
        }
        args.extend([ro_bind.into(), text.clone(), text]);
        covered.push(path);
    }
    for write in &common.write {
        if !settings(write, "--bind-try")
            && covered.iter().any(|c| write.starts_with(c) && write != c)
        {
            let write = write.display().to_string();
            args.extend(["--bind-try".into(), write.clone(), write]);
        }
    }
    for own in linux
        .own
        .iter()
        .filter(|path| !settings(path, "--bind-try"))
    {
        let own = own.display().to_string();
        args.extend(["--bind-try".into(), own.clone(), own]);
    }
    // SAFETY: geteuid can't fail.
    let root = unsafe { libc::geteuid() } == 0;
    // Run by root, a program inside is root of a user namespace mapping
    // only root, which can't give a file to anyone else: tar, keeping an
    // archive's owners as root does (a harness installer's Node), fails.
    if root {
        args.extend(["--setenv", "TAR_OPTIONS", "--no-same-owner"].map(String::from));
        // Run by root, bubblewrap keeps root's capabilities inside, which
        // open any process's descriptors and memory (the exec watch's
        // among them), read and write any file it's given, and more: none.
        // Not dumpable, the watch is then closed to it, as to anyone's.
        args.extend(["--cap-drop", "ALL"].map(String::from));
    }
    if linux.caps.no_userns {
        // No new user namespaces inside, which open much of the kernel to
        // an unprivileged program (the seccomp filter refuses them too).
        args.extend(["--unshare-user".into(), "--disable-userns".into()]);
    } else if root {
        // A user namespace of its own, which root's bubblewrap makes only
        // when asked: the proxy's DNS helper joins the sandbox's network
        // as its owner.
        args.push("--unshare-user".into());
    }
    // Every mount in /etc made: none is added there, or written, from
    // inside, root's `ld.so.preload` among them.
    args.extend(["--remount-ro".into(), "/etc".into()]);
    args.push("--".into());
    if let Some(exe) = &linux.confine {
        // First inside: this program installs the seccomp filter, then
        // becomes `argv` (`sandbox confine`). Only the file, never its
        // folder, which holds whatever else the user keeps there.
        let file = exe.display().to_string();
        let at = args.len() - 1;
        args.splice(at..at, ["--ro-bind-try".into(), file.clone(), file]);
        args.extend([exe.display().to_string(), "confine".into()]);
        // Then the only programs it may run, by Landlock, the loader
        // every program needs among them.
        if let Some(programs) = &common.commands {
            args.push("--only".into());
            for program in programs
                .iter()
                .cloned()
                .chain(crate::linux::landlock::loaders())
            {
                args.extend(["--exec".into(), program.display().to_string()]);
            }
        }
        // Then, where the kernel has Landlock, the file grants again, for a
        // second wall inside the mounts: read its read grants (the home
        // whole where it reads it, this program), read and write its write
        // grants (the home where it is its own empty scratch, its sockets'
        // folder). The system's read-only folders are added inside, beside
        // these. Absent Landlock, nothing is passed and the mounts stand
        // alone; so on Landlock's first ABI too, whose every layer refuses
        // a rename into another folder (no `REFER`), which git does.
        if file_wall_here(linux.caps.landlock) {
            args.push("--file-wall".into());
            let mut read = common.read.clone();
            read.push(exe.clone());
            if common.read_home {
                read.push(common.home.clone());
            }
            let mut write = common.write.clone();
            write.extend(linux.own.iter().cloned());
            if !common.read_home {
                write.push(common.home.clone());
            }
            for path in read {
                args.extend(["--read".into(), path.display().to_string()]);
            }
            for path in write {
                args.extend(["--write".into(), path.display().to_string()]);
            }
        }
        args.push("--".into());
    }
    args.extend(argv.iter().cloned());
    args
}

/// Makes each of the `closed` folders in one of Link's own `roots` (the
/// Link folder and the installation folder) that isn't there yet, empty and
/// the user's alone, as the Safes are made ([`link_files::layout::prepare`]).
/// [`args`] covers only a closed folder that exists, so one made after a
/// sandbox reading the home folder started (`~/Link/Archive`) would be
/// seen. A root that isn't there is left so, as its owner may have moved
/// it, and nothing outside the roots is made.
pub fn make_closed(closed: &[PathBuf], roots: &[PathBuf]) -> anyhow::Result<()> {
    for path in closed {
        let in_root = roots
            .iter()
            .any(|root| path.starts_with(root) && path != root && root.is_dir());
        if in_root && std::fs::symlink_metadata(path).is_err() {
            link_files::create_private_dir(path)
                .with_context(|| format!("making {}, which every sandbox hides", path.display()))?;
        }
    }
    Ok(())
}

/// Whether the file wall goes inside a sandbox on a kernel of Landlock
/// ABI `abi`: from ABI 2, which knows `REFER`, so renames across folders
/// inside a grant keep working.
fn file_wall_here(abi: Option<std::num::NonZeroU32>) -> bool {
    abi.is_some_and(|abi| abi.get() >= 2)
}

/// Covers each of `folders` that is a folder, not a link, with an empty
/// one: other programs' sockets in the system's folders
/// ([`crate::system::LINUX_SYSTEM_SOCKETS`]).
fn cover_sockets(args: &mut Vec<String>, folders: &[&str]) {
    for folder in folders {
        if std::fs::symlink_metadata(folder).is_ok_and(|meta| meta.is_dir()) {
            args.extend(["--tmpfs".into(), (*folder).into()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cover_sockets, make_closed, version_disables_userns};

    /// A closed folder in Link's own roots that isn't there yet is made,
    /// only the user's, and then covered; one outside them, or in a root
    /// that isn't there, is never made.
    #[test]
    fn closed_folders_in_links_roots_exist_before_wrapping() {
        use std::os::unix::fs::PermissionsExt;
        let root = link_test_kit::TempDir::new("closed-made");
        let (link, machine) = (root.join("Link"), root.join("machine"));
        std::fs::create_dir(&link).unwrap();
        std::fs::create_dir(&machine).unwrap();
        let archive = link.join("Archive");
        let data = machine.join("data/closed");
        let outside = root.join("elsewhere/hidden");
        let moved = root.join("moved/Closed");
        let closed = vec![
            archive.clone(),
            data.clone(),
            outside.clone(),
            moved.clone(),
        ];
        let roots = [link.clone(), machine.clone(), root.join("moved")];
        make_closed(&closed, &roots).unwrap();
        for made in [&archive, &data] {
            let mode = std::fs::metadata(made).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{}", made.display());
        }
        assert!(!root.join("elsewhere").exists() && !root.join("moved").exists());
        // Made again, nothing changes: what's there is left as it is.
        make_closed(&closed, &roots).unwrap();

        let common = crate::Common {
            home: root.to_path_buf(),
            read_home: true,
            closed,
            ..Default::default()
        };
        let args = super::args("bwrap", &common, &Default::default(), &[]);
        for covered in [&archive, &data] {
            let covered = covered.display().to_string();
            assert!(
                args.windows(2)
                    .any(|w| w[0] == "--tmpfs" && w[1] == covered),
                "{covered} in {args:?}"
            );
        }
    }

    /// A system folder of sockets is covered where it's a folder, and
    /// passed over where it's missing or a link.
    #[test]
    fn system_socket_folders_are_covered_where_they_are() {
        let root = link_test_kit::TempDir::new("system-sockets");
        let sockets = root.join("daemon-socket");
        std::fs::create_dir(&sockets).unwrap();
        let linked = root.join("linked");
        std::os::unix::fs::symlink(&sockets, &linked).unwrap();
        let (sockets, linked, missing) = (
            sockets.display().to_string(),
            linked.display().to_string(),
            root.join("missing").display().to_string(),
        );
        let mut args = Vec::new();
        cover_sockets(&mut args, &[&sockets, &linked, &missing]);
        assert_eq!(args, ["--tmpfs".to_string(), sockets]);
    }

    /// Landlock's first ABI, whose layers refuse renames across folders,
    /// gets no file wall: git would fail inside.
    #[test]
    fn no_file_wall_where_landlock_cant_rename() {
        use std::num::NonZeroU32;
        assert!(!super::file_wall_here(None));
        assert!(!super::file_wall_here(NonZeroU32::new(1)));
        assert!(super::file_wall_here(NonZeroU32::new(2)));
        assert!(super::file_wall_here(NonZeroU32::new(6)));
    }

    /// A root-run sandbox reads no root-only file: `/sys`'s kernel,
    /// firmware and zram control are covered, and a key's file in `/etc`
    /// is never shown, wherever it is.
    #[test]
    fn a_root_run_sandbox_reads_no_root_only_file() {
        for covered in ["/sys/kernel", "/sys/firmware", "/sys/class/zram-control"] {
            assert!(
                crate::system::LINUX_ROOT_ONLY.contains(&covered),
                "{covered}"
            );
        }
        assert!(crate::system::etc_hidden("server-key.pem"));
        assert!(crate::system::etc_hidden("private"));
        assert!(!crate::system::etc_hidden("server-cert.pem"));
        assert!(!crate::system::etc_hidden("privateer"));
        let etc = link_test_kit::TempDir::new("etc-keys");
        std::fs::create_dir_all(etc.join("ssl/certs")).unwrap();
        std::fs::write(etc.join("ssl/certs/ca.pem"), "").unwrap();
        std::fs::write(etc.join("ssl/certs/host-key.pem"), "").unwrap();
        let mut args = Vec::new();
        crate::linux::etc::args(&etc, &mut args);
        let joined = args.join(" ");
        assert!(joined.contains("ssl/certs/ca.pem"), "{joined}");
        assert!(!joined.contains("host-key.pem"), "{joined}");
    }

    /// Only a version bubblewrap gave decides: no output, or none in it,
    /// is no answer, so nothing is kept for it.
    #[test]
    fn only_a_version_read_answers() {
        assert_eq!(
            version_disables_userns(Some(b"bubblewrap 0.9.0\n")),
            Some(true)
        );
        assert_eq!(
            version_disables_userns(Some(b"bubblewrap 0.6.1\n")),
            Some(false)
        );
        assert_eq!(version_disables_userns(Some(b"")), None);
        assert_eq!(version_disables_userns(Some(b"bubblewrap\n")), None);
        assert_eq!(version_disables_userns(None), None);
    }
}
