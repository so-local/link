//! `lnk sandbox run`: a program in a sandbox.

use std::path::Path;

use link_format::{Done, Out, Report, WithinExt};
use link_sandbox::cli::{self};
use link_sandbox::opening::Opening;
use link_sandbox::spec::{self};
use link_sandbox_client::Programs;
use link_sandbox_client::Wrapped;

use super::*;

pub(crate) fn run_command(
    out: &mut Out,
    cli::SandboxRunArgs {
        spec,
        upstream,
        yes,
        with_fd,
        commands: command,
    }: cli::SandboxRunArgs,
    fields: link_spec::FieldFlags,
) -> Result<Done, Report> {
    let opening = Opening::of(spec, fields, upstream, yes);
    flagless(&command)?;
    let name = format!("run-{}", std::process::id());
    let with = with_fd.map(read_with).transpose()?;
    let mut opened = opening.opened(&name)?;
    if let Programs::Only(programs) = &opened.sandbox.commands {
        listed(programs, &command)?;
    }
    if let Some(read) = &opened.read {
        asked(out, read, &opened, opening.yes)?;
    }
    made(&opened.sandbox)?;
    let env = match with {
        Some(with) => {
            let home = link_files::try_home_dir()?;
            let path = link_files::absolute_path(&std::env::var_os("PATH").unwrap_or_default());
            let machine = spec::Machine {
                home: &home,
                settings: &link_files::try_installation_dir()?,
                hidden: &link_sandbox_contract::hidden_dirs(),
                spec: opened.read.as_ref().map(|read| read.path.as_path()),
                run_path: &path,
            };
            opened.sandbox.given(with, &machine)?
        }
        None => Default::default(),
    };
    Ok(Done::passed(run(
        out,
        &opened.sandbox,
        &command,
        None,
        env,
    )?))
}

/// Each folder `policy` writes that isn't there, made as it starts, as a
/// build folder is: a grant can't reach a folder that isn't there, so
/// what the program wrote in it would be lost with the run. Its spec's
/// folders were checked as it was read, and resolved through no link.
fn made(policy: &spec::Resolved) -> anyhow::Result<()> {
    for folder in &policy.write {
        if std::fs::symlink_metadata(folder).is_err() {
            std::fs::create_dir_all(folder).within(format!("making {}", folder.display()))?;
        }
    }
    Ok(())
}

/// Runs `command` under `policy`, and its exit status; in `home` (its
/// `HOME` and working folder) when given; `given`, variables its caller
/// gives it (`--with-fd`), under those the sandbox sets.
pub(crate) fn run(
    out: &mut Out,
    policy: &spec::Resolved,
    command: &[String],
    home: Option<&Path>,
    given: std::collections::BTreeMap<String, String>,
) -> Result<i32, Report> {
    ready(out, home.is_none())?;
    // Its proxy's secrets are handed to it on a pipe as it starts, never
    // written; it removes any other file it reads, as nothing starts it
    // again.
    let Wrapped { mut argv, env } = link_sandbox::wrap_handing(policy, command)?;
    // A proxy's line, its command's words as its arguments write them,
    // read back as the wrap wrote it.
    let words = cli::SandboxProxyArgs::default().argv();
    if let Some((exe, line)) = argv
        .split_first()
        .filter(|(_, line)| line.starts_with(&words))
    {
        let mut proxy = cli::SandboxProxyArgs::from_argv(line)
            .map_err(|err| anyhow::anyhow!("reading the proxy's line: {err}"))?;
        if let Some(launch) = &proxy.launch {
            link_sandbox::Launch::given(launch, given.keys().cloned())?;
        }
        proxy.delete_routes = true;
        argv = std::iter::once(exe.clone()).chain(proxy.argv()).collect();
    }
    let handed = link_sandbox::handed(policy)?;
    // The program writes to this terminal itself.
    let _hand = out.hand_over();
    // Only what it keeps and is given: none of this process's secrets.
    let start = |program: &str| {
        let mut child = link_secretive::process::command(program);
        child
            .env_clear()
            .envs(link_secretive::process::variables(|key| {
                KEPT.contains(&key)
            }))
            .envs(&given)
            .envs(&env);
        if let Some(home) = home {
            child.env("HOME", home).current_dir(home);
        }
        link_secretive::process::ends_with_parent(&mut child, libc::SIGTERM);
        child
    };
    // Its dot files' placeholders, where nothing is handed on: held here,
    // and bubblewrap started from here, as `sandbox placeholders` in
    // front of it would, with no process of its own between.
    let exe = std::env::current_exe().and_then(|e| e.canonicalize()).ok();
    if handed.is_empty()
        && let Some(args) = exe
            .as_deref()
            .and_then(|exe| link_sandbox_confine::linux::mounts::stage(exe, &argv))
    {
        let code = link_sandbox_confine::linux::mounts::held(
            placeholders_of(args)?,
            start,
            link_secretive::process::Terminal::Reaches,
        );
        clean_up(policy);
        return Ok(code?);
    }
    let mut child = start(&argv[0]);
    child.args(&argv[1..]);
    // macOS: a session of its own where it doesn't own the terminal, so
    // it never types into it.
    link_sandbox_proxy::run::own_session(&mut child);
    link_secretive::process::hand_to(
        &mut child,
        handed.iter().map(|(name, secret)| (name.as_str(), secret)),
    );
    // Stopped (a job cancelled, its services ended), it waits on what it
    // runs, then cleans up as at any end.
    let status = child
        .spawn()
        .and_then(|mut child| {
            #[cfg(unix)]
            link_secretive::process::stand_in_front(
                child.id(),
                link_secretive::process::Terminal::Reaches,
            );
            let status = child.wait();
            link_secretive::process::end_left(link_sandbox_contract::constants::BUBBLEWRAP);
            status
        })
        .within(format!("running {}", argv[0]));
    clean_up(policy);
    let status = status?;
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return Ok(128 + signal);
    }
    Ok(status.code().unwrap_or(1))
}

/// A program's exit status as a shell gives it: its code, or 128 and
/// the signal that ended it.
/// The placeholder stage `lnk-sandbox placeholders` is given.
pub(crate) fn placeholders(
    given: cli::SandboxPlaceholdersArgs,
) -> link_sandbox_confine::linux::mounts::Placeholders {
    link_sandbox_confine::linux::mounts::Placeholders {
        dir: given.dir,
        wanted: given.wanted,
        swept: given.swept,
        command: given.command,
    }
}

/// The placeholder stage this program wrote for itself to run
/// (`args`, after its `placeholders`), read as its command line is.
fn placeholders_of(
    args: &[String],
) -> anyhow::Result<link_sandbox_confine::linux::mounts::Placeholders> {
    let line = ["lnk-sandbox", "placeholders"]
        .into_iter()
        .map(std::ffi::OsString::from)
        .chain(args.iter().map(std::ffi::OsString::from));
    match link_sandbox::cli::parse_from(line)?.command.command {
        cli::Command::SandboxPlaceholders(given) => Ok(placeholders(given)),
        _ => anyhow::bail!("not the placeholder stage"),
    }
}

pub(crate) fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return 128 + signal;
    }
    status.code().unwrap_or(1)
}
