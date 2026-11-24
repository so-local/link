//! What runs on the machine an environment is on: `lnk env here`, which
//! `lnk` on your computer runs there over SSH (`lnk box run`), its
//! request on its input. It runs the stages, each in its sandbox (`lnk
//! sandbox run --spec`) or as root, starts the services and waits on
//! each, and runs a job's command, saying how each went on its error
//! output, each line after the request's mark.
//!
//! Values reach it on its input, never its command line, and go on to
//! each sandbox on a pipe (`lnk sandbox run --with-fd`): nothing secret
//! is written here. Its input closing (the SSH connection gone, the job
//! cancelled) stops everything it started.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, ensure};
use link_format::WithinExt;
use link_sandbox_client::{RunSecret, RunWith};
use link_sandbox_contract::box_users;
use link_secret_contract::route::Sealed;

use crate::file::{self, Env, Service, User};
use crate::remote::random_hex;
use crate::stage::Shipped;
use crate::values::Given;

pub use link_env_contract::here::*;

mod confining;
mod processes;
mod profile;
mod services;
mod stages;

pub use processes::sudo;
use processes::{kept, lnk_path, pass_fd, pipe, spawn, stop_all, stop_group, untrack, write_spec};
pub use services::serve;

/// A sandbox spec sent to run here: every path in it `~/` or in full,
/// none climbing with `..`, since a spec sent has no folder to be read
/// from. Checked where it's sent from, and again here.
pub fn check_shipped(spec: &link_sandbox_client::Spec) -> anyhow::Result<()> {
    if let Some(relative) = spec.relative_path() {
        bail!(
            "{:?} is relative or climbs with .., and a spec sent to run elsewhere has no \
             folder to be read from: write it from ~/ or in full, without ..",
            link_format::clean(relative)
        );
    }
    Ok(())
}

/// A run's [`Out`]: a folder from the home folder, without `..`, given as
/// a variable Link sets.
fn check_out(out: &Out) -> anyhow::Result<()> {
    let plain = !out.folder.as_os_str().is_empty()
        && out
            .folder
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    ensure!(
        plain,
        "{:?} isn't a folder from the home folder",
        link_format::clean(&out.folder.display().to_string())
    );
    let name = out.variable.strip_prefix("LNK_").unwrap_or_default();
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
        "{:?} isn't a variable Link sets (LNK_...)",
        link_format::clean(&out.variable)
    );
    Ok(())
}

/// `lnk env here`: reads its request from its input's first line, does
/// it, and exits with the job's code (0 for the rest), or 1 when setting
/// up failed.
///
/// First it says on its output what the request's secrets are sealed to:
/// a key made for this run alone, whose private half never leaves this
/// process and is dropped once they're opened.
pub fn main(shown: &mut link_format::Out) -> anyhow::Result<i32> {
    let recipient = link_secretive::Recipient::new();
    // The protocol's first line, at once, for the one reading it.
    let mut announced = Vec::new();
    recipient
        .announce(&mut announced)
        .within("saying what to seal to")?;
    shown.data(&announced);
    shown.hand_over();
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .within("reading the request")?;
    let request: Request = serde_json::from_str(&line).within("reading the request")?;
    let given: Given = recipient
        .open_json(&request.given, &about(&request.name))
        .within("opening what the request was given")?;
    drop(recipient);
    // The rest of its input is held open by whoever asked: once it
    // closes, nothing this started is left running.
    std::thread::spawn(|| {
        let mut rest = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut rest);
        stop_all();
        std::process::exit(143);
    });
    let code = run(&request, &given);
    stop_all();
    code
}

/// Says how a step went, after the mark.
fn say(mark: &str, what: &str) {
    let mut err = link_format::streams::protocol_on(link_format::streams::Stream::Err);
    let _ = writeln!(err, "{mark} {what}");
    let _ = err.flush();
}

