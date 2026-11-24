//! Durable account journals and process cleanup pinned by kernel pidfds.

use std::fs::File;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context, ensure};
use link_sandbox_contract::box_workers::Controller;
use serde::{Deserialize, Serialize};

use super::identities::reserve_ids;
use super::views::Views;
use super::{RUNTIME, journal};
use crate::linux::session::{exited, pidfd};

fn admin(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = link_secretive::process::command(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("running {program}"))?;
    ensure!(status.success(), "{program} failed");
    Ok(())
}

pub(super) fn root() -> anyhow::Result<()> {
    // SAFETY: geteuid has no failure.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "the worker launcher requires root"
    );
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Account {
    pub(super) name: String,
    pub(super) uid: u32,
    pub(super) groups: Vec<(String, u32)>,
    supervisor: u32,
    started: String,
    boot: String,
    #[serde(skip)]
    pub(super) cleanup: bool,
}

link_files::specification!(Account, include_str!("account.schema.toml"));

fn start_time(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()?
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_string)
}

fn group_id(name: &str) -> anyhow::Result<Option<u32>> {
    Ok(std::fs::read_to_string("/etc/group")?
        .lines()
        .find_map(|line| {
            let mut parts = line.split(':');
            (parts.next()? == name).then_some(())?;
            parts.next()?;
            parts.next()?.parse().ok()
        }))
}

fn user_id(name: &str) -> anyhow::Result<Option<u32>> {
    Ok(std::fs::read_to_string("/etc/passwd")?
        .lines()
        .find_map(|line| {
            let mut parts = line.split(':');
            (parts.next()? == name).then_some(())?;
            parts.next()?;
            parts.next()?.parse().ok()
        }))
}

