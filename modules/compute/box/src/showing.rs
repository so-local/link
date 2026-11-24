//! `lnk box spec show`, `lnk box check --plan` and `lnk box size list`: a box's
//! spec, what it asks a cloud for, and the sizes a cloud offers, printed.

use super::*;
use link_format::{Section, Size, Table, Text, Version};

/// `lnk box spec show`: the spec of box `show.thing` as made again, its
/// machine pinned as its cloud says it is, or the spec a file makes,
/// with the field flags `flags` over either.
pub(super) fn print_spec(
    out: &mut Out,
    show: &link_spec::Show,
    flags: &link_spec::FieldFlags,
) -> anyhow::Result<Done> {
    let made = match (&show.thing, &show.spec) {
        (Some(_), Some(_)) => None,
        (Some(name), None) => Some(made_again(out, name)?),
        (None, _) => None,
    };
    // The checks serde can't make hold the printed spec too.
    match &made {
        Some(made) => link_spec::over(made, flags)?.check("", false)?,
        None if show.thing.is_none() => {
            BoxSpec::made(show.spec.as_deref(), flags, None)?;
        }
        None => {}
    }
    let resource = |_: &str| made.clone().within("a box's spec");
    let shown = link_spec::show::<BoxSpec>(show, flags, Some(&resource))?;
    match show.output.json {
        true => out.reply(&serde_json::from_str::<serde_json::Value>(&shown)?),
        false => out.data(shown.as_bytes()),
    }
    Ok(Done::shown())
}

/// The spec box `name` was made from, its machine pinned as its cloud
/// says it is: what makes that box again.
fn made_again(out: &mut Out, name: &str) -> anyhow::Result<BoxSpec> {
    let shown = Name::new("box", name);
    if link_box::machine::is_here(name) {
        let binding = link_box::machine::inspect(name)?;
        let spec = BoxSpec {
            isolation: binding
                .inputs
                .vm
                .as_ref()
                .map(|input| link_box::spec::Isolation {
                    vm: Some(input.provenance.reference.clone().into()),
                    no_sandbox: None,
                    sandbox: None,
                }),
            env: binding
                .inputs
                .environment
                .as_ref()
                .map(|input| input.provenance.reference.clone().into()),
            ..Default::default()
        };
        return Ok(spec);
    }
    let machine = Boxes::load()?.pick(Some(name))?.clone();
    if machine.id.is_empty() {
        return Err(said(
            Report::error(sentence!(
                "Couldn't print the spec of {}, which isn't made yet.",
                &shown
            ))
            .why(prose!(
                "Another lnk box create is making it, or stopped before it could."
            ))
            .next(cmd!("lnk box list"), "list your boxes")
            .exit(Exit::Usage),
        ));
    }
    // What this computer kept (none for a box made before specs,
    // or on another computer: its defaults apply), never the box's.
    let made: BoxSpec = machine
        .spec
        .clone()
        .and_then(|t| toml::Value::Table(t).try_into().ok())
        .unwrap_or_default();
    let found = match call(&machine, "describe") {
        Ok(reply) => reply.boxes.unwrap_or_default().into_iter().next(),
        Err(err) => {
            out.block(
                    Report::warning(sentence!(
                        "Its cloud didn't say what {} is, so its size and place are as this computer last saw them.",
                        &shown
                    ))
                    .why(words(&err)),
                );
            None
        }
    };
    let found = found.unwrap_or_else(|| cloud::Found {
        size: machine.size.clone(),
        place: machine.place.clone(),
        id: machine.id.clone(),
        name: machine.name.clone(),
        machine_id: machine.machine_id.clone(),
        owner: None,
        state: machine.state.clone(),
        arch: None,
        deadline: None,
        disk_gb: None,
        disk_type: None,
        ip: None,
        os: None,
        spot: None,
        stopped_by_cloud: None,
        tags: Default::default(),
    });
    let mut spec = made.pinned(&machine.kind, &found);
    // A file can't install an lnk behind this computer's: so
    // the spec reads back, a lower one is said, not printed.
    let this = link_version::version();
    if let Some(version) = spec.lnk_version()
        && version < this
    {
        spec.lnk.get_or_insert_default().version = None;
        out.block(Report::warning(sentence!(
                "{} was made with lnk {}, behind this computer's {}, which a spec can't install, so it is left out and the spec installs the latest; to make that release again, give --lnk.version {} with it.",
                &shown,
                Version::new(version.to_string()),
                Version::new(this.to_string()),
                Version::new(version.to_string())
            )));
    }
    Ok(spec)
}

