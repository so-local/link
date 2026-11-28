//! The environment on a box, from your computer: each step is `lnk box
//! ...`, as a person would type it (`docs/architecture/modules/
//! contracts.md`), and what runs there is `lnk env here`, asked over SSH
//! (`lnk box run`).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::ensure;
use link_format::clean;
use link_format::{
    Done, Exit, Name, Out, Path as Shown, Prose, Question, Refuser, Report, Sentence, WithinExt,
    cmd, prose, sentence,
};
use link_secretive::Duplicate;
use serde::Deserialize;

use crate::file::{self, Named, Read};
use crate::here::{self, Action, Request};
use crate::values::{Blank, Given};

mod image;
mod session;
pub use image::{build_image, ensure_image, image, image_here, image_lifetime, root_stage, setup};
pub use session::{Ended, Log, Said, Session, random_hex, said};

/// `lnk <args>`, its output this one's; an error naming it when it fails.
pub fn lnk(args: &[&str]) -> anyhow::Result<()> {
    let status = link_core::lnk()
        .args(args)
        .stdin(Stdio::null())
        .status()
        .within(format!("running lnk {}", clean(&args.join(" "))))?;
    ensure!(
        status.success(),
        "lnk {} failed ({status})",
        clean(&args.join(" "))
    );
    Ok(())
}

/// `lnk <args>`'s output, read as JSON.
pub fn lnk_json<T: serde::de::DeserializeOwned>(args: &[&str]) -> anyhow::Result<T> {
    let output = link_core::lnk()
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .within(format!("running lnk {}", clean(&args.join(" "))))?;
    ensure!(
        output.status.success(),
        "lnk {} failed ({})",
        clean(&args.join(" ")),
        output.status
    );
    serde_json::from_slice(&output.stdout).within(format!(
        "reading what lnk {} printed",
        clean(&args.join(" "))
    ))
}

/// The boxes this computer knows.
pub fn boxes() -> anyhow::Result<Vec<link_box_contract::Listed>> {
    list_boxes(true)
}

/// Your boxes (`lnk box list --json`): with `here`, those this computer
/// knows, asking no cloud; else those your clouds know too.
pub fn list_boxes(here: bool) -> anyhow::Result<Vec<link_box_contract::Listed>> {
    let output = link_core::lnk()
        .args(["box", "list"])
        .args(here.then_some("--here"))
        .arg("--json")
        .stdin(std::process::Stdio::null())
        .output()
        .within("running lnk box list")?;
    ensure!(
        output.status.success(),
        "lnk box list failed: {}",
        clean(String::from_utf8_lossy(&output.stderr).trim())
    );
    serde_json::from_slice(&output.stdout).within("reading lnk box list --json")
}

/// Whether `place` is a box (else it's a cloud account). Either is the box
/// module's, installed first if it's missing.
pub fn is_box(place: &str) -> anyhow::Result<bool> {
    link_core::modules::ensure("box", &format!("--on {place}"))?;
    Ok(boxes()?.iter().any(|b| b.name == place))
}

/// Whether `account` has a ready image with `hash`.
pub fn has_image(account: &str, hash: &str) -> anyhow::Result<bool> {
    let images: Vec<serde_json::Value> = lnk_json(&[
        "box", "image", "list", "--cloud", account, "--hash", hash, "--json",
    ])?;
    Ok(!images.is_empty())
}

/// `name` cut so `-<suffix>` after it is a box's name (at most 30).
pub fn named(name: &str, suffix: &str) -> String {
    let room = 30usize.saturating_sub(suffix.len() + 1);
    let cut: String = name.chars().take(room).collect();
    format!("{}-{suffix}", cut.trim_end_matches('-'))
}

/// The first `<name>-<n>` that's no box here.
pub fn free_name(name: &str) -> anyhow::Result<String> {
    let taken: Vec<String> = boxes()?.into_iter().map(|b| b.name).collect();
    (1..1000)
        .map(|n| named(name, &n.to_string()))
        .find(|n| !taken.contains(n))
        .within("no free name for a box")
}

