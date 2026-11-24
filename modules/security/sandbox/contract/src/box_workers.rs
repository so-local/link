//! The root launcher of an ordinary box. Packed boxes use their slot users;
//! computers keep the person's explicit keeper fallback.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

pub fn settings_path() -> &'static Path {
    static PATH: std::sync::LazyLock<PathBuf> =
        std::sync::LazyLock::new(|| link_files::layout::System::WorkerSettings.path());
    PATH.as_path()
}

pub fn program() -> &'static Path {
    static PATH: std::sync::LazyLock<PathBuf> =
        std::sync::LazyLock::new(|| link_files::layout::System::SandboxProgram.path());
    PATH.as_path()
}
pub const TEST_SETTINGS: &str = "LNK_TEST_WORKERS_SETTINGS";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controller {
    pub uid: u32,
    pub gid: u32,
    pub groups: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Settings {
    Ordinary { controller: Controller },
    Packed,
}

link_files::specification!(Settings, include_str!("box_workers.schema.toml"));

pub fn controller() -> anyhow::Result<Option<Controller>> {
    if !cfg!(target_os = "linux") {
        return Ok(None);
    }
    let test = link_secretive::test_var(TEST_SETTINGS);
    // Existing fake boxes replace init and sudo, not the host's accounts.
    // Worker integration stand-ins explicitly provide their launcher settings.
    if test.is_none() && link_secretive::test_var(link_machine::machine::TEST_KIND_FILE).is_some() {
        return Ok(None);
    }
    let path = test
        .map(PathBuf::from)
        .unwrap_or_else(|| settings_path().to_owned());
    let owner = if link_secretive::test_var(TEST_SETTINGS).is_some() {
        // SAFETY: geteuid has no failure.
        unsafe { libc::geteuid() }
    } else {
        0
    };
    let trust = link_files::fs::Trust {
        owners: &[0, owner],
        deny_mode: 0o022,
        ancestors: true,
        no_link: true,
    };
    let read = match link_files::fs::read_trusted(&path, trust, 4096, "the worker launcher") {
        Ok(read) => read,
        Err(err) if link_files::fs::missing(&err) => {
            ensure!(
                !link_machine::machine::kind()?.is_box(),
                "this box has no worker launcher: lnk sandbox workers --install installs one"
            );
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    match link_files::saved::parse_json(read.text.as_bytes())
        .context("reading the worker launcher")?
    {
        Settings::Packed => Ok(None),
        Settings::Ordinary { controller } => {
            // Named box users already have separate non-sudo identities. Only
            // root's installed record and the passwd identity select that path;
            // USER, HOME and workload environment cannot claim the exception.
            let uid = unsafe { libc::geteuid() };
            if uid != 0
                && uid != controller.uid
                && let Some(name) = link_machine::machine::user_name()
                && crate::box_users::check_name(&name).is_ok()
                && crate::box_users::load(
                    &crate::box_users::Places {
                        root: "/".into(),
                        owners: vec![0],
                    },
                    &name,
                )?
                .is_some()
            {
                return Ok(None);
            }
            Ok(Some(controller))
        }
    }
}

/// The root-owned helper. Only an already authorized controller can ask sudo;
/// no sudo rule is given to a worker. Preserve explicitly inherited descriptors
/// (the proxy's namespace handshake), as well as the child's given environment.
pub fn privileged(program: &Path) -> Command {
    // SAFETY: geteuid has no failure.
    if unsafe { libc::geteuid() } == 0 {
        link_secretive::process::command(program)
    } else {
        let mut command = link_secretive::process::command("/usr/bin/sudo");
        command
            .args(["-n", "-E", "-C", "2147483647", "--"])
            .arg(program);
        command
    }
}

pub fn current() -> anyhow::Result<Controller> {
    // SAFETY: these getters have no failure; getgroups fills the sized buffer.
    unsafe {
        let count = libc::getgroups(0, std::ptr::null_mut());
        ensure!(count >= 0, "reading the controller's groups");
        let mut groups = vec![0; count as usize];
        ensure!(
            libc::getgroups(count, groups.as_mut_ptr()) == count,
            "reading the controller's groups"
        );
        let gid = libc::getegid();
        groups.retain(|g| *g != gid);
        groups.sort_unstable();
        groups.dedup();
        groups.insert(0, gid);
        Ok(Controller {
            uid: libc::geteuid(),
            gid,
            groups,
        })
    }
}
