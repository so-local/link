//! A box reached: files copied to it, its address found, restarted, and
//! kept in or forgotten from the boxes this machine knows.

use super::*;
use link_format::{Count, Path};

/// The first folder on the way to `file` that is a link: below `home`
/// any, elsewhere one root doesn't own (the system's own, `/var` on a
/// Mac, are left alone).
fn linked_folder(file: &std::path::Path, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let file = std::path::absolute(file).ok()?;
    let parent = file.parent()?;
    let (mut path, below, in_home) = match parent.strip_prefix(home) {
        Ok(below) => (home.to_path_buf(), below, true),
        Err(_) => (std::path::PathBuf::from("/"), parent, false),
    };
    for part in below.components() {
        path.push(part);
        let link = std::fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.file_type().is_symlink());
        #[cfg(unix)]
        let link = link.filter(|m| in_home || std::os::unix::fs::MetadataExt::uid(m) != 0);
        #[cfg(not(unix))]
        let _ = in_home;
        if link.is_some() {
            return Some(path);
        }
    }
    None
}

/// `lnk box copy <name> <files> <dest>`: each file to folder `dest` on
/// the box, over its SSH.
pub(super) fn copy(
    out: &mut Out,
    name: &str,
    files: &[std::path::PathBuf],
    dest: &str,
) -> anyhow::Result<Done> {
    let refuse =
        |headline: Sentence, why: Prose| said(Report::error(headline).why(why).exit(Exit::Usage));
    let mut names = std::collections::BTreeSet::new();
    for file in files {
        let shown = Path::new(file);
        let named = file.file_name().map(|n| n.to_os_string());
        if !named.is_some_and(|n| names.insert(n)) {
            return Err(refuse(
                sentence!("Couldn't copy {}.", shown),
                prose!("It is named like another file given, and each keeps its name there."),
            ));
        }
        // Never a link's target: one planted where you copy from would
        // send what it points to (an SSH key) to the box. That holds for
        // a folder on the way to it too, anywhere in your home.
        if let Some(link) = linked_folder(file, &link_files::home_dir()) {
            return Err(refuse(
                sentence!("Couldn't copy {}.", shown),
                prose!(
                    "The folder {} is a link on the way to it, and lnk box copy never follows one, so copy the file by its own path.",
                    Path::new(link)
                ),
            ));
        }
        let meta = std::fs::symlink_metadata(file).ok();
        if meta.as_ref().is_some_and(|m| m.file_type().is_symlink()) {
            return Err(refuse(
                sentence!("Couldn't copy {}.", shown),
                prose!(
                    "It is a link, and lnk box copy copies files, never a link's target, so copy the file it points to by its own path."
                ),
            ));
        }
        if !meta.is_some_and(|m| m.is_file()) {
            return Err(refuse(
                sentence!("Couldn't copy {}.", shown),
                prose!(
                    "It isn't a file, and lnk box copy copies files, so tar a folder first and copy that."
                ),
            ));
        }
    }
    if let Some(machine) = link_box::native::find(name)? {
        link_box::native::copy(&machine, files, dest)?;
        return Ok(Done::success(sentence!(
            "Copied {} to {} in {}.",
            Count::new(files.len() as u64, "file", "files"),
            Path::new(dest),
            Name::new("box", name)
        ))
        .next(
            cmd!("lnk box ssh {}", Name::new("box", name)),
            "open a shell in it",
        ));
    }
    ssh::remote_folder(dest)?;
    let ip = reachable(out, Some(name))?;
    out.block(Note(sentence!(
        "Copying {} to {} on {}.",
        Count::new(files.len() as u64, "file", "files"),
        Path::new(dest),
        Name::new("box", name)
    )));
    out.hand_over();
    ssh::copy(&ip, files, dest)?;
    Ok(Done::success(sentence!(
        "Copied {} to {} on {}.",
        Count::new(files.len() as u64, "file", "files"),
        Path::new(dest),
        Name::new("box", name)
    ))
    .next(
        cmd!("lnk box ssh {}", Name::new("box", name)),
        "open a shell on it",
    ))
}

/// Box `name`'s address (the only box's without one), once this computer
/// gets in ([`reach`]).
pub(super) fn reachable(out: &mut Out, name: Option<&str>) -> anyhow::Result<String> {
    let local = match name {
        Some(name) if link_box::machine::is_here(name) => Some(name.to_string()),
        None if Boxes::load()?.boxes.is_empty() => {
            let local = link_box::machine::bindings()?;
            match local.as_slice() {
                [binding] => Some(binding.name.clone()),
                _ => None,
            }
        }
        _ => None,
    };
    if let Some(name) = local {
        let binding = link_box::machine::inspect(&name)?;
        if binding.state != link_box_contract::BoxState::Ready {
            let shown = Name::new("box", &name);
            return Err(said(
                Report::error(sentence!("Couldn't reach {}, which isn't ready.", &shown))
                    .why(prose!(
                        "It is {}.",
                        format!("{:?}", binding.state).to_lowercase()
                    ))
                    .next(cmd!("lnk box start {}", &shown), "start it"),
            ));
        }
        return Ok(format!("{}{name}", link_box::machine::PREFIX));
    }
    let mut machine = Boxes::load()?.pick(name)?.clone();
    let ip = address(&machine)?;
    if machine.from_cloud || !machine.certified {
        reach(out, &mut machine, &ip)?;
        keep(&machine)?;
    }
    Ok(ip)
}

