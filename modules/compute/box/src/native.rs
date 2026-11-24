//! Boxes on this machine with no VM: what Box does without a provider.
//! A box here is a home folder, `~/Link/Boxes/<name>/home`, and a record
//! in Box's state beside the cloud boxes ([`crate::state::Here`]). Its
//! commands run on this machine from that home, inside the box's sandbox
//! (`lnk sandbox run`), or directly where it was made with
//! `--no-sandbox` and the machine's policy still allows that. Its
//! environment is set up and started as `lnk env setup here` does.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use anyhow::ensure;
use link_format::{Name, Path as Shown, Report, WithinExt, cmd, prose, sentence};
use link_sandbox_contract::isolation::{Mode, check_unsandboxed};

use crate::constants::{HERE_BOX, HOME};
use crate::said::said;
use crate::state::{Boxes, Confinement, Here, Machine};

/// The box `name`, when it's one Box placed here: its own record says so.
pub fn find(name: &str) -> anyhow::Result<Option<Machine>> {
    Ok(Boxes::load()?
        .get(name)
        .filter(|machine| machine.here.is_some())
        .cloned())
}

/// The box a command names, when it's one placed here: `name`'s, or, with
/// no name, the only box there is.
pub fn named(name: Option<&str>) -> anyhow::Result<Option<Machine>> {
    match name {
        Some(name) => find(name),
        None => Ok(match Boxes::load()?.boxes.as_slice() {
            [only] if only.here.is_some() => Some(only.clone()),
            _ => None,
        }),
    }
}

/// Where box `name`'s home is: `~/Link/Boxes/<name>/home`.
pub fn home_of(name: &str) -> anyhow::Result<PathBuf> {
    crate::state::check_name(name)?;
    Ok(link_files::layout::Home::Boxes
        .try_path()?
        .join(name)
        .join(HOME))
}

/// What runs a new box's commands, as `mode` asks: its sandbox,
/// from `file` (resolved already) or Link's own, each writing `home`; or
/// this machine directly, where its policy allows that.
pub fn confinement(
    mode: Mode<'_>,
    file: Option<&Path>,
    home: &Path,
) -> anyhow::Result<Confinement> {
    match mode {
        Mode::Direct => {
            check_unsandboxed()?;
            Ok(Confinement::Direct)
        }
        Mode::Sandbox(_) => Ok(Confinement::Sandbox {
            spec: sandbox_spec(file, home)?,
        }),
        Mode::Vm(_) => anyhow::bail!("a VM box is the VM provider's, never Box's own"),
    }
}

/// The sandbox spec a box runs in: `file`'s, as `lnk sandbox spec show` checks
/// it and prints it absolute, or Link's own; with its home among what it
/// writes. Captured once: the file is never read again.
fn sandbox_spec(file: Option<&Path>, home: &Path) -> anyhow::Result<String> {
    let mut spec = match file {
        Some(file) => {
            let output = link_core::lnk()
                .args(["sandbox", "spec", "show", "--spec"])
                .arg(file)
                .stdin(std::process::Stdio::null())
                .output()
                .within("running lnk sandbox spec")?;
            ensure!(
                output.status.success(),
                "the sandbox spec {} isn't one: {}",
                file.display(),
                link_format::clean(String::from_utf8_lossy(&output.stderr).trim())
            );
            link_spec::parse_spec::<link_sandbox_contract::Spec>(&String::from_utf8_lossy(
                &output.stdout,
            ))
            .within(format!("reading the sandbox spec {}", file.display()))?
        }
        None => link_sandbox_contract::Spec::default(),
    };
    let home = home.display().to_string();
    let files = spec.files.get_or_insert_with(Default::default);
    let write = files.write.get_or_insert_with(Vec::new);
    if !write.contains(&home) {
        write.push(home);
    }
    link_spec::print_spec(&spec)
}

/// The box's home, checked: a folder, never a link.
fn home(machine: &Machine) -> anyhow::Result<(&Here, &Path)> {
    let here = machine.here.as_ref().within("the box isn't one here")?;
    let home = here.home.as_path();
    let meta = std::fs::symlink_metadata(home).within(format!("reading {}", home.display()))?;
    ensure!(
        meta.is_dir(),
        "{} isn't a folder, so the box has no home",
        home.display()
    );
    Ok((here, home))
}

