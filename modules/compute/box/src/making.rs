//! `lnk box create`: a box made, the question it asks before, its host
//! keys kept and its public modules installed.

use super::*;
use link_format::{Address, Fields, Question, Refuser, Steps};
use link_workplace::Target;

/// `lnk box create`: a new box where `place` says (a cloud account or
/// here), made from the spec in `file` (if any) with its field flags,
/// `flags`, laid over it, its disk from `image` if given.
pub(super) fn make(
    out: &mut Out,
    place: &Target,
    name: Option<String>,
    file: Option<&std::path::Path>,
    flags: &link_spec::FieldFlags,
    image: Option<image::Pick>,
    yes: bool,
) -> anyhow::Result<Done> {
    let again = create_again(place);
    let target = named(place)?;
    // The slots' zone is the region of the account's cloud's table.
    let account = match target {
        "here" => None,
        target => Some(account(target)?),
    };
    let zone = match (place, &account) {
        (
            Target::Cloud {
                zone: Some(zone), ..
            },
            Some(account),
        ) => Some((account.kind.as_str(), zone.as_str())),
        _ => None,
    };
    let made = BoxSpec::made(file, flags, zone)?;
    // This build has no VM provider: a spec naming a VM is refused
    // wherever the box goes, never made in a sandbox instead.
    let isolation = made.spec.isolation();
    isolation.check_vm()?;
    // A cloud machine's own isolation is its users' sandboxes: a box's
    // sandbox, or none, is a box here's.
    let isolated = isolation.no_sandbox || isolation.sandbox.is_some();
    if target != "here" && isolated {
        return Err(said(
            Report::error(sentence!(
                "Couldn't make a box in {} with a sandbox of its own or none.",
                Name::new("account", target)
            ))
            .why(prose!(
                "A box in a cloud account is a machine of its own, where each user has a sandbox; --isolation.sandbox and --isolation.no_sandbox are for a box here."
            ))
            .next(cmd!("lnk box create here"), "make a box here")
            .exit(Exit::Usage),
        ));
    }
    // Every placement captures its inputs once, before anything is asked
    // or made: what the box is made from is never read again.
    let inputs = link_box::machine::captured(&made, flags)?;
    if target == "here" {
        if image.is_some() {
            return Err(said(
                Report::error(sentence!("Couldn't make a box here from an image."))
                    .why(prose!(
                        "A box here runs on this machine, or in a VM whose base image the host's policy chooses, so --image and --image-hash don't apply."
                    ))
                    .next(
                        again.clone(),
                        "make it without them",
                    )
                    .exit(Exit::Usage),
            ));
        }
        // A VM box only where the host's owner requires one: then the
        // host's default VM, as Agent makes one.
        if !link_box::machine::policy()?.vm_required {
            return make_native(out, place, name, made.spec, yes, inputs);
        }
        return make_here(out, place, name, made.spec, yes, inputs);
    }
    // Its environment, read back from what was captured, and the values
    // it's given, asked before anything is made.
    let environment = inputs
        .environment
        .as_ref()
        .map(link_box::machine::staged_environment)
        .transpose()?;
    let given = environment
        .as_ref()
        .map(|(_, env)| link_env_contract::values::of_yours(env))
        .transpose()?;
    // Stages saved in its image run where the image is built, once, and
    // the box starts from it.
    let imaged = environment
        .as_ref()
        .is_some_and(|(_, env)| env.file.stages().iter().any(|stage| stage.saved()));
    if imaged && image.is_some() {
        return Err(said(
            Report::error(sentence!("Couldn't make a box from an image and an environment's image."))
                .why(prose!(
                    "The environment's saved stages make the image its box starts from, so --image and --image-hash don't apply."
                ))
                .next(again.clone(), "make it without them")
                .exit(Exit::Usage),
        ));
    }
    let boxes = Boxes::load()?;
    let account = account.within("a box in a cloud names its account")?;
    let kind = account.kind.clone();
    let mut spec = made.spec;
    for outranked in spec.outranked(&kind, flags) {
        out.block(Note(Sentence::adopt(&outranked)));
    }
    // The box runs the environment's OS, which its spec may name too.
    if let Some(os) = environment.as_ref().and_then(|(_, env)| env.file.os) {
        let named = spec
            .cloud(&kind)
            .and_then(|table| table.os)
            .or(spec.machine().os);
        ensure!(
            named.is_none_or(|named| named == os),
            "the environment runs on {}, and the box spec names another OS: a box of another \
             OS is refused",
            os.word()
        );
        spec.machine.get_or_insert_default().os = Some(os);
    }
    let mut want = spec.want(&kind);
    want.check_buildable()?;
    let prefix = spec
        .name
        .as_ref()
        .map_or(target.to_string(), |n| n.to_string());
    let owner = need_owner()?;
    // An image of yours there: the box fits it (its region, its disk at
    // least the image's), or is refused before anything's asked.
    let from = match image {
        Some(image) => {
            let found = image::pick(&account, &owner, image)?;
            let raised = link_box::spec::fit(&found, &mut want)?;
            Some((found, raised))
        }
        None => None,
    };
    let chosen = name.is_some();
    if let Some(name) = &name {
        check_name(name)?;
        if boxes.get(name).is_some() {
            return Err(exists(name));
        }
    }
    ssh::check()?;
    let machine_id = link_id::candidate()?;
    let defaults = cloud::call(
        &kind,
        "defaults",
        &Request {
            account: account.duplicate(),
            place: want.region.as_ref().map(|r| r.to_string()),
            ..Request::default()
        },
    )?;
    let place = defaults.place.within("the adapter didn't say where")?;
    // Not a name a box of yours has in the cloud, where it's asked: here,
    // and where this computer knows boxes of the account's. A box found
    // later elsewhere under a name taken here is renamed then (`list`).
    let mut places: Vec<String> = boxes
        .boxes
        .iter()
        .filter(|m| m.account == target)
        .map(|m| m.place.clone())
        .chain([place.clone()])
        .collect();
    places.sort();
    places.dedup();
    // Asked while the machine is chosen: each is a cloud call or two.
    let (chosen_size, taken) = std::thread::scope(|scope| {
        let taken = (!chosen).then(|| scope.spawn(|| cloud_names(&account, &owner, places)));
        let size = choose(&account, &place, defaults.size, &want);
        let taken = taken.and_then(|t| t.join().ok()).unwrap_or_default();
        (size, taken)
    });
    let chosen_size = chosen_size?;
    let name = name.unwrap_or_else(|| boxes.next_name(&prefix, &taken));
    check_name(&name)?;
    for note in &chosen_size.notes {
        out.block(Report::warning(Sentence::adopt(note)));
    }
    let size = chosen_size.size.clone();
    out.block(Note(sentence!(
        "A machine of type {} in your {} account {} ({}) is billed to that account by {} while it runs, and its disk until you remove it.",
        size.as_str(),
        kind.as_str(),
        Name::new("account", target),
        place.as_str(),
        kind.as_str()
    )));
    out.now(question(
        &chosen_size,
        &want,
        &spec,
        from.as_ref().map(|(f, _)| f),
    ));
    if let Some((_, Some(raised))) = &from {
        out.block(Note(Sentence::adopt(raised)));
    }
    if let Some((_, env)) = &environment {
        out.block(Note(sentence!(
            "It runs the environment {}{}.",
            Name::new("environment", &env.file.name),
            if imaged {
                ", from its image, built first in this account when there's none"
            } else {
                ""
            }
        )));
    }
    if !out.asks(Question::confirm(sentence!("Make it?"), true).yes(yes))? {
        return Err(said(
            Report::refusal(Refuser::Person, sentence!("Didn't make the box."))
                .next(again.clone(), "make it when you're ready"),
        ));
    }
    // The environment's image, built now that the box is wanted: its disk
    // starts from it, as from any image of yours.
    let from = match (&environment, imaged) {
        (Some((_, env)), true) => {
            let named = link_env_contract::file::Named {
                os: Some(want.os),
                arch: Some(want.arch),
            };
            let hash = link_env_contract::remote::ensure_image(env, target, named)?;
            let found = image::pick(&account, &owner, image::Pick::Hashed(&hash))?;
            let raised = link_box::spec::fit(&found, &mut want)?;
            Some((found, raised))
        }
        _ => from,
    };
    // The install.sh the box runs, checked here by this lnk's keys before
    // the cloud is asked for anything: the latest at or above this lnk,
    // when it's a released one (a build from source is newer than any).
    let releases = setup::Releases::here()?;
    let version = releases.version(spec.lnk_version().as_ref());
    let floor = link_core::RELEASED.then(link_version::version);
    let checked = releases.install(version.as_ref(), floor.as_ref())?;
    // A copy for the box of releases it can't read where they are.
    let staged = releases.stage(&checked, want.arch)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut machine = Machine {
        price: chosen_size.price(&want),
        deadline: spec.deadline.map(|d| now + d.get().as_secs()),
        name,
        machine_id: machine_id.clone(),
        tagged: true,
        from_cloud: false,
        account: target.to_string(),
        kind: kind.clone(),
        place: place.clone(),
        id: String::new(),
        size: size.clone(),
        ip: None,
        state: "starting".into(),
        created: now,
        host_keys: Vec::new(),
        spec: Some(toml::Table::try_from(&spec)?),
        spot: want.spot,
        stopped_by_cloud: false,
        stop_guessed: false,
        // Its first boot trusts this computer's certificates alone.
        certified: true,
        environment: inputs.environment.clone(),
        here: None,
        staged: releases.follows_staged(),
    };
    // Its name, kept from here on (with no id until it's made), so another
    // `lnk box create` at the same time picks another.
    machine.name = Boxes::update(|boxes| {
        if boxes.get(&machine.name).is_some() {
            if chosen {
                return Err(exists(&machine.name));
            }
            machine.name = boxes.next_name(&prefix, &taken);
        }
        boxes.put(machine.clone());
        Ok(machine.name.clone())
    })??;
    let name = machine.name.clone();
    let shown = Name::new("box", &name);
    let gpus = chosen_size
        .info
        .as_ref()
        .map_or((0, ""), |s| (s.gpus(), s.gpu_model()));
    let mut cloud_spec = match cloud_spec(&name, &size, &machine_id, &owner, &want, gpus) {
        Ok(spec) => spec,
        Err(err) => {
            forget(&machine)?;
            return Err(err);
        }
    };
    cloud_spec.attach = spec.attach.clone();
    for tag in spec.link_tags() {
        let (key, value) = link_tag(tag)?;
        cloud_spec.tags.insert(key, value);
    }
    cloud_spec.self_remove = spec.self_remove().then_some(true);
    cloud_spec.deadline = spec.deadline;
    if let Some(deadline) = spec.deadline {
        cloud_spec.tags.insert(
            "lnk-deadline".into(),
            (machine.created + deadline.get().as_secs()).to_string(),
        );
        cloud_spec.user_data =
            setup::with_deadline(&cloud_spec.user_data, deadline, machine.created);
    }
    // Making it, reading its host key, waiting for SSH and installing lnk.
    let mut steps = Steps::new(4);
    out.step(&mut steps, phrase!("Making {}", &shown));
    let request = Request {
        account,
        place: Some(place.clone()),
        spec: Some(cloud_spec),
        image: from.as_ref().map(|(f, _)| f.id.clone()),
        ..Request::default()
    };
    let launched = match cloud::call(&kind, "launch", &request) {
        Ok(launched) => launched,
        Err(err) => {
            // What the adapter made for boxes before the launch failed
            // (a firewall rule, a key) goes again, unless a box uses it.
            if let Err(left) = cloud::call(&kind, "cleanup", &request) {
                out.block(
                    Report::warning(sentence!("Not all Link made for boxes there is gone."))
                        .why(words(&left)),
                );
            }
            // Why the cloud refused (a quota, and where to raise it) is
            // the adapter's to say, above.
            forget(&machine)?;
            return Err(said(
                failed(sentence!("Couldn't make the box {}.", &shown), err)
                    .next(cmd!("lnk cloud list"), "check the cloud account"),
            ));
        }
    };
    let Some(id) = launched.id else {
        forget(&machine)?;
        return Err(said(
            Report::error(sentence!(
                "Couldn't make the box, since the adapter didn't say which machine it made."
            ))
            .next(cmd!("lnk box list"), "see what your clouds hold"),
        ));
    };
    machine.id = id;
    // Kept from here on, so `lnk box delete` finds it whatever happens next.
    Boxes::update(|boxes| boxes.put(machine.clone()))?;
    let set_up = (|| -> anyhow::Result<()> {
        let ip = call(&machine, "wait")?
            .ip
            .within("the adapter didn't say the box's address")?;
        machine.ip = Some(ip.clone());
        keep(&machine)?;
        ssh::forget(&ip);
        ssh::close(&ip);
        out.step(
            &mut steps,
            phrase!("Reading {}'s SSH host key from its console", &shown),
        );
        match host_keys(&machine) {
            Ok(keys) => {
                ssh::pin(&ip, &keys)?;
                machine.host_keys = keys;
                keep(&machine)?;
                // Kept in the cloud: the console's first boot doesn't last,
                // and another of your computers pins them from there.
                let keys = machine.host_keys.clone();
                if let Err(err) = call_with(&machine, "save-hostkeys", |r| {
                    r.host_keys = link_cloud_contract::carried(keys)
                }) {
                    out.block(
                        Report::warning(sentence!(
                            "Its host keys aren't kept in the cloud, so another computer trusts it on first contact."
                        ))
                        .why(words(&err)),
                    );
                }
            }
            Err(err) => out.block(
                Report::warning(sentence!(
                    "Its host key is trusted on first contact instead, and pinned after."
                ))
                .why(words(&err)),
            ),
        }
        {
            let _waiting = out.wait_step(
                &mut steps,
                phrase!("Waiting for {} ({}) to take SSH", &shown, Address::new(&ip)),
            );
            ssh::wait(&ip, Duration::from_secs(300))?;
        }
        out.step(&mut steps, phrase!("Installing lnk on {}", &shown));
        out.hand_over();
        staged.send(&ip)?;
        let me = link_machine::installation::Installation {
            id: machine_id.clone(),
            name: name.clone(),
        };
        let mut script = setup::install_script(&me, &releases, &checked)?;
        if from.is_some() {
            // The image's box's ID goes: this box gets its own.
            script = format!("{}{script}", setup::forget_installation_script());
        }
        out.hand_over();
        ssh::run_script(&ip, &script)?;
        Ok(())
    })();
    if let Err(err) = set_up {
        return Err(said(
            failed(sentence!("Made {} but couldn't set it up.", &shown), err)
                .next(cmd!("lnk box ssh {}", &shown), "look at it")
                .next(cmd!("lnk box delete {}", &shown), "delete it"),
        ));
    }
    machine.state = "running".into();
    keep(&machine)?;
    let ip = machine.ip.clone().unwrap_or_default();
    if let (Some((_, env)), Some(given)) = (&environment, given) {
        // A box from no image is readied as one that's there.
        if !imaged && let Err(err) = link_env_contract::remote::ready(&name) {
            return Err(said(
                failed(sentence!("Made {} but couldn't ready it.", &shown), err)
                    .next(cmd!("lnk box delete {}", &shown), "delete it"),
            ));
        }
        return link_env_contract::remote::start_on(&name, env, given).map_err(said);
    }
    Ok(Done::success(sentence!(
        "Your box {} is up at {}, a machine of type {} in {}.",
        &shown,
        Address::new(&ip),
        machine.size.as_str(),
        machine.place.as_str()
    ))
    .next(cmd!("lnk box ssh {}", &shown), "open a shell on it")
    .next(cmd!("lnk box stop {}", &shown), "stop it, keeping its disk")
    .next(cmd!("lnk box delete {}", &shown), "delete it"))
}

