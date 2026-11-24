//! `lnk-box`: the box module's commands. `lnk` runs it for `lnk box ...`.

mod image;
mod inspecting;
mod listing;
mod making;
mod reaching;
mod removing;
mod showing;
mod slots;
mod tagging;
mod users;
use listing::{accounts, list};
use making::{make, price};
use reaching::{copy, forget, keep, reachable, restart};
use removing::delete;
use showing::{list_sizes, print_plan, print_spec};
use tagging::{check_attachments_on, delete_id, link_tag, list_tagged};

use std::time::Duration;

use anyhow::ensure;

use link_box::cli::{self, Command};
use link_box::said::{Asks, failed, said, usage, words};
use link_box::spec::BoxSpec;
use link_box::state::{Boxes, Machine, check_name};
use link_box::{
    Chosen, Offered, account, call, call_with, choose, choose_from, cloud_spec, need_owner,
    offered, owner, reach, setup, ssh,
};
use link_cloud_contract::want::Want;
use link_cloud_contract::want::word;
use link_cloud_contract::{self as cloud, Account, Reply, Request};
use link_format::clean;
use link_format::{
    Address, Done, Exit, Name, Note, Out, Prose, Report, Sentence, Value, WithinExt, cmd, phrase,
    prose, sentence,
};
use link_secretive::Duplicate;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!(link_box::spec::BoxSpec);
    link_settings::output::run(|out| dispatch(out).map_err(link_box::said::report))
}