fn processes(uid: u32) -> anyhow::Result<Vec<OwnedFd>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse() else {
            continue;
        };
        // Pin before checking ownership. A later PID reuse cannot change
        // the task signalled through this handle; a dead one is skipped.
        let fd = match pidfd(pid) {
            Ok(fd) => fd,
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => continue,
            Err(e) => return Err(e.into()),
        };
        match std::fs::read_to_string(entry.path().join("status")) {
            Ok(status) => {
                let matches = status
                    .lines()
                    .find_map(|l| l.strip_prefix("Uid:"))
                    .is_some_and(|ids| ids.split_whitespace().any(|id| id.parse() == Ok(uid)));
                if matches && !exited(&fd, 0)? {
                    found.push(fd);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(found)
}

impl Account {
    fn path(&self) -> PathBuf {
        journal().join(format!("{}.json", self.name))
    }

    pub(super) fn create(controller: &Controller) -> anyhow::Result<Self> {
        let ids = reserve_ids(controller.groups.len())?;
        let token = link_files::random_token()?;
        let name = format!("lnk-w-{}", &token[..12]);
        let account = Self {
            uid: ids[0],
            groups: ids
                .iter()
                .enumerate()
                .map(|(n, id)| {
                    (
                        if n == 0 {
                            name.clone()
                        } else {
                            format!("{name}g{n}")
                        },
                        *id,
                    )
                })
                .collect(),
            name,
            supervisor: std::process::id(),
            started: start_time(std::process::id()).context("the launcher's identity")?,
            boot: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?,
            cleanup: true,
        };
        ensure!(
            user_id(&account.name)?.is_none(),
            "worker name already exists"
        );
        for (name, _) in &account.groups {
            ensure!(group_id(name)?.is_none(), "worker group already exists");
        }
        account.save()?;
        for (name, id) in &account.groups {
            admin(
                "/usr/sbin/groupadd",
                &["--system", "--gid", &id.to_string(), "--", name],
            )?;
        }
        admin(
            "/usr/sbin/useradd",
            &[
                "--system",
                "--key",
                "SYS_UID_MAX=2147483646",
                "--uid",
                &account.uid.to_string(),
                "--gid",
                &account.groups[0].1.to_string(),
                "--no-user-group",
                "--no-create-home",
                "--no-log-init",
                "--shell",
                "/usr/sbin/nologin",
                "--",
                &account.name,
            ],
        )?;
        ensure!(
            user_id(&account.name)? == Some(account.uid),
            "the new worker's identity changed"
        );
        Ok(account)
    }

    fn save(&self) -> anyhow::Result<()> {
        link_files::write_private(&self.path(), &link_files::saved::print_json(self)?)?;
        Ok(())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "polls an account's processes until they end"
    )]
    pub(super) fn clean(&self) -> anyhow::Result<()> {
        ensure!(
            self.name
                .strip_prefix("lnk-w-")
                .is_some_and(|x| x.len() == 12 && x.bytes().all(|b| b.is_ascii_hexdigit())),
            "invalid worker journal name"
        );
        // Check even the primary group before userdel can remove it.
        for (name, gid) in &self.groups {
            ensure!(
                name == &self.name
                    || name
                        .strip_prefix(&format!("{}g", self.name))
                        .is_some_and(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())),
                "invalid worker journal group"
            );
            if let Some(actual) = group_id(name)? {
                ensure!(
                    actual != 0 && (*gid == 0 || *gid == actual),
                    "the worker's group changed; retain its account"
                );
            }
        }
        if let Some(uid) = user_id(&self.name)? {
            ensure!(
                uid != 0 && (self.uid == 0 || self.uid == uid),
                "the worker's identity changed; retain its account"
            );
            let began = Instant::now();
            loop {
                // Reap descendants adopted through PR_SET_CHILD_SUBREAPER.
                // SAFETY: nonblocking wait of this process's children only.
                while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
                let processes = processes(uid)?;
                if processes.is_empty() {
                    break;
                }
                for process in processes {
                    // SAFETY: this handle pins the ownership-checked task.
                    let result = unsafe {
                        libc::syscall(
                            libc::SYS_pidfd_send_signal,
                            process.as_raw_fd(),
                            libc::SIGKILL,
                            std::ptr::null::<libc::siginfo_t>(),
                            0,
                        )
                    };
                    if result != 0 {
                        let error = std::io::Error::last_os_error();
                        ensure!(
                            error.raw_os_error() == Some(libc::ESRCH),
                            "terminating a worker: {error}"
                        );
                    }
                }
                ensure!(
                    began.elapsed() < Duration::from_secs(10),
                    "worker processes remain; its account is retained"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            admin("/usr/sbin/userdel", &["--", &self.name])?;
        }
        // userdel may remove its private group itself.
        for (name, _) in &self.groups {
            if group_id(name)?.is_some() {
                admin("/usr/sbin/groupdel", &["--", name])?;
            }
        }
        Views::existing(Path::new(RUNTIME).join(&self.name))?.clean()?;
        std::fs::remove_file(self.path())?;
        File::open(journal())?.sync_all()?;
        Ok(())
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        if self.cleanup
            && let Err(err) = self.clean()
        {
            link_format::aside(
                link_format::Report::warning(link_format::sentence!(
                    "Worker cleanup is incomplete."
                ))
                .why(link_format::Prose::from(link_format::Sentence::adopt(
                    &format!("{err:#}"),
                ))),
            );
        }
    }
}

/// Under the registry lock, collect interrupted launchers. A live PID must
/// also have the original process start time, so PID reuse retains no access.
pub(super) fn reap() -> anyhow::Result<()> {
    root()?;
    let dir = records()?;
    let _lock = dir.lock(libc::LOCK_EX)?;
    reap_in(&dir)
}

pub(super) fn records() -> anyhow::Result<link_files::nofollow::Dir> {
    link_files::create_private_dir(journal())?;
    link_files::fs::check_folders(
        &journal().join("entry"),
        link_files::fs::Trust {
            owners: &[0],
            deny_mode: 0o022,
            ancestors: true,
            no_link: true,
        },
    )?;
    // A first launch may have created either directory. Persist their
    // parent entries too before any passwd/group mutation can survive reboot.
    for path in journal().ancestors() {
        File::open(path)?.sync_all()?;
    }
    Ok(link_files::nofollow::Dir::open_unlinked(journal())?)
}

pub(super) fn reap_in(dir: &link_files::nofollow::Dir) -> anyhow::Result<()> {
    for name in dir.names()? {
        if Path::new(&name).extension().is_none_or(|x| x != "json") {
            continue;
        }
        let bytes = dir.read(&name, 64 * 1024)?;
        let account: Account = link_files::saved::parse_json(&bytes)?;
        ensure!(
            account.path().file_name() == Some(name.as_os_str()),
            "worker journal name mismatch"
        );
        if std::fs::read_to_string("/proc/sys/kernel/random/boot_id")? != account.boot
            || start_time(account.supervisor).as_deref() != Some(account.started.as_str())
        {
            account.clean()?;
        }
    }
    Ok(())
}