/// A box to make: on the OS and processor named, as its box spec says
/// (none, the least that works), from the image with its hash.
#[derive(Clone, Copy, Debug, Default)]
pub struct NewBox<'a> {
    pub named: Named,
    pub spec: Option<&'a Path>,
    pub hash: Option<&'a str>,
    /// When its cloud removes it, whatever happened here.
    pub deadline: Option<u64>,
    /// Removes itself once its job ends (a job's box alone).
    pub self_remove: bool,
    /// A region or zone of the cloud (`--zone`).
    pub place: Option<&'a str>,
}

/// The slot that names `account`: an account that is one machine is
/// `--on`, any other cloud account `--cloud`.
fn slot_of(accounts: &[(String, bool)], account: &str) -> &'static str {
    let one_machine = accounts
        .iter()
        .any(|(name, machine)| name == account && *machine);
    if one_machine { "--on" } else { "--cloud" }
}

/// Asks before making what `account`'s cloud bills, `prompt` saying
/// what and `why` what each costs; `yes` (`--yes`) answers it. An account
/// that is one machine bills no one, so nothing is asked of it. A no, or no
/// one here to answer, makes nothing: the refusal names `--yes`.
pub fn billed(
    out: &mut Out,
    account: &str,
    prompt: Sentence,
    why: Prose,
    yes: bool,
) -> Result<(), Report> {
    if slot_of(&crate::cli::accounts(), account) == "--on" {
        return Ok(());
    }
    if out.ask(Question::confirm(prompt, true).why(why).yes(yes))? {
        return Ok(());
    }
    Err(Report::refusal(Refuser::Person, sentence!("Made nothing."))
        .next(out.typed(), "make it when you're ready"))
}

/// Makes the box `machine` in `account`, as `new` says, once its person
/// said yes ([`billed`]), so its `lnk box create` asks nothing again. Never signed in
/// to the account (`--lnk.login=false`): an image build or a job runs code it
/// was handed, and a box signed in would hand that code a token of the
/// account's.
pub fn create(account: &str, machine: &str, new: NewBox) -> anyhow::Result<()> {
    let slot = slot_of(&crate::cli::accounts(), account);
    let mut args = vec!["box", "create", slot, account, "--name", machine, "--yes"];
    let spec = new.spec.map(|p| p.display().to_string());
    if let Some(spec) = &spec {
        args.extend(["--spec", spec]);
    }
    if let Some(os) = new.named.os {
        args.extend(["--machine.os", os.word()]);
    }
    if let Some(arch) = new.named.arch {
        args.extend(["--machine.arch", arch.word()]);
    }
    if let Some(hash) = new.hash {
        args.extend(["--image-hash", hash]);
    }
    let deadline = new.deadline.map(|secs| format!("{secs}s"));
    if let Some(deadline) = &deadline {
        args.extend(["--deadline", deadline]);
    }
    if let Some(place) = new.place {
        args.extend(["--zone", place]);
    }
    if new.self_remove {
        args.push("--self_remove");
    }
    args.push("--lnk.login=false");
    lnk(&args)
}

/// Readies box `machine` for an environment: this module and the sandbox
/// there, the sandbox able to run.
pub fn ready(machine: &str) -> anyhow::Result<()> {
    lnk(&[
        "box", "run", machine, "--", "module", "add", "env", "sandbox",
    ])?;
    lnk(&[
        "box", "run", machine, "--", "sandbox", "check", "--ready", "--yes",
    ])
}