/// `argv` run in `machine`, from its home: in its sandbox, or directly.
fn command(machine: &Machine, argv: &[OsString]) -> anyhow::Result<Command> {
    let (here, home) = home(machine)?;
    let (program, rest) = argv.split_first().within("nothing to run")?;
    let mut command = match &here.confinement {
        Confinement::Sandbox { spec } => {
            let dir = link_files::layout::Hidden::Boxes.try_path()?.join("here");
            link_files::create_private_dir(&dir)?;
            let path = dir.join(format!("{}.toml", machine.name));
            link_files::write_private(&path, spec.as_bytes())?;
            let mut command = link_core::lnk();
            command
                .args(["sandbox", "run", "--yes", "--spec"])
                .arg(&path)
                .arg("--")
                .arg(program)
                .args(rest);
            command
        }
        Confinement::Direct => {
            // Asked again at every run: the policy may forbid it since.
            check_unsandboxed()?;
            let mut command = link_secretive::process::command(program);
            command.args(rest);
            command
        }
    };
    command.current_dir(home).env(HERE_BOX, &machine.name);
    Ok(command)
}

/// The report for a box here that's stopped.
fn stopped(machine: &Machine) -> anyhow::Result<()> {
    let shown = Name::new("box", &machine.name);
    if machine.state == "stopped" {
        return Err(said(
            Report::error(sentence!("Couldn't reach {}, which is stopped.", &shown))
                .why(prose!("Its environment's services are stopped."))
                .next(cmd!("lnk box start {}", &shown), "start it"),
        ));
    }
    Ok(())
}

/// `lnk box run <box> -- <args>`: this machine's `lnk` run in the box.
pub fn run_lnk(machine: &Machine, args: &[String]) -> anyhow::Result<ExitStatus> {
    stopped(machine)?;
    let mut argv = vec![link_core::lnk().get_program().to_os_string()];
    argv.extend(args.iter().map(OsString::from));
    command(machine, &argv)?
        .status()
        .within("running lnk in the box")
}

/// `lnk box ssh <box> [-- <command>]`: a shell in the box, or the command
/// line given, as SSH would run it.
pub fn shell(machine: &Machine, remote: &[String]) -> anyhow::Result<ExitStatus> {
    stopped(machine)?;
    let argv: Vec<OsString> = if remote.is_empty() {
        let shell = std::env::var_os("SHELL")
            .filter(|shell| Path::new(shell).is_absolute())
            .unwrap_or_else(|| "/bin/sh".into());
        vec![shell]
    } else {
        [
            "sh".into(),
            "-c".into(),
            crate::setup::command_line(remote).into(),
        ]
        .into()
    };
    command(machine, &argv)?
        .status()
        .within("running a shell in the box")
}