/// `lnk box create` for `place`, as a Next says to try it again.
fn create_again(place: &Target) -> link_format::Command {
    match place {
        Target::Here => cmd!("lnk box create here"),
        Target::On(machine) => cmd!("lnk box create --on {}", Name::new("installation", machine)),
        Target::Cloud { cloud, .. } => {
            cmd!("lnk box create {}", Name::new("cloud", cloud))
        }
    }
}

/// The name `place` stands for: `here` or a cloud account; anything else is refused with the slot it belongs in.
fn named(place: &Target) -> anyhow::Result<&str> {
    match place {
        Target::Here => Ok("here"),
        Target::Cloud { cloud, .. } => Ok(cloud),
        Target::On(name) if Boxes::load()?.get(name).is_some() => Err(said(
            Report::error(sentence!(
                "Couldn't make a box in {}, which is a box already.",
                Name::new("box", name)
            ))
            .why(prose!(
                "Create a new box in a cloud account or here, rather than inside an existing box."
            ))
            .next(
                cmd!("lnk box start {}", Name::new("box", name)),
                "start the existing box",
            )
            .exit(Exit::Usage),
        )),
        Target::On(name) => Err(said(
            Report::error(sentence!(
                "Couldn't create a box on {}.",
                Name::new("installation", name)
            ))
            .why(prose!("Create a box here or choose a cloud account."))
            .next(cmd!("lnk box create here"), "make a box here")
            .next(cmd!("lnk cloud list"), "list your cloud accounts")
            .exit(Exit::Usage),
        )),
    }
}