/// [`main`]'s request, done, with what it was given, opened.
pub fn run(request: &Request, given: &Given) -> anyhow::Result<i32> {
    file::check_name(&request.name)?;
    let dir = files_dir(&request.name);
    let mut read = match &request.file {
        Some(path) => {
            let read = file::read(&dir.join(path))?;
            ensure!(
                read.file.name == request.name,
                "{} is environment {}, not {}",
                path.display(),
                read.file.name,
                request.name
            );
            read
        }
        None => {
            ensure!(
                matches!(request.action, Action::Run { .. }),
                "only a run of stages has no environment"
            );
            let file = Env::bare(request.name.clone());
            file::Read {
                text: String::new(),
                path: dir.clone(),
                dir: dir.clone(),
                file,
            }
        }
    };
    if let Some(workdir) = &request.workdir {
        ensure!(
            matches!(request.action, Action::Run { .. })
                && !workdir.as_os_str().is_empty()
                && workdir
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "a run's explicit working directory must stay beneath its home"
        );
        let folder = link_files::try_home_dir()?.join(workdir);
        link_files::create_private_dir(&folder)
            .within(format!("creating guest workspace {}", folder.display()))?;
        read.file.workdir = Some(workdir.to_string_lossy().into_owned());
    }
    // The file, for what names it again (an agent, a background service).
    let named = || request.file.as_deref().within("no environment's file");
    // Who each run is, and whether one needs a box, before any runs.
    let kind = link_machine::machine::kind().within("checking the workload machine kind")?;
    // A box's stages are for its OS: an image is built for one, and a
    // setup's stages are the image's.
    if let (
        Action::Image | Action::Setup,
        link_machine::machine::Kind::Box { os, .. },
        Some(named),
    ) = (&request.action, &kind, read.file.os)
    {
        ensure!(
            named == *os,
            "{} is for {}, and this box is {}: nothing ran",
            read.file.name,
            named.word(),
            os.word()
        );
    }
    if request.direct {
        link_sandbox_contract::isolation::check_unsandboxed()
            .within("running the environment with no sandbox")?;
    }
    if let Some(why) = needs_box(&request.action, &read.file)? {
        ensure!(
            kind.is_box(),
            "{why}, which happens on a box alone, and this machine is a computer: nothing ran"
        );
    }
    let here = Here {
        upstream: None,
        protected: None,
        file: &read.file,
        dir: &read.dir,
        home: link_files::try_home_dir().within("finding the workload home")?,
        mark: &request.mark,
        given,
        kind,
        direct: request.direct,
    };
    match &request.action {
        Action::Image | Action::Setup => {
            for stage in read.file.stages().iter().filter(|s| s.saved()) {
                here.stage(stage, Values::Sealed)?;
            }
            crate::carried::prepared(&read)?;
            Ok(0)
        }
        Action::Start => {
            for stage in read.file.stages().iter().filter(|s| !s.saved()) {
                here.stage(stage, Values::All)?;
            }
            for service in read.file.services() {
                here.start_service(service, named()?)?;
            }
            Ok(0)
        }
        Action::Run {
            stages,
            variables,
            secrets,
            out,
            profile,
        } => {
            if let Some(out) = out {
                check_out(out)?;
            }
            ensure!(!stages.is_empty(), "a run has a stage at least");
            // Root runs beside the proxy: a run with a root stage is given
            // no sealed secret, in any stage.
            ensure!(
                !stages.iter().any(Shipped::root) || given.sealed.is_empty(),
                "a run with a root stage is given no sealed secret"
            );
            file::check_groups(stages)?;
            for stage in stages.iter().flat_map(|s| s.members()) {
                if let Some(protected) = &stage.protected {
                    ensure!(
                        stage.user.is_none(),
                        "a run in a protected sandbox is as this machine's user"
                    );
                    link_sandbox_client::protected::flags(
                        protected,
                        stage
                            .sandbox
                            .as_deref()
                            .within("the protected sandbox's text")?,
                    )?;
                }
                file::check_name(&stage.name)?;
                let what = format!("the run's stage {}", stage.name);
                // Checked again here, as where it was sent from.
                if let Some(text) = &stage.sandbox {
                    let spec = link_spec::parse_spec::<link_sandbox_client::Spec>(text)
                        .within(format!("{what}'s sandbox"))?;
                    check_shipped(&spec).within(format!("{what}'s sandbox"))?;
                }
                match (&stage.sandbox, whose(stage.user.as_ref())) {
                    (Some(_), Whose::Root) => bail!("{what} as root runs outside any sandbox"),
                    (Some(_), Whose::Theirs(name)) => {
                        bail!("{what} runs in {name}'s own sandbox, and names none")
                    }
                    (None, Whose::Own) => bail!("{what} runs in a sandbox, or as root"),
                    _ => {}
                }
            }
            // What the run adds to the environment's values, never
            // replacing one: its own stages are given both.
            let mut values = read.file.clone();
            for (name, value) in variables {
                file::check_variable(name)?;
                ensure!(
                    values
                        .variables
                        .insert(name.clone(), value.clone())
                        .is_none(),
                    "{name} is the environment's already"
                );
            }
            // Its stages are given the secrets granted to the run, the
            // environment's names among them, and only those.
            for name in secrets.names().iter().chain(secrets.sealed()) {
                file::check_variable(name)
                    .map_err(|_| anyhow::anyhow!("a run's secret's name isn't a variable name"))?;
                ensure!(
                    !values.variables.contains_key(name),
                    "{name} is a variable of the environment's already"
                );
            }
            values.secrets = Some(secrets.clone());
            for stage in read.file.stages().iter().filter(|s| !s.saved()) {
                here.stage(stage, Values::All)?;
            }
            let mut started = Vec::new();
            for service in read.file.services() {
                if let Some(child) = here.service_for_job(service)? {
                    started.push(child);
                }
            }
            say(here.mark, "running");
            let code = here
                .run(stages, values, out.as_ref(), profile.as_ref())
                .within("running the sent guest stages");
            for child in started.iter_mut().rev() {
                stop_group(child.id());
                untrack(child);
                let _ = child.wait();
            }
            code
        }
        Action::Stop => {
            for service in read.file.services() {
                let (label, unit) = service_names(&read.file.name, &service.name);
                link_service::stop(&label, &unit)
                    .within(format!("stopping service {}", service.name))?;
            }
            Ok(0)
        }
        Action::Reset { service } => {
            let service = read.file.service(service)?;
            let Some(reset) = &service.reset else {
                bail!("service {} has no reset", service.name);
            };
            let status = here
                .confined(
                    service.sandbox.as_deref(),
                    service.user.as_ref(),
                    &sh(reset),
                    Vec::new(),
                    Values::None,
                )?
                .wait()?;
            ensure!(
                status.success(),
                "{}'s reset failed ({status})",
                service.name
            );
            say(here.mark, &format!("reset {}", service.name));
            Ok(0)
        }
    }
}