/// Copies the file and every file it names to box `machine`, each to its
/// place from the file's folder, in Link's settings there: the file as it
/// was read and checked, never read again.
pub fn copy(read: &Read, machine: &str) -> anyhow::Result<()> {
    let base = here::files_dir_from_home(&read.file.name);
    let staged = link_files::private_temp_dir("env-file").within("staging the file")?;
    let file = staged.join(read.path.file_name().within("its name")?);
    let copied = std::fs::write(&file, &read.text)
        .within("staging the file")
        .and_then(|()| copy_named(read, machine, &base, &file));
    let _ = std::fs::remove_dir_all(&staged);
    copied
}

/// The login an existing guest gives its environment's setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuestLogin<'a> {
    /// The guest's root, as a Linux guest gives: any step can run.
    Root,
    /// The guest's approved ordinary user and no root, as a macOS guest
    /// gives: every step runs as that user, in its sandbox, under its home.
    Ordinary(&'a str),
}

/// Refused before the guest is touched when one of `file`'s stages or
/// services needs more than `login` gives: root, or another of a box's
/// users, in a guest whose login is one ordinary user.
pub fn check_login(file: &file::Env, login: GuestLogin) -> Result<(), Report> {
    let GuestLogin::Ordinary(user) = login else {
        return Ok(());
    };
    let stages = file
        .stages()
        .iter()
        .flat_map(|s| s.members())
        .map(|s| ("stage", Name::new("stage", &s.name), s.user.as_ref()));
    let services = file
        .services()
        .iter()
        .map(|s| ("service", Name::new("service", &s.name), s.user.as_ref()));
    for (noun, what, runs_as) in stages.chain(services) {
        let wanted = match runs_as {
            None => continue,
            Some(runs_as) if runs_as.named() == Some(user) => continue,
            Some(runs_as) => runs_as.to_string(),
        };
        return Err(Report::refusal(
            link_format::Refuser::Policy("guest users".into()),
            sentence!(
                "Couldn't prepare this guest, since the {} {} runs as {}.",
                noun,
                &what,
                wanted
            ),
        )
        .why(prose!(
            "The guest has one user, its approved {}, and no root, so nothing was copied or run; a Linux guest runs any step.",
            user
        )));
    }
    Ok(())
}

/// Apply uncached preparation privately in an existing guest, without image selection.
/// Every step is checked against the guest's `login` before anything reaches it.
pub fn prepare_private(
    machine: &str,
    read: &Read,
    login: GuestLogin,
    materialize: bool,
    start: bool,
    others: bool,
) -> Result<(), Report> {
    check_login(&read.file, login)?;
    copy(read, machine)?;
    if materialize {
        let given = if others {
            Given::default()
        } else {
            image::build_given(read)?
        };
        let mut session = Session::start(machine, read, Action::Setup, given)?;
        let ended = session.wait(Some(Duration::from_secs(3600)), &|| false)?;
        if ended.code != Some(0) {
            return Err(
                Report::error(sentence!("Couldn't prepare the guest's environment."))
                    .why(prose!("Its setup failed in the guest.")),
            );
        }
    }
    if start {
        let given = if others {
            Given::default()
        } else {
            crate::values::of_yours(read)?
        };
        let mut session = Session::start(machine, read, Action::Start, given)?;
        let ended = session.wait(Some(Duration::from_secs(3600)), &|| false)?;
        if ended.code != Some(0) {
            return Err(
                Report::error(sentence!("Couldn't start the guest's environment."))
                    .why(prose!("Its start failed in the guest.")),
            );
        }
    }
    Ok(())
}

/// [`copy`]'s copies: `file`, the file as read, and every file it names.
fn copy_named(read: &Read, machine: &str, base: &str, file: &Path) -> anyhow::Result<()> {
    let mut by_folder: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    by_folder
        .entry(base.to_string())
        .or_default()
        .push(file.display().to_string());
    for path in read.file.named_files() {
        let folder = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(parent) => format!("{base}/{}", parent.display()),
            None => base.to_string(),
        };
        by_folder
            .entry(folder)
            .or_default()
            .push(read.dir.join(&path).display().to_string());
    }
    for (folder, files) in by_folder {
        let mut args: Vec<&str> = vec!["box", "copy", machine];
        args.extend(files.iter().map(String::as_str));
        args.push(&folder);
        lnk(&args)?;
    }
    Ok(())
}