/// A box made here with no VM: Box's own placement on this machine, a
/// home folder and a record, its commands run in its sandbox or
/// directly, from the inputs captured before anything was asked.
fn make_native(
    out: &mut Out,
    place: &Target,
    name: Option<String>,
    spec: BoxSpec,
    yes: bool,
    inputs: link_box_contract::MachineInputs,
) -> anyhow::Result<Done> {
    let again = create_again(place);
    if *spec.machine() != link_box::spec::Machine::default()
        || spec.attach() != Default::default()
        || spec.lnk().version.is_some()
        || spec.lnk().login == Some(true)
        || spec.self_remove()
        || !spec.link_tags().is_empty()
        || spec.deadline.is_some()
    {
        return Err(said(
            Report::error(sentence!("Couldn't make a box here with those options."))
                .why(prose!(
                    "A box here is this machine: a cloud machine size, attachments, a sign-in, self-removal, a deadline and a Link version don't apply."
                ))
                .next(cmd!("lnk box create here"), "make it without them")
                .exit(Exit::Usage),
        ));
    }
    // Its path is absolute already: a file's from its folder, a flag's
    // from the working folder.
    let isolation = spec.isolation();
    let file = isolation.sandbox_file().map(std::path::Path::to_path_buf);
    let mode = isolation.mode();
    let mut taken = link_box::machine::bindings()?
        .into_iter()
        .map(|binding| binding.name)
        .collect::<Vec<_>>();
    taken.extend(
        link_box::machine::pending()?
            .into_iter()
            .map(|pending| pending.input.name),
    );
    let prefix = spec
        .name
        .as_ref()
        .map_or("here", |name| name.as_str())
        .to_string();
    let name = match name {
        Some(name) => name,
        None => Boxes::load()?.next_name(&prefix, &taken),
    };
    check_name(&name)?;
    if Boxes::load()?.get(&name).is_some() || taken.contains(&name) {
        return Err(exists(&name));
    }
    let shown = Name::new("box", &name);
    let home = link_box::native::home_of(&name)?;
    // Its sandbox, read and checked before anything is asked.
    let confinement = link_box::native::confinement(mode, file.as_deref(), &home)?;
    let how = match &confinement {
        link_box::state::Confinement::Sandbox { .. } => "in its sandbox",
        link_box::state::Confinement::Direct => "directly, with your own access",
    };
    out.block(Note(sentence!(
        "A box here is a folder on this machine, {}, and what runs in it runs {}.",
        link_format::Path::new(&home),
        how
    )));
    if let Some(env) = &inputs.environment {
        out.block(Note(sentence!(
            "It runs the environment in {}, set up here.",
            link_format::Path::new(&env.provenance.reference)
        )));
    }
    if !out.asks(Question::confirm(sentence!("Make the box {} here?", &shown), true).yes(yes))? {
        return Err(said(
            Report::refusal(Refuser::Person, sentence!("Didn't make {}.", &shown))
                .next(again.clone(), "make it when you're ready"),
        ));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let machine_id = link_id::candidate()?;
    let machine = Machine {
        price: None,
        deadline: None,
        name: name.clone(),
        machine_id: machine_id.clone(),
        tagged: true,
        from_cloud: false,
        account: "here".into(),
        kind: "here".into(),
        place: "here".into(),
        id: machine_id,
        size: String::new(),
        ip: None,
        state: "running".into(),
        created: now,
        host_keys: Vec::new(),
        spec: Some(toml::Table::try_from(&spec)?),
        spot: false,
        stopped_by_cloud: false,
        stop_guessed: false,
        certified: false,
        environment: inputs.environment.clone(),
        here: Some(link_box::state::Here {
            home: home.clone(),
            confinement,
        }),
        staged: false,
    };
    // Kept before its home is made, so another create picks another name
    // and `lnk box delete` finds whatever is made next.
    Boxes::update(|boxes| {
        if boxes.get(&name).is_some() {
            return Err(exists(&name));
        }
        boxes.put(machine.clone());
        Ok(())
    })??;
    if let Err(err) = link_box::native::make_home(&name) {
        Boxes::update(|boxes| boxes.remove(&name))?;
        return Err(err);
    }
    if let Err(err) = link_box::native::set_up(&machine) {
        return Err(said(
            failed(
                sentence!("Made {} but couldn't set up its environment.", &shown),
                err,
            )
            .next(cmd!("lnk box ssh {}", &shown), "look at it")
            .next(cmd!("lnk box delete {}", &shown), "delete it"),
        ));
    }
    Ok(
        Done::success(sentence!("The box {} is ready here.", &shown))
            .next(cmd!("lnk box ssh {}", &shown), "open a shell in it")
            .next(cmd!("lnk box stop {}", &shown), "stop its services")
            .next(cmd!("lnk box delete {}", &shown), "delete it"),
    )
}

/// A box made here: a VM, from the inputs captured
/// before anything was asked.
fn make_here(
    out: &mut Out,
    place: &Target,
    name: Option<String>,
    spec: BoxSpec,
    yes: bool,
    inputs: link_box_contract::MachineInputs,
) -> anyhow::Result<Done> {
    let again = create_again(place);
    if *spec.machine() != link_box::spec::Machine::default()
        || spec.attach() != Default::default()
        || spec.lnk().version.is_some()
        || spec.lnk().login == Some(true)
        || spec.self_remove()
        || !spec.link_tags().is_empty()
        || spec.isolation().no_sandbox
        || spec.isolation().sandbox.is_some()
    {
        return Err(said(
            Report::error(sentence!("Couldn't make a VM box with those options."))
                .why(prose!(
                    "This machine's owner requires a VM for a box here, which takes its resources and base images from the host's VM policy, and its guest sandboxes what runs in it; a cloud machine size, attachments, a sign-in on the guest, self-removal, a Link version, isolation.sandbox and isolation.no_sandbox don't apply."
                ))
                .next(again.clone(), "make it without them")
                .exit(Exit::Usage),
        ));
    }
    let mut taken = link_box::machine::bindings()?
        .into_iter()
        .map(|binding| binding.name)
        .collect::<Vec<_>>();
    taken.extend(
        link_box::machine::pending()?
            .into_iter()
            .map(|pending| pending.input.name),
    );
    let name = name.unwrap_or_else(|| {
        Boxes::load().unwrap_or_default().next_name(
            spec.name.as_ref().map_or("here", |name| name.as_str()),
            &taken,
        )
    });
    let shown = Name::new("box", &name);
    if !out.asks(Question::confirm(sentence!("Make the VM box {}?", &shown), false).yes(yes))? {
        return Err(said(
            Report::refusal(Refuser::Person, sentence!("Didn't make {}.", &shown))
                .next(again.clone(), "make it when you're ready"),
        ));
    }
    let (lifetime, lease) = match spec.deadline {
        Some(deadline) => {
            let expires_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .within("reading the clock")?
                .as_millis() as u64
                + deadline.get().as_secs() * 1000;
            (
                link_box_contract::Lifetime::Created,
                link_box_contract::Lease::Finite {
                    expires_at,
                    renewable_until: expires_at,
                },
            )
        }
        None => (
            link_box_contract::Lifetime::Retained,
            link_box_contract::Lease::Retained {
                principal_expires_at: None,
            },
        ),
    };
    let making = out.activity(phrase!("Making {}", &shown));
    let binding = link_box::machine::create(&name, inputs, lifetime, lease)?;
    // An environment needs Link; an SSH-only request does not.
    let selector = link_box_contract::BoxIdentity {
        name: binding.name.clone(),
        machine_id: binding.machine_id.clone(),
    }
    .selector();
    if let Some(environment) = &binding.inputs.environment {
        let prepared = link_box::machine::bootstrap(&selector)
            .and_then(|_| link_box::machine::prepare(&selector, environment, true, true, false));
        if let Err(error) = prepared {
            let cleanup = link_box::machine::remove(&selector);
            drop(making);
            let report = failed(
                sentence!("Couldn't prepare the guest of {}.", &shown),
                error,
            );
            return Err(said(match cleanup {
                Ok(()) => {
                    out.block(Note(sentence!("Its guest was removed.")));
                    report.next(again.clone(), "make it again")
                }
                Err(cleanup) => {
                    out.block(
                        Report::warning(sentence!(
                            "Couldn't remove {}'s guest; its cleanup is still pending.",
                            &shown
                        ))
                        .why(words(&cleanup)),
                    );
                    report.next(cmd!("lnk box delete {}", &shown), "remove it")
                }
            }));
        }
    }
    drop(making);
    Ok(Done::success(sentence!("The VM box {} is ready.", &shown))
        .next(cmd!("lnk box ssh {}", &shown), "open a shell on it")
        .next(cmd!("lnk box stop {}", &shown), "stop it, keeping its disk")
        .next(cmd!("lnk box delete {}", &shown), "delete it"))
}

/// The report for a box named `name` that exists already.
fn exists(name: &str) -> anyhow::Error {
    said(
        Report::error(sentence!(
            "Couldn't make a box named {}, since it exists already.",
            Name::new("box", name)
        ))
        .next(
            cmd!("lnk box start {}", Name::new("box", name)),
            "start that box",
        )
        .next(cmd!("lnk box list"), "list your boxes")
        .exit(Exit::Usage),
    )
}

/// What the question before making a box says of it: its machine type
/// (`g6.xlarge: 4 CPUs, 16 GB, 1 nvidia-l4, x86_64, spot`), its disk, its
/// `lnk` (an image's, from one), and the price when the cloud gives one.
fn question(
    chosen: &Chosen,
    want: &Want,
    spec: &BoxSpec,
    image: Option<&cloud::FoundImage>,
) -> Fields {
    let mut machine = match &chosen.info {
        Some(info) => link_cloud_contract::want::describe(info),
        None => clean(&chosen.size),
    };
    if want.spot {
        machine.push_str(", spot");
    }
    let mut fields = Fields::new()
        .row("Machine", machine)
        .row(
            "Disk",
            format!("{} GB {}", want.disk.whole_gb(), word(&want.disk_type)),
        )
        .row(
            "Lnk",
            match image {
                Some(image) => format!("as on image {}", clean(&image.name)),
                None => spec
                    .lnk_version()
                    .as_ref()
                    .map_or("latest".to_string(), |v| v.to_string()),
            },
        );
    if let Some(image) = image {
        fields = fields.row(
            "Image",
            format!("{} ({})", clean(&image.name), clean(&image.id)),
        );
    }
    if let Some((dollars, partial)) = chosen.price(want).and_then(|p| p.hourly()) {
        let more = if partial { " and more" } else { "" };
        fields = fields.row("Price", format!("{}{more}", price(dollars)));
    }
    fields
}

/// The box's SSH host keys, from its console (`hostkeys`), once its first
/// boot has printed them: asked every few seconds for up to 5 minutes, as
/// long as the adapter answers.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a box's console for its host key"
)]
fn host_keys(machine: &Machine) -> anyhow::Result<Vec<String>> {
    let timeout = Duration::from_secs(300);
    let start = std::time::Instant::now();
    loop {
        let keys = call(machine, "hostkeys")
            .within("its console couldn't be read")?
            .host_keys
            .unwrap_or_default();
        if !keys.is_empty() {
            return Ok(keys);
        }
        ensure!(
            start.elapsed() < timeout,
            "its console showed no host key within {} s",
            timeout.as_secs()
        );
        std::thread::sleep(Duration::from_secs(5));
    }
}

/// The names of `owner`'s boxes in `account`, in `places`, as its cloud
/// says; none when it can't say.
fn cloud_names(account: &Account, owner: &str, places: Vec<String>) -> Vec<String> {
    let request = Request {
        account: account.duplicate(),
        owner: Some(owner.to_string()),
        places: link_cloud_contract::carried(places),
        ..Request::default()
    };
    cloud::call(&account.kind, "list", &request)
        .map(|reply| {
            reply
                .boxes
                .unwrap_or_default()
                .into_iter()
                .map(|b| b.name)
                .collect()
        })
        .unwrap_or_default()
}

/// A price per hour, as the question says it.
pub(super) fn price(dollars: f64) -> String {
    format!("${dollars:.2}/hour")
}