/// The runs `action` makes of `file`, each by what it is, and its user.
/// (A stage at every start is never root: the file says so.)
fn runs<'a>(action: &'a Action, file: &'a Env) -> anyhow::Result<Vec<(String, Option<&'a User>)>> {
    let stages = |saved: bool| {
        file.stages()
            .iter()
            .filter(move |s| s.saved() == saved)
            .flat_map(|s| s.members())
            .map(|s| (format!("stage {}", s.name), s.user.as_ref()))
    };
    let services = || {
        file.services()
            .iter()
            .map(|s| (format!("service {}", s.name), s.user.as_ref()))
    };
    Ok(match action {
        Action::Image | Action::Setup => stages(true).collect(),
        Action::Start => stages(false).chain(services()).collect(),
        Action::Run { stages: own, .. } => stages(false)
            .chain(services())
            .chain(
                own.iter()
                    .flat_map(|s| s.members())
                    .map(|s| (format!("the run's stage {}", s.name), s.user.as_ref())),
            )
            .collect(),
        Action::Stop => Vec::new(),
        Action::Reset { service } => {
            let service = file.service(service)?;
            vec![(
                format!("service {}'s reset", service.name),
                service.user.as_ref(),
            )]
        }
    })
}

/// Why `action` on `file` needs a box, if it does: an image is a box's,
/// and so is every run as root, or as another user of the box.
fn needs_box(action: &Action, file: &Env) -> anyhow::Result<Option<String>> {
    let mut why = matches!(action, Action::Image).then(|| "an image is built here".to_string());
    for (what, user) in runs(action, file)? {
        let needs = match whose(user) {
            Whose::Own => continue,
            Whose::Root => format!("{what} runs as root"),
            Whose::Theirs(name) => format!("{what} runs as {name}, a user of a box"),
        };
        why.get_or_insert(needs);
    }
    Ok(why)
}

/// Whose a run is on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Whose<'a> {
    /// The machine's own user's, in the run's sandbox: `user` left out, or
    /// naming them.
    Own,
    /// Root's, outside any sandbox.
    Root,
    /// Another user's of the box (`lnk box user add`), in their own
    /// sandbox and folder.
    Theirs(&'a str),
}

/// Whose a run with `user` is.
fn whose(user: Option<&User>) -> Whose<'_> {
    match user.map(|user| (user, user.named())) {
        None => Whose::Own,
        Some((_, None)) => Whose::Root,
        Some((_, Some(name))) if link_machine::machine::user_name().as_deref() == Some(name) => {
            Whose::Own
        }
        Some((_, Some(name))) => Whose::Theirs(name),
    }
}

/// Which of the secrets a run is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Values {
    /// None: a service, a root stage, a service's check or reset.
    None,
    /// The sealed ones, which the proxy holds and what runs never has: a
    /// sandboxed stage while an image is built (a private clone).
    Sealed,
    /// Every one: what runs at a start, and a job's command.
    All,
}

/// `sh -c <line>`.
fn sh(line: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), line.into()]
}

#[derive(Clone)]
struct Here<'a> {
    upstream: Option<&'a str>,
    /// The stage's protected sandbox, when it has one.
    protected: Option<&'a Path>,
    file: &'a Env,
    /// The file's folder, its specs' too.
    dir: &'a Path,
    home: PathBuf,
    mark: &'a str,
    given: &'a Given,
    /// What this machine is, read once: a run as root only on a box.
    kind: link_machine::machine::Kind,
    /// Its own runs in no sandbox ([`Request::direct`]).
    direct: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_the_machines_own_users_roots_or_another_users_of_the_box() {
        assert_eq!(whose(None), Whose::Own);
        assert_eq!(whose(Some(&User::root())), Whose::Root);
        if let Some(me) = link_machine::machine::user_name().filter(|me| me != "root") {
            assert_eq!(whose(Some(&User(me))), Whose::Own);
        }
        let ann = User("zz-ann".into());
        assert_eq!(whose(Some(&ann)), Whose::Theirs("zz-ann"));
    }
}