/// `lnk box copy <box> <files> <dest>`: each file into folder `dest` in
/// the box's home (`~` the home itself), made if it's missing: never
/// through a link, and never outside the home.
pub fn copy(machine: &Machine, files: &[PathBuf], dest: &str) -> anyhow::Result<()> {
    use link_files::nofollow::Dir;
    let (_, home) = home(machine)?;
    let below = dest
        .strip_prefix('~')
        .map(|rest| rest.trim_start_matches('/'))
        .unwrap_or(dest);
    let below = Path::new(below);
    ensure!(
        below
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "{} isn't a folder in the box's home: give it from ~",
        link_format::clean(dest)
    );
    let mut folder = Dir::open_unlinked(home).within(format!("opening {}", home.display()))?;
    for part in below.components() {
        folder = folder
            .create_folder(part.as_os_str(), 0o700)
            .within(format!("making {dest} in the box"))?;
    }
    // Every file opened and checked before any is written.
    let opened = files
        .iter()
        .map(|file| {
            let input = crate::ssh::open_unlinked(file, &link_files::home_dir())?;
            let name = file
                .file_name()
                .within(format!("{} has no name to copy it as", file.display()))?;
            Ok((file, name, input))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    for (file, name, mut input) in opened {
        let mode = std::os::unix::fs::PermissionsExt::mode(&input.metadata()?.permissions());
        // A unique name beside it, moved into place once whole.
        let aside = OsString::from(link_files::layout::temporary_name());
        let mut output = folder
            .create_new(&aside, mode & 0o777)
            .within(format!("writing {} in the box", file.display()))?;
        std::io::copy(&mut input, &mut output).within(format!("copying {}", file.display()))?;
        output.sync_all()?;
        folder
            .rename(&aside, name)
            .within(format!("copying {}", file.display()))?;
    }
    Ok(())
}

/// The box's environment, read back from what was captured when it was
/// made, and whether it runs directly; none, a box with no environment.
fn environment(
    machine: &Machine,
) -> anyhow::Result<
    Option<(
        crate::machine::StagedEnvironment,
        link_env_contract::file::Read,
        bool,
    )>,
> {
    let Some(input) = &machine.environment else {
        return Ok(None);
    };
    let (staged, read) = crate::machine::staged_environment(input)?;
    let direct = matches!(
        machine.here.as_ref().map(|here| &here.confinement),
        Some(Confinement::Direct)
    );
    Ok(Some((staged, read, direct)))
}

/// Box `machine`'s environment set up and started, when it has one: its
/// stages an image would save, then its stages at every start and its
/// services.
pub fn set_up(machine: &Machine) -> anyhow::Result<()> {
    if let Some((_staged, read, direct)) = environment(machine)? {
        link_env_contract::remote::set_up_here(&read, direct).map_err(said)?;
        link_env_contract::remote::start_here(&read, direct).map_err(said)?;
    }
    Ok(())
}

/// `lnk box start`: its environment's stages at every start and its
/// services, started again.
pub fn start(machine: &Machine) -> anyhow::Result<()> {
    if let Some((_staged, read, direct)) = environment(machine)? {
        link_env_contract::remote::start_here(&read, direct).map_err(said)?;
    }
    set_state(&machine.name, "running")
}

/// `lnk box stop`: its environment's services stopped.
pub fn stop(machine: &Machine) -> anyhow::Result<()> {
    if let Some((_staged, read, _)) = environment(machine)? {
        link_env_contract::remote::stop_here(&read).map_err(said)?;
    }
    set_state(&machine.name, "stopped")
}

/// Its saved state, as `state`.
fn set_state(name: &str, state: &str) -> anyhow::Result<()> {
    Boxes::update(|boxes| {
        if let Some(saved) = boxes.get_mut(name)
            && saved.here.is_some()
        {
            saved.state = state.into();
        }
    })
}

/// `lnk box delete`'s end for a box here: its services stopped and its
/// record removed. Its home stays unless `home` says to remove it.
pub fn remove(machine: &Machine, home: bool) -> anyhow::Result<()> {
    stop(machine)?;
    Boxes::update(|boxes| {
        if boxes
            .get(&machine.name)
            .is_some_and(|saved| saved.machine_id == machine.machine_id && saved.here.is_some())
        {
            boxes.remove(&machine.name);
        }
    })?;
    if let Ok(dir) = link_files::layout::Hidden::Boxes.try_path() {
        let _ = std::fs::remove_file(dir.join("here").join(format!("{}.toml", machine.name)));
    }
    if home {
        remove_home(machine)?;
    }
    Ok(())
}

/// The box's home and the folder it's in, gone: only where Box makes it,
/// and never through a link.
fn remove_home(machine: &Machine) -> anyhow::Result<()> {
    let expected = home_of(&machine.name)?;
    let here = machine.here.as_ref().within("the box isn't one here")?;
    ensure!(
        here.home == expected,
        "{} isn't where Box keeps {}'s home, so it stays",
        here.home.display(),
        machine.name
    );
    let folder = expected.parent().within("the home's folder")?;
    match std::fs::symlink_metadata(folder) {
        Ok(meta) if meta.is_dir() => {
            std::fs::remove_dir_all(folder).within(format!("removing {}", folder.display()))
        }
        Ok(_) => anyhow::bail!("{} isn't a folder, so it stays", folder.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).within(format!("reading {}", folder.display())),
    }
}

/// A new box here's home, made empty and private; one there already is
/// refused, never reused.
pub fn make_home(name: &str) -> anyhow::Result<PathBuf> {
    let home = home_of(name)?;
    if std::fs::symlink_metadata(&home).is_ok() {
        return Err(said(
            Report::error(sentence!(
                "Couldn't make the box {}, since its home is there already.",
                Name::new("box", name)
            ))
            .why(prose!(
                "{} is left from another box of its name; it's yours to keep or remove.",
                Shown::new(&home)
            ))
            .exit(link_format::Exit::Usage),
        ));
    }
    let folder = home.parent().within("the home's folder")?;
    link_files::create_private_dir(folder).within(format!("making {}", folder.display()))?;
    link_files::create_private_dir(&home).within(format!("making {}", home.display()))?;
    Ok(home)
}

/// Whether a run is in a box here, and which: what `lnk box run` and
/// `ssh` tell what they run. A box user is a login on a machine of its
/// own, so `lnk box user` refuses there.
pub fn running_in() -> Option<String> {
    std::env::var(HERE_BOX).ok().filter(|name| !name.is_empty())
}
