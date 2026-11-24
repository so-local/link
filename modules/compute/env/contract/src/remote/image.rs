//! An environment's image: built on a box made for it, which its cloud
//! removes at the image's lifetime if this program never does (or here,
//! on the box itself), found or built in an account, and the setup run
//! where it's asked.

use std::path::Path;
use std::time::Duration;

use anyhow::{bail, ensure};
use link_format::{
    Done, Exit, Name, Note, Out, Path as Shown, Report, WithinExt, aside, cmd, prose, sentence,
};

use super::{
    NewBox, Session, billed, copy, copy_here, create, has_image, is_box, lnk, named, ready,
};
use crate::file::{self, Named, Read};
use crate::here::Action;
use crate::values::Given;

/// Builds `read`'s image in `account` now, for the OS and processor
/// `on`: a box made for it, the stages saved in the image run there,
/// given the sealed secrets alone, within `timeout` if one is given,
/// saved with `hash`, and the box removed whatever happened: here, or by
/// its cloud at [`image_lifetime`] if this program never gets to it.
pub fn build_image(
    read: &Read,
    account: &str,
    hash: &str,
    on: Named,
    timeout: Option<Duration>,
) -> anyhow::Result<()> {
    let machine = image_box(read, hash);
    let _building = building(&machine)?;
    build(read, account, hash, on, timeout, &machine)
}

/// The box `read`'s image with `hash` is built on, by name: the same
/// for every build of it.
fn image_box(read: &Read, hash: &str) -> String {
    named(&read.file.name, &format!("image-{}", &hash[..6]))
}

/// Holds the build of an image on box `machine` from this computer
/// until dropped: builds started at once (a run of jobs on one
/// environment) each wait for the one before, and so never ask for the
/// same box twice.
fn building(machine: &str) -> anyhow::Result<std::fs::File> {
    link_machine::machine::check_name(machine)?;
    if is_box(machine)? {
        aside(Note(sentence!(
            "Another build of this image is under way, on {}, so this waits for it.",
            Name::new("box", machine)
        )));
    }
    link_files::lock_file(
        &link_files::layout::Hidden::Envs
            .path()
            .join("building")
            .join(format!("{machine}.lock")),
    )
}

/// [`build_image`], once its build is held.
fn build(
    read: &Read,
    account: &str,
    hash: &str,
    on: Named,
    timeout: Option<Duration>,
    machine: &str,
) -> anyhow::Result<()> {
    aside(Note(sentence!(
        "Building the image of {} ({}) in {}, on a box made for it: {}.",
        Name::new("environment", &read.file.name),
        hash,
        Name::new("account", account),
        Name::new("box", machine)
    )));
    create(
        account,
        machine,
        NewBox {
            deadline: Some(image_lifetime(timeout)),
            named: on,
            ..NewBox::default()
        },
    )?;
    let built = (|| {
        ready(machine)?;
        copy(read, machine)?;
        let given = build_given(read)?;
        let mut session = Session::start(machine, read, Action::Image, given)?;
        built(
            &mut session,
            &format!("building {}'s image", read.file.name),
            timeout,
            machine,
        )?;
        let image = format!("{}-{}", read.file.name, &hash[..8]);
        lnk(&[
            "box", "image", "create", machine, &image, "--hash", hash, "--yes",
        ])
    })();
    if let Err(err) = lnk(&["box", "delete", machine, "--yes"]) {
        aside(
            Report::warning(sentence!(
                "Couldn't delete the box {}, which the image was built on.",
                Name::new("box", machine)
            ))
            .why(link_format::Sentence::adopt(&format!("{err:#}")))
            .next(
                cmd!("lnk box delete {}", Name::new("box", machine)),
                "delete it",
            ),
        );
    }
    built
}

/// How long an image's box lives: its build's `timeout` and a quarter
/// hour to set it up and save it, or four hours without one.
pub fn image_lifetime(timeout: Option<Duration>) -> u64 {
    let build = timeout.map_or(4 * 3600 - 900, |t| t.as_secs());
    build.saturating_add(900).min(7 * 86400)
}