fn dispatch(out: &mut Out) -> anyhow::Result<Done> {
    let parsed = cli::parse();
    let fields = parsed.fields;
    let command = match parsed.command.command {
        Command::BoxResourceJournalSchema(_) => {
            out.data(&cloud::ownership::schema_bytes());
            return Ok(Done::shown());
        }
        Command::Uninstall(given) => {
            let args = &given;
            use link_core::uninstall as u;
            let kept = u::Kept {
            parts: vec![
            u::Part {
                name: "clouds".into(),
                about: "your cloud accounts' keys, and Link's SSH key and certificate authority for your boxes (the boxes keep running, and billing)".into(),
                paths: vec![u::settings(link_files::layout::Hidden::Boxes)?],
                keys: vec!["box/".into()],
                credentials: u::settings(link_files::layout::Hidden::Boxes)
                    .map(|dir| dir.join("id_ed25519"))
                    .ok()
                    .filter(|key| key.exists())
                    .map(|key| u::Held {
                        what: link_core::uninstall::CredentialKind::BoxSshKey.into(),
                        of: String::new(),
                        kept: format!("{} (only yours, no passphrase)", key.display()),
                        may: "Signs in to a box as its admin with a certificate the box SSH \
                              secret signed, while it lasts (an hour)"
                            .into(),
                    })
                    .into_iter()
                    .chain(ssh::certs::kept_at().map(|kept| u::Held {
                        what: link_core::uninstall::CredentialKind::BoxSshSecret.into(),
                        of: String::new(),
                        kept,
                        may: "Signs certificates that sign in to every box this machine made, \
                              as their admin"
                            .into(),
                    }))
                    .collect(),
                ..u::Part::default()
            },
            ],
            stays: "Your boxes, and the images you saved of them (lnk box image list), stay in \
                    your cloud accounts, billed, until you delete them there."
                .into(),
            ..u::Kept::default()
        };
            u::run(args, &kept, || Ok(()), |_| Ok(()))?;
            return Ok(Done::shown());
        }
        command => command,
    };
    let shown = |name: &str| Name::new("box", name);
    match command {
        Command::BoxMachine(_) => {
            link_box::machine::serve()?;
        }
        Command::BoxEndHere(_) => link_box::ending::here()?,
        Command::UserAdd(add) => return users::user(out, users::UserCommand::Add(add)),
        Command::UserList(_) => return users::user(out, users::UserCommand::List),
        Command::UserRemove(remove) => {
            return users::user(out, users::UserCommand::Remove(remove));
        }
        Command::KeyShow(cli::KeyShowArgs { secret_file: true }) => {
            out.data(format!("{}\n", ssh::key()?.display()).as_bytes());
        }
        Command::KeyShow(cli::KeyShowArgs { secret_file: false }) => {
            out.data(format!("{}\n", ssh::public_key()?).as_bytes());
        }
        Command::BoxCreate(cli::BoxCreateArgs {
            bundle: Some(bundle),
            ..
        }) => return link_box::bundled::create(&bundle),
        Command::BoxCreate(cli::BoxCreateArgs {
            place,
            on,
            cloud,
            zone,
            here,
            name,
            spec,
            image,
            image_hash,
            bundle: None,
            yes,
        }) => {
            let slots = link_workplace::Slots {
                place,
                on,
                cloud,
                zone,
                here,
            };
            let Some(workplace) = slots::target(&slots)? else {
                return Err(usage(
                    sentence!("Couldn't make a box without saying where."),
                    prose!("Give --cloud <cloud> or --here."),
                    cmd!("lnk cloud list"),
                    "list your cloud accounts",
                ));
            };
            return make(
                out,
                &workplace,
                name,
                spec.as_deref(),
                &fields,
                match (&image, &image_hash) {
                    (Some(name), _) => Some(image::Pick::Named(name)),
                    (None, Some(hash)) => Some(image::Pick::Hashed(hash)),
                    (None, None) => None,
                },
                yes,
            );
        }
        Command::BoxShow(cli::BoxShowArgs { name, json: _ }) => {
            return inspecting::show(out, &name);
        }
        Command::BoxStart(cli::BoxStartArgs { name }) => {
            if let Some(machine) = link_box::native::find(&name)? {
                let starting = out.activity(phrase!("Starting {}", shown(&name)));
                link_box::native::start(&machine)?;
                drop(starting);
                return Ok(
                    Done::success(sentence!("{} is up again here.", shown(&name)))
                        .next(cmd!("lnk box ssh {}", shown(&name)), "open a shell in it"),
                );
            }
            let ip = restart(out, &name)?;
            return Ok(Done::success(sentence!(
                "{} is up again at {}.",
                shown(&name),
                Address::new(&ip)
            ))
            .next(cmd!("lnk box ssh {}", shown(&name)), "open a shell on it"));
        }
        Command::SizeList(cli::SizeListArgs {
            place,
            cloud,
            zone,
            spec,
            json: _,
        }) => {
            let cloud = one_cloud(place, cloud, "box size list")?;
            return list_sizes(out, &cloud, zone.as_deref(), spec.as_deref(), &fields);
        }
        Command::SpecShow(show) => return print_spec(out, &show, &fields),
        Command::BoxCheck(cli::BoxCheckArgs {
            plan: Some(kind),
            spec,
            ..
        }) => {
            let kind = link_box::spec::CloudKind::try_from(kind).map_err(|why| {
                usage(
                    Sentence::adopt(&format!("{why}.")),
                    prose!("A cloud's kind is lowercase letters, digits and dashes, as lnk cloud list says it."),
                    cmd!("lnk cloud list"),
                    "list your cloud accounts and their kinds",
                )
            })?;
            return print_plan(out, &kind, spec.as_deref(), &fields);
        }
        Command::BoxCheck(cli::BoxCheckArgs {
            place,
            cloud,
            zone,
            spec,
            plan: None,
            json: _,
        }) => {
            let cloud = one_cloud(place, cloud, "box check")?;
            return check_attachments_on(out, spec.as_deref(), &fields, &cloud, zone.as_deref());
        }
        Command::BoxSsh(cli::BoxSshArgs { name, command }) => {
            if let Some(machine) = link_box::native::named(name.as_deref())? {
                out.hand_over();
                let status = link_box::native::shell(&machine, &command)?;
                return Ok(Done::passed(status.code().unwrap_or(1)));
            }
            let ip = reachable(out, name.as_deref())?;
            out.hand_over();
            let status = ssh::interactive(&ip, &command)?;
            return Ok(Done::passed(status.code().unwrap_or(1)));
        }
        Command::BoxForward(cli::BoxForwardArgs {
            name,
            port,
            socket,
            proxy_port,
        }) => {
            // Each port is from 0 to 65535, as its declaration holds it.
            if link_box::native::find(&name)?.is_some() {
                return Err(said(
                    Report::error(sentence!(
                        "Couldn't forward to {}, which is a box here.",
                        shown(&name)
                    ))
                    .why(prose!(
                        "It is this machine, so what runs in it reaches this machine's ports and sockets as its sandbox allows."
                    ))
                    .exit(Exit::Usage),
                ));
            }
            let ip = reachable(out, Some(&name))?;
            let status = match (socket, port) {
                (Some(socket), _) => ssh::forward_socket(&ip, &socket, proxy_port)?,
                (None, Some(port)) => ssh::forward(&ip, port, proxy_port)?,
                (None, None) => {
                    return Err(said(
                        Report::error(sentence!("Couldn't forward without a port or a socket."))
                            .next(out.typed().and("--port <port>"), "forward a port")
                            .next(out.typed().and("--socket <path>"), "forward a socket")
                            .exit(Exit::Usage),
                    ));
                }
            };
            return Ok(Done::passed(status.code().unwrap_or(1)));
        }
        Command::BoxNetworkForward(args) => {
            out.hand_over();
            std::process::exit(
                link_box::machine::network_forward(&args.name)?
                    .code()
                    .unwrap_or(1),
            );
        }
        Command::BoxRun(cli::BoxRunArgs { name, args }) => {
            if let Some(machine) = link_box::native::find(&name)? {
                out.hand_over();
                let status = link_box::native::run_lnk(&machine, &args)?;
                return Ok(Done::passed(status.code().unwrap_or(1)));
            }
            let ip = reachable(out, Some(&name))?;
            out.hand_over();
            let staged = Boxes::load()?.get(&name).is_some_and(|m| m.staged);
            let status = ssh::run_lnk(&ip, staged, &args)?;
            return Ok(Done::passed(status.code().unwrap_or(1)));
        }
        Command::BoxCopy(cli::BoxCopyArgs { name, files, dest }) => {
            let files: Vec<std::path::PathBuf> = files.into_iter().map(Into::into).collect();
            return copy(out, &name, &files, &dest);
        }
        Command::BoxList(cli::BoxListArgs {
            tag: tags,
            place,
            cloud,
            refresh,
            all_regions,
            here,
            json: _,
        }) => {
            let cloud = match (place, cloud) {
                (None, None) => None,
                (place, cloud) => Some(one_cloud(place, cloud, "box list")?),
            };
            if !tags.is_empty() {
                return list_tagged(out, &tags, cloud.as_deref());
            } else if cloud.is_some() {
                return Err(said(
                    Report::error(sentence!("Couldn't list by cloud without a tag."))
                        .why(prose!(
                            "The flag --cloud narrows a listing by tag, so it needs --tag."
                        ))
                        .next(out.typed().and("--tag <tag>"), "list the boxes with a tag")
                        .exit(Exit::Usage),
                ));
            } else {
                return list(out, refresh, all_regions, here).map_err(said);
            }
        }
        Command::BoxStop(cli::BoxStopArgs { name }) => {
            if let Some(machine) = link_box::native::find(&name)? {
                let stopping = out.activity(phrase!("Stopping {}", shown(&name)));
                link_box::native::stop(&machine)?;
                drop(stopping);
                return Ok(Done::success(sentence!(
                    "Stopped {}'s services; its home stays.",
                    shown(&name)
                ))
                .next(cmd!("lnk box start {}", shown(&name)), "start it again")
                .next(cmd!("lnk box delete {}", shown(&name)), "delete it"));
            }
            if link_box::machine::is_here(&name) {
                link_box::machine::binding(link_box::machine::call(
                    link_box_contract::HereOperation::Stop { name: name.clone() },
                )?)?;
                return Ok(Done::success(sentence!(
                    "Stopped {}.",
                    Name::new("box", &name)
                )));
            }
            let machine = Boxes::load()?.pick(Some(&name))?.clone();
            let shown = Name::new("box", &name);
            let stopping = out.activity(link_format::phrase!("Stopping {}", &shown));
            call(&machine, "stop").within("asking its cloud to stop it")?;
            drop(stopping);
            Boxes::update(|boxes| {
                if let Some(saved) = boxes.get_mut(&name)
                    && saved.id == machine.id
                {
                    saved.state = "stopped".into();
                }
            })
            .within("saving its state")?;
            out.block(Report::warning(sentence!(
                "Its disk is still billed until it's removed."
            )));
            return Ok(Done::success(sentence!("Stopped {}.", &shown))
                .next(cmd!("lnk box start {}", &shown), "start it again")
                .next(cmd!("lnk box delete {}", &shown), "delete it and its disk"));
        }
        Command::BoxDelete(cli::BoxDeleteArgs {
            name,
            yes,
            id,
            cloud,
            zone,
        }) => {
            if let Some(id) = id {
                let (Some(cloud), Some(zone)) = (cloud, zone) else {
                    return Err(usage(
                        sentence!(
                            "Couldn't delete a machine by its ID without its cloud and zone."
                        ),
                        prose!("The flag --id needs --cloud and --zone."),
                        cmd!("lnk box list --tag KEY=VALUE"),
                        "list boxes by a tag, with their zones",
                    ));
                };
                return delete_id(out, &id, &cloud, &zone, yes);
            }
            let Some(name) = name else {
                return Err(usage(
                    sentence!("Couldn't delete a box without its name."),
                    prose!("Give the name of the box to delete."),
                    cmd!("lnk box list"),
                    "list your boxes",
                ));
            };
            return delete(out, &name, yes);
        }
        Command::ImageCreate(cli::ImageCreateArgs {
            name,
            image,
            hash,
            yes,
        }) => return image::create(out, &name, &image, hash.as_deref(), yes),
        Command::ImageList(cli::ImageListArgs {
            place,
            cloud,
            hash,
            json: _,
        }) => {
            let cloud = one_cloud(place, cloud, "box image list")?;
            return image::list(out, &cloud, hash.as_deref());
        }
        Command::ImageDelete(cli::ImageDeleteArgs {
            image,
            place,
            cloud,
            yes,
        }) => {
            let cloud = one_cloud(place, cloud, "box image delete")?;
            return image::delete(out, &cloud, &image, yes);
        }
        Command::BoxResourceJournalSchema(_) | Command::Uninstall(_) => {
            unreachable!("answered above")
        }
    }
    Ok(Done::shown())
}

/// The cloud account a command whose main argument is the cloud names:
/// its positional, or `--cloud`, its strict form, given once.
fn one_cloud(
    place: Option<String>,
    cloud: Option<String>,
    command: &str,
) -> anyhow::Result<String> {
    match (place, cloud) {
        (Some(place), Some(cloud)) if place != cloud => Err(usage(
            sentence!("Couldn't tell which cloud account to use."),
            prose!("Give the cloud account once: as the first word, or as --cloud."),
            cmd!("lnk {} --help", command),
            "see its arguments",
        )),
        (Some(cloud), _) | (None, Some(cloud)) => Ok(cloud),
        (None, None) => Err(usage(
            sentence!("Couldn't tell which cloud account to use."),
            prose!("Give a cloud account (lnk cloud list), as the first word or as --cloud."),
            cmd!("lnk cloud list"),
            "list your cloud accounts",
        )),
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
