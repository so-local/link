//! `lnk sandbox enter`: what sshd runs for a key let into a sandbox (a
//! box's users', `lnk box user`), and what a run as one of them runs.

use std::path::Path;

use link_format::{Done, Exit, Out, Path as Shown, Report, WithinExt, cmd, prose, sentence};
use link_sandbox::cli::{self};
use link_sandbox::spec::{self, Spec};
use link_sandbox_client::{At, Confinement, Programs, ProxyAt, Way};
use link_secretive::Duplicate;

use super::*;

/// `lnk sandbox enter --spec <file> --folder <dir>`: the spec read as any
/// is, `~` the home it runs with, and their command (after `--`, else
/// `SSH_ORIGINAL_COMMAND`: a command, scp, sftp) or a login shell, in its
/// sandbox, in the folder, given the variables `--keep-env` names. With
/// `--check`, what the sandbox opens, a line each, and nothing run.
pub(crate) fn enter(out: &mut Out, entered: cli::SandboxEnterArgs) -> Result<Done, Report> {
    let cli::SandboxEnterArgs {
        spec: file,
        folder,
        check,
        keep_env,
        command,
    } = entered;
    if !folder.is_absolute() {
        return Err(
            Report::error(sentence!("{} isn't absolute.", Shown::new(&folder)))
                .why(prose!("--folder takes a path from the root."))
                .exit(Exit::Usage),
        );
    }
    // Named for its file: a box user's is theirs.
    let name = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string();
    link_sandbox_contract::box_users::check_name(&name)
        .within(format!("{} names the sandbox", file.display()))?;
    let read = link_spec::read_spec::<Spec>(&file)?;
    let home = link_files::try_home_dir()?;
    let spec = read.spec.on(spec::Os::here())?.absolute(&read.dir, &home)?;
    let mut policy = opened(&name, &folder, &spec, Some(&read.path))?;
    if check {
        // The session's own, its folder written among them.
        // A line each, as a program reads them.
        for line in policy.describe() {
            out.data(format!("{}\n", link_format::clean(&line)).as_bytes());
        }
        return Ok(Done::shown());
    }
    link_files::create_private_dir(&folder).within("making its folder")?;
    let command = match command.join(" ") {
        given if !given.trim().is_empty() => given,
        _ => std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default(),
    };
    let command = match command.trim().is_empty() {
        false => vec!["/bin/bash".to_string(), "-c".into(), command],
        true => vec!["/bin/bash".to_string(), "-l".into()],
    };
    // What a run as them is given, from this program's environment.
    let mut given = std::collections::BTreeMap::new();
    for name in keep_env {
        let value = link_secretive::Secret::from_env(&name)
            .or_else(|| std::env::var(&name).ok().map(link_secretive::Secret::new))
            .within(format!("--keep-env {name}: no such variable here"))?;
        given.insert(name, value.expose_owned());
    }
    // Their SSH session's terminal is theirs alone.
    policy.terminal(std::io::IsTerminal::is_terminal(&std::io::stdin()));
    let _settings = SessionSettings::outside(&folder)?;
    let code = run(out, &policy, &command, Some(&folder), given);
    if let Some(proxy) = link_sandbox::proxy_of(&policy) {
        let _ = std::fs::remove_dir_all(&proxy.dir);
    }
    Ok(Done::passed(code?))
}

/// A box user's whole home is work, so its session's control files live
/// outside that grant. The original settings stay in the policy's hidden
/// paths; this private temporary folder is removed when the session ends.
struct SessionSettings {
    path: std::path::PathBuf,
    previous: Option<std::ffi::OsString>,
}

impl SessionSettings {
    fn outside(folder: &Path) -> anyhow::Result<Option<Self>> {
        if !link_files::try_installation_dir()?.starts_with(folder) {
            return Ok(None);
        }
        let path = link_files::private_temp_dir("box-session-settings")?;
        anyhow::ensure!(
            !path.starts_with(folder),
            "session settings must be outside its writable folder; use a TMPDIR outside it"
        );
        let previous = std::env::var_os(link_files::INSTALLATION_DIR_VAR);
        // SAFETY: enter is a single-threaded command; it launches the proxy
        // in a child, never a runtime on this process.
        unsafe {
            std::env::set_var(link_files::INSTALLATION_DIR_VAR, &path);
        }
        Ok(Some(Self { path, previous }))
    }
}

impl Drop for SessionSettings {
    fn drop(&mut self) {
        // SAFETY: the session's child has ended; this command stays single threaded.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(link_files::INSTALLATION_DIR_VAR, value),
                None => std::env::remove_var(link_files::INSTALLATION_DIR_VAR),
            }
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The sandbox `name`'s `spec` (one OS's, absolute) makes, every refusal
/// checked (`file`: where it was read from), with `folder`, which is
/// always theirs to write, first. Their session
/// is `/bin/bash`, which its `[commands]` must let run.
fn opened(
    name: &str,
    folder: &Path,
    spec: &Spec,
    file: Option<&Path>,
) -> Result<spec::Resolved, Report> {
    let home = link_files::try_home_dir()?;
    let path = link_files::absolute_path(&std::env::var_os("PATH").unwrap_or_default());
    let confinement = spec::Resolved::new(Confinement::of(
        spec,
        At {
            name,
            dir: Path::new("/"),
            home: &home,
            path: Some(&path),
            way: Way::Proxy(ProxyAt {
                upstream: "direct".into(),
                dir: link_sandbox::run_proxy_dir()?,
                ..Default::default()
            }),
        },
    )?)?;
    spec::check(
        &confinement,
        &spec::Machine {
            home: &home,
            settings: &link_files::try_installation_dir()?,
            hidden: &link_sandbox_contract::hidden_dirs(),
            spec: file,
            run_path: &path,
        },
    )?;
    spec::network_here(&confinement, spec::on_cloud())?;
    let programs = spec::programs(
        &confinement,
        &link_sandbox_confine::programs::temp_folders(),
    )?;
    if let Programs::Only(programs) = &programs {
        listed(programs, &["/bin/bash".to_string()]).map_err(|report| {
            report
                .why(prose!(
                    "A session is /bin/bash, which [commands] allow must list."
                ))
                .next(
                    cmd!(
                        "lnk sandbox spec show --spec {}",
                        Shown::new(file.unwrap_or(Path::new("<file>")))
                    ),
                    "see what the spec allows",
                )
        })?;
    }
    let mut policy = confinement.duplicate();
    policy.set_programs(programs);
    policy.hide_more(link_sandbox_contract::hidden_dirs());
    // Their folder is theirs, as an agent's is: nothing of yours runs there.
    policy.session_folder(folder)?;
    Ok(policy)
}