/// Copies the file and every file it names to where `lnk env here` reads
/// them on this machine, in Link's settings, the file as it was read and
/// checked; a file there already, the one an agent's job names, stays as
/// it is.
pub fn copy_here(read: &Read) -> anyhow::Result<()> {
    let base = here::files_dir(&read.file.name);
    let to = base.join(read.path.file_name().within("its name")?);
    if std::fs::canonicalize(&to).ok().as_ref() != Some(&read.path) {
        link_files::create_private_dir(&base).within(format!("making {}", base.display()))?;
        std::fs::write(&to, &read.text).within(format!("writing {}", to.display()))?;
    }
    let mut files = Vec::new();
    for path in read.file.named_files() {
        files.push((read.dir.join(&path), base.join(&path)));
    }
    for (from, to) in files {
        let same = matches!(
            (std::fs::canonicalize(&from), std::fs::canonicalize(&to)),
            (Ok(a), Ok(b)) if a == b
        );
        if same {
            continue;
        }
        let folder = to.parent().within("its folder")?;
        link_files::create_private_dir(folder).within(format!("making {}", folder.display()))?;
        std::fs::copy(&from, &to).within(format!(
            "copying {} to {}",
            from.display(),
            to.display()
        ))?;
    }
    Ok(())
}

/// Environment `read` set up on this machine itself, as `lnk env setup
/// --here` does: the stages an image would save, each in its sandbox, or
/// `direct`ly in none. Nothing runs as root, and no image is saved.
pub fn set_up_here(read: &Read, direct: bool) -> Result<(), Report> {
    copy_here(read)?;
    let given = image::build_given(read)?;
    run_here(read, Action::Setup, given, direct, "set up")
}

/// Environment `read` started on this machine itself: its stages at
/// every start, then its services as background services, which outlive
/// this until [`stop_here`].
pub fn start_here(read: &Read, direct: bool) -> Result<(), Report> {
    copy_here(read)?;
    let given = crate::values::of_yours(read)?;
    run_here(read, Action::Start, given, direct, "start")
}

/// The services [`start_here`] started, stopped.
pub fn stop_here(read: &Read) -> Result<(), Report> {
    copy_here(read)?;
    run_here(read, Action::Stop, Given::default(), false, "stop")
}

/// `action` of environment `read` run on this machine, its steps said as
/// they're done; `what` names it when it fails.
fn run_here(
    read: &Read,
    action: Action,
    given: Given,
    direct: bool,
    what: &str,
) -> Result<(), Report> {
    let mut session = Session::here(read, action, given, direct)?;
    let ended = session.wait(None, &|| false)?;
    if ended.code != Some(0) {
        let at = ended
            .failed
            .as_deref()
            .map(|stage| format!(" at {}", clean(stage)))
            .unwrap_or_default();
        return Err(Report::error(sentence!(
            "Couldn't {} {} on this machine.",
            what,
            Name::new("environment", &read.file.name)
        ))
        .why(prose!("It failed{}.", at)));
    }
    Ok(())
}