/// `lnk env image create <cloud> --spec <file>`, within `timeout` if one
/// is given: billed by the cloud, so built only once you say yes (`yes`,
/// `--yes`).
pub fn image(
    out: &mut Out,
    read: &Read,
    account: &str,
    timeout: Option<Duration>,
    yes: bool,
) -> Result<Done, Report> {
    let read = read.clone();
    let hash = file::hash(&read, read.file.platform())?;
    billed(
        out,
        account,
        sentence!(
            "Make the image of {} in {}, billed by that cloud?",
            Name::new("environment", &read.file.name),
            Name::new("account", account)
        ),
        prose!(
            "It's built on a box made for it, billed while it runs and removed once the image is saved; the image is billed for its storage until you remove it."
        ),
        yes,
    )?;
    build_image(&read, account, &hash, read.file.named(), timeout)?;
    Ok(Done::success(sentence!(
        "The image of {} is saved in {}, with hash {}.",
        Name::new("environment", &read.file.name),
        Name::new("account", account),
        hash
    ))
    .next(
        cmd!(
            "lnk env create --cloud {} --spec {}",
            Name::new("account", account),
            Shown::new(&read.path)
        ),
        "make a box from it",
    ))
}

/// The image for `read` in `account`, for the OS and processor `named`,
/// built first when none has its hash; its hash. Asked once any build of
/// it started here at the same time ends: that build's image is this one.
pub fn ensure_image(read: &Read, account: &str, named: Named) -> anyhow::Result<String> {
    let hash = file::hash(read, named.platform())?;
    let machine = image_box(read, &hash);
    let _building = building(&machine)?;
    if has_image(account, &hash)? {
        aside(Note(sentence!(
            "{} has the image of {} ({}), so no stage but those at every start runs.",
            Name::new("account", account),
            Name::new("environment", &read.file.name),
            hash.as_str()
        )));
    } else {
        build(read, account, &hash, named, None, &machine)?;
    }
    Ok(hash)
}

/// What a build of `read`'s image is given: its sealed secrets only,
/// which the proxy holds (a private clone), so nothing it saves holds
/// one. None when a stage runs as root: beside the proxy, on the machine
/// the others run on, what it leaves running could read one.
pub(super) fn build_given(read: &Read) -> anyhow::Result<Given> {
    match root_stage(&read.file) {
        None => crate::values::sealed(read),
        Some(stage) => {
            if !read.file.sealed_names().is_empty() {
                aside(Note(sentence!(
                    "No sealed secret is given to the build, since stage {} runs as root, which could read it.",
                    Name::new("stage", stage)
                )));
            }
            Ok(Given::default())
        }
    }
}

/// Waits for `session`, running `read`'s build stages on `machine`, to
/// end, within `timeout` if one is given: an error unless every stage
/// passed. `doing` is what it does, for the timeout's message ("building
/// app's image").
fn built(
    session: &mut Session,
    doing: &str,
    timeout: Option<Duration>,
    machine: &str,
) -> anyhow::Result<()> {
    let ended = session.wait(timeout, &|| false)?;
    // Stopped here, at the build's timeout, rather than ended there.
    if let (true, None, Some(timeout)) = (ended.timed_out, ended.code, timeout) {
        bail!(
            "{doing} reached its timeout, {}",
            crate::file::Timeout::from_std(timeout)
        );
    }
    ensure!(
        ended.code == Some(0),
        "setting {machine} up failed{}",
        ended
            .failed
            .as_deref()
            .map(|s| format!(" at {s}"))
            .unwrap_or_default()
    );
    Ok(())
}

