//! Install and maintain the ordinary box's privileged launch boundary.

use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::ensure;
use link_format::WithinExt;
use link_sandbox::cli::SandboxWorkersArgs as Workers;
use link_sandbox_contract::box_workers::{self, Settings};

pub(crate) fn workers(given: Workers) -> anyhow::Result<()> {
    if given.reap {
        return link_sandbox_confine::linux::workers::reap();
    }
    ensure!(
        link_machine::machine::kind()?.is_box(),
        "only a made box installs worker identities"
    );
    if given.install {
        let settings = Settings::Ordinary {
            controller: box_workers::current()?,
        };
        let exe = std::env::current_exe()?;
        // This first install precedes the closefrom rule. Its only input is stdin.
        let mut command = if unsafe { libc::geteuid() } == 0 {
            link_secretive::process::command(exe)
        } else {
            let mut command = link_secretive::process::command("/usr/bin/sudo");
            command.args(["-n", "-E", "--"]).arg(exe);
            command
        };
        let mut child = command
            .args(["workers", "--configure"])
            .stdin(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .within("worker settings input")?
            .write_all(&link_files::saved::print_json(&settings)?)?;
        ensure!(
            child.wait()?.success(),
            "installing the worker launcher failed"
        );
        return Ok(());
    }
    ensure!(given.configure, "use lnk sandbox workers --install");
    // SAFETY: geteuid has no failure.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "configuring the worker launcher requires root"
    );
    let mut bytes = Vec::new();
    std::io::stdin().take(4097).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "worker settings is too large");
    let settings: Settings = link_files::saved::parse_json(&bytes)?;
    let Settings::Ordinary { controller } = &settings else {
        anyhow::bail!("only the box controller installs this launcher")
    };
    link_sandbox_confine::linux::workers::provision()?;
    ensure!(
        controller.groups.first() == Some(&controller.gid),
        "the primary controller group comes first"
    );
    for dir in [
        link_files::layout::SystemRoot::Exec.path(),
        link_files::layout::SystemRoot::Settings.path(),
        PathBuf::from("/etc/sudoers.d"),
    ] {
        std::fs::create_dir_all(&dir)?;
        link_files::fs::check_folders(
            &dir.join("entry"),
            link_files::fs::Trust {
                owners: &[0],
                deny_mode: 0o022,
                ancestors: true,
                no_link: true,
            },
        )?;
    }
    let program = box_workers::program();
    let temporary =
        program.with_extension(format!("worker-install-{}", link_files::random_token()?));
    let mut installed = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)?;
    std::io::copy(
        &mut std::fs::File::open(std::env::current_exe()?)?,
        &mut installed,
    )?;
    installed.set_permissions(std::fs::Permissions::from_mode(0o755))?;
    installed.sync_all()?;
    drop(installed);
    std::fs::rename(&temporary, program)?;
    std::fs::File::open(program.parent().within("worker program folder")?)?.sync_all()?;
    link_files::write_private(
        box_workers::settings_path(),
        &link_files::saved::print_json(&settings)?,
    )?;
    std::fs::set_permissions(
        box_workers::settings_path(),
        std::fs::Permissions::from_mode(0o644),
    )?;
    // This changes descriptor handling only. It grants no additional sudo access.
    let sudoers = Path::new("/etc/sudoers.d/lnk-worker-descriptors");
    link_files::write_private(
        sudoers,
        format!(
            "Defaults!{} closefrom_override,!secure_path\n",
            box_workers::program().display()
        )
        .as_bytes(),
    )?;
    std::fs::set_permissions(sudoers, std::fs::Permissions::from_mode(0o440))?;
    let unit = format!(
        "[Unit]\nDescription=Remove interrupted Link worker identities\nAfter=local-fs.target\nBefore=multi-user.target\n\n[Service]\nType=oneshot\nExecStart={} sandbox workers --reap\n\n[Install]\nWantedBy=multi-user.target\n",
        box_workers::program().display()
    );
    link_files::write_private(
        Path::new("/etc/systemd/system/lnk-workers-reap.service"),
        unit.as_bytes(),
    )?;
    let status = link_secretive::process::command("/usr/bin/systemctl")
        .args(["enable", "lnk-workers-reap.service"])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .status()?;
    ensure!(status.success(), "enabling interrupted-worker cleanup");
    Ok(())
}