/// `lnk env create --cloud <cloud> --spec <file>`: a box
/// named `name`, as the box spec `spec` says (none, the least that
/// works), from the environment's image, built first when there's none;
/// each billed by the cloud, so made only once you say yes (`yes`,
/// `--yes`).
pub fn start(
    out: &mut Out,
    read: &Read,
    account: &str,
    name: Option<&str>,
    spec: Option<&Path>,
    yes: bool,
) -> Result<Done, Report> {
    let read = read.clone();
    // The environment's workload, minted the first time a box is made
    // from its file, and the same at every later one.
    crate::workloads::of("env", &read.path)?;
    let given = crate::values::of_yours(&read)?;
    let named = crate::file::platform_named(Some(&read), spec)?;
    let imaged = read.file.stages().iter().any(|s| s.saved());
    let machine = match name {
        Some(name) => name.to_string(),
        None => free_name(&read.file.name)?,
    };
    let shown = Name::new("box", &machine);
    let cloud = Name::new("account", account);
    let unbuilt = imaged && !has_image(account, &file::hash(&read, named.platform())?)?;
    let (prompt, why) = if unbuilt {
        (
            sentence!(
                "Make the image of {} and the box {} in {}, billed by that cloud?",
                Name::new("environment", &read.file.name),
                &shown,
                &cloud
            ),
            prose!(
                "The image is built on a box made for it, billed while it runs and removed once the image is saved; the image is billed for its storage, and the box {} while it runs and its disk, until you remove them.",
                &shown
            ),
        )
    } else {
        (
            sentence!(
                "Make the box {} in {}, billed by that cloud?",
                &shown,
                &cloud
            ),
            prose!("A box is billed while it runs, and its disk until you remove it."),
        )
    };
    billed(out, account, prompt, why, yes)?;
    let hash = if imaged {
        Some(ensure_image(&read, account, named)?)
    } else {
        None
    };
    create(
        account,
        &machine,
        NewBox {
            named,
            spec,
            hash: hash.as_deref(),
            ..NewBox::default()
        },
    )?;
    start_on(&machine, &read, given)
}

/// Environment `read` started on box `machine`, made for it: noted as
/// that box's, its file and every file it names copied there, then its
/// stages at every start and its services run, with `given`.
pub fn start_on(machine: &str, read: &Read, given: Given) -> Result<Done, Report> {
    made_for(machine, read)?;
    // The image holds the files it was built from; what it never held
    // (services, the stages at every start) may have changed since.
    copy(read, machine)?;
    let mut session = Session::start(machine, read, Action::Start, given)?;
    let ended = session.wait(None, &|| false)?;
    let shown = Name::new("box", machine);
    if ended.code != Some(0) {
        let at = ended
            .failed
            .as_deref()
            .map(|stage| format!(" at stage {}", clean(stage)))
            .unwrap_or_default();
        return Err(Report::error(sentence!(
            "Couldn't start {} on {}.",
            Name::new("environment", &read.file.name),
            &shown
        ))
        .why(prose!("The start failed{}, and the box is up.", at))
        .next(cmd!("lnk box ssh {}", &shown), "look at what went wrong")
        .next(cmd!("lnk box delete {}", &shown), "delete it"));
    }
    Ok(Done::success(sentence!(
        "{} is up as {} says, its services ready.",
        &shown,
        Shown::new(&read.path)
    ))
    .next(cmd!("lnk box ssh {}", &shown), "go in"))
}

/// What this computer knows of the box made for an environment by `lnk
/// env create`: `~/.lnk/safe/envs/boxes/<box>.json`.
#[derive(serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Made {
    environment: String,
    /// Its file, in full: what the box was made from.
    file: std::path::PathBuf,
    /// Its cloud's ID.
    id: String,
}

link_files::specification!(Made, include_str!("remote.schema.toml"));

/// Each box an environment made, which an upgrade converts.
pub fn saved_made() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Made>()
}

fn made_path(machine: &str) -> std::path::PathBuf {
    link_files::layout::Hidden::Envs
        .path()
        .join("boxes")
        .join(format!("{machine}.json"))
}

/// Notes box `machine` as made for the environment `read`.
fn made_for(machine: &str, read: &Read) -> anyhow::Result<()> {
    link_machine::machine::check_name(machine)?;
    let id = boxes()?
        .into_iter()
        .find(|b| b.name == machine)
        .map(|b| b.id)
        .unwrap_or_default();
    let made = Made {
        environment: read.file.name.clone(),
        file: read.path.clone(),
        id,
    };
    link_files::write_private(&made_path(machine), &link_files::saved::print_json(&made)?)
        .within("noting the box made for the environment")
}