/// `lnk box check --plan <kind>`: what a box of the spec `file` and `flags` make
/// asks a cloud of `kind` for, every default filled in.
pub(super) fn print_plan(
    out: &mut Out,
    kind: &link_box::spec::CloudKind,
    file: Option<&std::path::Path>,
    flags: &link_spec::FieldFlags,
) -> anyhow::Result<Done> {
    let spec = BoxSpec::made(file, flags, None)?.spec;
    let want = spec.want(kind.as_str());
    want.check_buildable()?;
    out.reply(&link_actions::answer::output::<link_box::cli::Checked>(
        &serde_json::json!({"kind": "plan", "want": want}),
    )?);
    out.data(toml::to_string_pretty(&want)?.as_bytes());
    Ok(Done::shown())
}

/// `lnk box size list`: what `create` would pick in `account`, and the
/// rest that fit.
pub(super) fn list_sizes(
    out: &mut Out,
    account_name: &str,
    zone: Option<&str>,
    file: Option<&std::path::Path>,
    flags: &link_spec::FieldFlags,
) -> anyhow::Result<Done> {
    let account = account(account_name)?;
    let kind = account.kind.clone();
    let spec = BoxSpec::made(file, flags, zone.map(|zone| (kind.as_str(), zone)))?.spec;
    let want = spec.want(&kind);
    want.check_buildable()?;
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
    // Listing is what it's for: the adapter is asked whatever the spec.
    let listed = match offered(&account, &place) {
        Offered::Failed(why) => {
            return Err(said(
                Report::error(sentence!(
                    "Couldn't list the machine types of {} in {}.",
                    Name::new("account", account_name),
                    place.as_str()
                ))
                .why(Prose::from(Sentence::adopt(&why)))
                .next(cmd!("lnk cloud list"), "check the cloud account"),
            ));
        }
        listed => listed,
    };
    let chosen = choose_from(&kind, Some(listed), &place, defaults.size, &want)?;
    for note in &chosen.notes {
        out.block(Report::warning(Sentence::adopt(note)));
    }
    out.reply(&link_actions::answer::output::<link_box::cli::Sizes>(
        &serde_json::json!({
            "pick": chosen.size,
            "place": place,
            "sizes": chosen.fitting,
            "rates": chosen.rates,
            "disk_gb": want.disk.whole_gb(),
            "disk_type": want.disk_type,
        }),
    )?);
    let account_shown = Name::new("account", account_name);
    if chosen.fitting.is_empty() {
        out.block(Text(prose!(
            "A box made in {} would be a machine of type {} in {}; the adapter {} lists no more.",
            &account_shown,
            chosen.size.as_str(),
            place.as_str(),
            Name::new("module", format!("lnk-{kind}"))
        )));
        return Ok(Done::shown().next(
            cmd!("lnk box create {}", &account_shown),
            "make a box with it",
        ));
    }
    const SHOWN: usize = 20;
    let mut table = Table::new(
        ["Size", "Cpus", "Memory", "Gpus", "Arch", "Price"],
        sentence!("No machine types fit."),
    )
    .right(1)
    .right(2);
    for s in chosen.fitting.iter().take(SHOWN) {
        let gpus = match s.gpus {
            0 => "-".to_string(),
            n => format!("{n} {}", clean(s.gpu_model())),
        };
        table.row([
            Value::from(clean(&s.size)),
            Value::from(s.cpus.to_string()),
            Size(s.memory_mb() * 1_048_576).into(),
            Value::from(gpus),
            Value::from(clean(&s.arch)),
            Value::from(s.price.map_or("-".into(), price)),
        ]);
    }
    out.block(Section::new(phrase!("In {} ({})", &account_shown, place.as_str())).with(table));
    out.block(Text(prose!(
        "The first is the pick: {}.",
        Value::from(clean(&chosen.size))
    )));
    if chosen.fitting.len() > SHOWN {
        out.block(Text(prose!(
            "And {} more; --json lists them all.",
            chosen.fitting.len() - SHOWN
        )));
    }
    Ok(Done::shown()
        .next(
            cmd!("lnk box create {}", &account_shown),
            "make a box with the pick",
        )
        .next(
            cmd!("lnk box check --plan {}", Name::new("cloud", &kind)),
            "see what it asks of that cloud",
        ))
}