/// Starts box `name` again; its address, once it takes SSH.
pub(super) fn restart(out: &mut Out, name: &str) -> anyhow::Result<String> {
    if link_box::machine::is_here(name) {
        let binding = link_box::machine::binding(link_box::machine::call(
            link_box_contract::HereOperation::Start { name: name.into() },
        )?)?;
        let selector = link_box_contract::BoxIdentity {
            name: binding.name.clone(),
            machine_id: binding.machine_id.clone(),
        }
        .selector();
        if let Some(environment) = &binding.inputs.environment {
            link_box::machine::bootstrap(&selector)?;
            link_box::machine::prepare(&selector, environment, false, true, false)?;
        }
        return Ok(format!("{}{name}", link_box::machine::PREFIX));
    }
    let mut machine = Boxes::load()?.pick(Some(name))?.clone();
    let shown = Name::new("box", name);
    if machine.id.is_empty() {
        return Err(said(
            Report::error(sentence!(
                "Couldn't start {}, which isn't made yet.",
                &shown
            ))
            .why(prose!(
                "Another lnk box create is making it, or stopped before it could."
            ))
            .next(
                cmd!("lnk box delete {}", &shown),
                "forget it if nothing is making it",
            )
            .exit(Exit::Usage),
        ));
    }
    let starting = out.activity(phrase!("Starting {}", &shown));
    let ip = call(&machine, "start")?
        .ip
        .within("the adapter didn't say the box's address")?;
    machine.ip = Some(ip.clone());
    machine.state = "running".into();
    machine.stopped_by_cloud = false;
    machine.stop_guessed = false;
    // Its address may be new; its host keys aren't. Kept first, so the
    // box at that address is this one, whose name they're pinned under.
    keep(&machine)?;
    if !machine.host_keys.is_empty() {
        ssh::pin(&ip, &machine.host_keys)?;
    }
    if machine.from_cloud {
        reach(out, &mut machine, &ip)?;
        keep(&machine)?;
    }
    ssh::wait(&ip, Duration::from_secs(300))?;
    drop(starting);
    // Up, with this computer's key: a box made before certificates is
    // given them now.
    if !machine.certified {
        reach(out, &mut machine, &ip)?;
        keep(&machine)?;
    }
    // Its environment's services and stages at every start run again,
    // from the files captured when it was made.
    if let Some(environment) = &machine.environment {
        let (_staged, env) = link_box::machine::staged_environment(environment)?;
        let given = link_env_contract::values::of_yours(&env)?;
        link_env_contract::remote::start_on(name, &env, given).map_err(said)?;
    }
    Ok(ip)
}

/// Saves `machine` as it is now, unless another command removed it (or
/// put another box in its name) meanwhile.
pub(super) fn keep(machine: &Machine) -> anyhow::Result<()> {
    Boxes::update(|boxes| {
        if boxes
            .get(&machine.name)
            .is_some_and(|saved| saved.id == machine.id)
        {
            boxes.put(machine.clone());
        }
    })
}

/// Forgets a box `lnk box create` kept a name for but didn't make.
pub(super) fn forget(machine: &Machine) -> anyhow::Result<()> {
    Boxes::update(|boxes| {
        if boxes
            .get(&machine.name)
            .is_some_and(|saved| saved.id.is_empty() && saved.machine_id == machine.machine_id)
        {
            boxes.remove(&machine.name);
        }
    })
}

/// A running box's address.
fn address(machine: &Machine) -> anyhow::Result<String> {
    let shown = Name::new("box", &machine.name);
    let stopped = |why: Prose| {
        said(
            Report::error(sentence!("Couldn't reach {}, which is stopped.", &shown))
                .why(why)
                .next(cmd!("lnk box start {}", &shown), "start it")
                .exit(Exit::Usage),
        )
    };
    match (&machine.ip, machine.state.as_str()) {
        (_, "stopped") if machine.stopped_by_cloud => Err(stopped(prose!(
            "Its cloud took the spot machine back; starting it says when its cloud starts it again itself."
        ))),
        (_, "stopped") => Err(stopped(prose!("It was stopped, not removed."))),
        (Some(ip), _) => Ok(ip.clone()),
        (None, _) => Err(said(
            Report::error(sentence!(
                "Couldn't reach {}, which has no address yet.",
                &shown
            ))
            .next(cmd!("lnk box list --refresh"), "ask its cloud for it"),
        )),
    }
}