/// What this computer noted of box `machine` when `lnk env create` made
/// it, if it's that box, and not another of its name since.
fn made(machine: &str) -> anyhow::Result<Option<Made>> {
    if link_machine::machine::check_name(machine).is_err() {
        return Ok(None);
    }
    let Ok(text) = std::fs::read(made_path(machine)) else {
        return Ok(None);
    };
    let Ok(made) = link_files::saved::parse_json::<Made>(&text) else {
        return Ok(None);
    };
    let id = boxes()?
        .into_iter()
        .find(|b| b.name == machine)
        .map(|b| b.id);
    Ok(id.is_some_and(|id| id == made.id).then_some(made))
}

/// Whether box `machine` is the one `lnk env create` made for
/// environment `name`.
fn is_made_for(machine: &str, name: &str) -> anyhow::Result<bool> {
    Ok(made(machine)?.is_some_and(|made| made.environment == name))
}

/// The file of the environment box `machine` was made from, as `lnk env
/// start` noted it: the box knows its environment.
pub fn made_from(machine: &str) -> Result<std::path::PathBuf, Report> {
    match made(machine)? {
        Some(made) => Ok(made.file),
        None => Err(Report::error(sentence!(
            "Couldn't tell which environment {} was made from.",
            Name::typed("box", machine)
        ))
        .why(prose!(
            "It wasn't made from an environment by lnk env create on this computer, so which is its isn't known here."
        ))
        .next(cmd!("lnk env reset <service> --on {} --spec <file>", Name::new("box", machine)), "give the file")
        .exit(Exit::Usage)),
    }
}

/// The service `name` of `read`, which the person typed: none of that
/// name is a usage error, the file's own list one command away.
fn service_of<'a>(read: &'a Read, path: &Path, name: &str) -> Result<&'a file::Service, Report> {
    read.file
        .services()
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| {
            Report::error(sentence!(
                "Couldn't find the service {} in {}.",
                Name::typed("service", name),
                Name::new("environment", &read.file.name)
            ))
            .next(
                cmd!("lnk env check --spec {}", Shown::new(path)),
                "see the services it has",
            )
            .exit(Exit::Usage)
        })
}

/// `lnk env reset <service> --on <box>`. A root service's reset
/// runs as root, so only on a box made for its environment, which runs
/// it as root already.
pub fn reset(read: &Read, machine: &str, service: &str) -> Result<Done, Report> {
    let read = read.clone();
    let path = read.path.clone();
    let path = path.as_path();
    let root = service_of(&read, path, service)?.root();
    let shown = Name::new("box", machine);
    let service_name = Name::new("service", service);
    if root && !is_made_for(machine, &read.file.name)? {
        return Err(Report::refusal(
            link_format::Refuser::Policy("root services".into()),
            sentence!(
                "Couldn't reset {} on {}, which isn't a box made for {}.",
                &service_name,
                &shown,
                Name::new("environment", &read.file.name)
            ),
        )
        .why(prose!(
            "The service runs as root, so its reset would too, and a root service is refused on a box that's there."
        ))
        .next(
            cmd!("lnk env create --cloud <cloud> --spec {}", Shown::new(path)),
            "make a box for the environment",
        ));
    }
    copy(&read, machine)?;
    let mut session = Session::start(
        machine,
        &read,
        Action::Reset {
            service: service.into(),
        },
        Given::default(),
    )?;
    let ended = session.wait(None, &|| false)?;
    if ended.code != Some(0) {
        return Err(
            Report::error(sentence!("Couldn't reset {} on {}.", &service_name, &shown))
                .why(prose!("Its reset command failed."))
                .next(cmd!("lnk box ssh {}", &shown), "look at it"),
        );
    }
    Ok(Done::success(sentence!(
        "{} on {} is as its image had it.",
        &service_name,
        &shown
    )))
}

#[cfg(test)]
mod tests;