/// `lnk env image create here --spec <file>`, on a box alone (`/etc/lnk/
/// kind.toml`): the stages saved in its image run here, as on a box
/// made for it, within `timeout` if one is given, for the box's OS and
/// processor. The box is saved as the image from the computer that made
/// it, which holds its cloud account (`lnk box image create`), and says
/// how. On your computer it's refused before anything runs: a computer
/// is never an image.
pub fn image_here(
    path: &Path,
    flags: &link_spec::FieldFlags,
    timeout: Option<Duration>,
) -> Result<Done, Report> {
    let link_machine::machine::Kind::Box { os, arch } = link_machine::machine::kind()? else {
        return Err(Report::error(sentence!(
            "Couldn't make an image of this machine, which is a computer."
        ))
        .why(prose!("A computer is never an image."))
        .next(
            cmd!("lnk env image create <cloud> --spec {}", Shown::new(path)),
            "build it on a box made for it",
        )
        .next(cmd!("lnk cloud list"), "see your cloud accounts")
        .exit(Exit::Usage));
    };
    let read = file::read_with(path, flags)?;
    if let Some(named) = read.file.os.filter(|named| *named != os) {
        return Err(Report::error(sentence!(
            "Couldn't build the image of {} here.",
            Name::new("environment", &read.file.name)
        ))
        .why(prose!(
            "It is for {}, and this box is {}: its image is built on a box of that OS.",
            named.word(),
            os.word()
        ))
        .exit(Exit::Usage));
    }
    let hash = file::hash(&read, file::Platform { os, arch })?;
    let me = link_machine::installation::installation()?;
    aside(Note(sentence!(
        "Building the image of {} ({}) here, on box {}.",
        Name::new("environment", &read.file.name),
        hash.as_str(),
        Name::new("box", &me.name)
    )));
    copy_here(&read)?;
    let file = read.path.file_name().within("the file's name")?;
    let mut session = Session::with_log(
        None,
        &read.file.name,
        Some(Path::new(file)),
        Action::Image,
        build_given(&read)?,
        None,
    )?;
    built(
        &mut session,
        &format!("building {}'s image", read.file.name),
        timeout,
        &me.name,
    )?;
    Ok(Done::success(sentence!(
        "{} is set up as the image of {}, with hash {}.",
        Name::new("box", &me.name),
        Name::new("environment", &read.file.name),
        hash.as_str()
    ))
    .next(
        cmd!(
            "lnk box image create {} {} --hash {}",
            Name::new("box", &me.name),
            format!("{}-{}", read.file.name, &hash[..8]),
            hash.as_str()
        ),
        "save it, from the computer that made it",
    ))
}

/// `lnk env setup --spec <file> --on <box | here>`: the stages an image
/// would save, run on the box or this machine, within `timeout` if one is
/// given, and no image saved. On a computer a `root` stage is refused
/// before anything runs (`lnk env here`'s `needs_box`); the rest run in
/// their sandboxes, writing only what those let them.
pub fn setup(read: &Read, on: Option<&str>, timeout: Option<Duration>) -> Result<Done, Report> {
    let read = read.clone();
    let machine = match on {
        Some(machine) => {
            if !is_box(machine)? {
                return Err(Report::error(sentence!(
                    "Couldn't set up {}, which isn't one of your boxes.",
                    Name::typed("box", machine)
                ))
                .why(prose!("Setup sets up a box or here."))
                .next(cmd!("lnk box list"), "see your boxes")
                .exit(Exit::Usage));
            }
            ready(machine)?;
            copy(&read, machine)?;
            machine.to_string()
        }
        None => {
            copy_here(&read)?;
            "this machine".to_string()
        }
    };
    aside(Note(sentence!(
        "Setting {} up as {} says, saving no image.",
        machine.as_str(),
        Name::new("environment", &read.file.name)
    )));
    let file = read.path.file_name().within("the file's name")?;
    let mut session = Session::with_log(
        on,
        &read.file.name,
        Some(Path::new(file)),
        Action::Setup,
        build_given(&read)?,
        None,
    )?;
    built(
        &mut session,
        &format!("setting {machine} up"),
        timeout,
        &machine,
    )?;
    Ok(Done::success(sentence!(
        "{} is set up as {} says.",
        machine.as_str(),
        Name::new("environment", &read.file.name)
    )))
}

/// The first of `file`'s stages saved in its image that runs as root, by
/// name: a build of it is given no sealed secret.
pub fn root_stage(file: &crate::file::Env) -> Option<&str> {
    file.stages()
        .iter()
        .flat_map(|s| s.members())
        .find(|s| s.saved() && s.root())
        .map(|s| s.name.as_str())
}
