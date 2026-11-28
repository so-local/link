//! `lnk-secret`: the secret module's commands. `lnk` runs it for `lnk
//! secret ...`.

use std::io::IsTerminal;

use link_format::{
    Done, Exit, Fields, Foreign, Name, Note, Out, Question, Report, Section, Success, Table, Value,
    WithinExt, cmd, phrase, prose, sentence,
};
use link_secret::cli::{self, Command};
use link_secret::{failed, keeper};
use link_secret_contract::{
    self as secrets, Called, Grant, Grantee, Listed, Place, Removal, Sealing,
};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[link_secret_contract::saved, link_secret::settings::saved]);
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

/// A command line that can't be used: `headline`, the error's own words
/// as its why, and what to try.
fn refused(headline: link_format::Sentence, err: anyhow::Error) -> Report {
    failed(headline, err)
        .exit(Exit::Usage)
        .next(cmd!("lnk secret list"), phrase!("see what is kept"))
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let command = cli::parse();
    match command {
        Command::Uninstall(args) => {
            use link_core::uninstall as u;
            let kept = u::Kept {
                parts: vec![u::Part {
                    name: "secrets".into(),
                    about: "the secrets lnk secret keeps, and where each is granted".into(),
                    paths: vec![secrets::path(), link_secret::settings::path()?],
                    secrets: Some(vec![secrets::path()]),
                    keys: vec![secrets::KEYCHAIN.into()],
                    credentials: secrets::held(),
                    ..u::Part::default()
                }],
                ..u::Kept::default()
            };
            // Your files go with their paths; what the keeper keeps for you, by asking it.
            u::run(&args, &kept, || Ok(()), |_| secrets::clear(&secrets::reach))?;
            Ok(Done::shown())
        }
        Command::SecretCreate(args) => create(out, args),
        Command::SecretAdd(args) => add(out, args),
        Command::SecretRemove(args) => remove(out, args),
        Command::SecretList(args) => list(out, args.to),
        Command::SecretShow(args) => show(out, &args.secret),
        Command::KeeperCreate(_) => keeper::make(out),
        // Offered on REST alone, served by a cloud: no command reads them
        // here.
        Command::SecretPass(_) | Command::SecretRead(_) | Command::SecretWrite(_) => {
            unreachable!("no command reads a REST action")
        }
        Command::SecretKeep(args) => {
            keeper::keep(args.socket, args.store, args.sealed)?;
            Ok(Done::shown())
        }
        Command::SecretBootstrap(args) => {
            // Its stdout is the launcher's pipe: nothing of Out's goes on it.
            let _ = out.hand_over();
            keeper::bootstrap(&args.file, &args.instance).map_err(|err| {
                failed(sentence!("The bootstrap handoff failed."), err).exit(Exit::Refused)
            })?;
            Ok(Done::shown())
        }
        Command::SecretBridge(args) => {
            let started = link_sandbox_route::bridge::Started {
                socket: args.socket,
                runtime: args.runtime,
                settings: args.settings,
                keys: args.keys,
            };
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .within("starting its runtime")?;
            runtime
                .block_on(link_secret::bridge::serve(&started))
                .within("serving the secret route")?;
            Ok(Done::shown())
        }
        Command::SecretDelete(args) => delete(out, args),
    }
}

/// `lnk secret create <NAME> [--internet-host <host>]`: a secret kept,
/// given to no one yet.
fn create(out: &mut Out, args: cli::SecretCreateArgs) -> Result<Done, Report> {
    let cli::SecretCreateArgs {
        name,
        internet_host: host,
        header,
        format,
        basic_user: basic,
        yes,
        json: _,
    } = args;
    link_spec::check_variable(&name)
        .map_err(|err| refused(sentence!("That isn't a name a secret can have."), err))?;
    // A workload is given a secret sealed only, which its proxy adds
    // for one host: what it runs is its own to choose. One kept
    // plain is for what you run yourself (an environment you approve).
    let sealing = match &host {
        Some(host) => Some(
            Sealing::new(host, header.as_deref(), format.as_deref(), basic.as_deref())
                .map_err(|err| refused(sentence!("That isn't a way to seal a secret."), err))?,
        ),
        None => {
            if header.is_some() || format.is_some() || basic.is_some() {
                return Err(Report::error(sentence!(
                    "The flags --header, --format and --basic-user say how a sealed secret is given."
                ))
                .why(prose!("Give --internet-host too."))
                .exit(Exit::Usage)
                .next(
                    cmd!("lnk secret create <name> --internet-host <host>"),
                    phrase!("keep one sealed"),
                ));
            }
            None
        }
    };
    let value = read_secret(out, &name)?;
    keeper::ask_first(out, yes)?;
    let how = match &sealing {
        Some(sealing) => format!(
            "sealed: the proxy adds it to requests for {} alone, in {}",
            sealing.host, sealing.header
        ),
        None => "plain: what it's granted to reads it".to_string(),
    };
    let secrets::Added { id, before, left } = secrets::add(&name, &value, sealing, Vec::new())
        .map_err(|err| failed(sentence!("The secret could not be kept."), err))?;
    if let Some(before) = before {
        out.block(Note(sentence!(
            "{} replaced, which was {} ({}). Its ID and grants went with it.",
            Name::new("secret", &name),
            Foreign::new(&described(&before)),
            Value::from(before.resource())
        )));
    }
    if let Some(left) = left {
        out.block(Note(sentence!(
            "The Keychain kept item {} of the secret this replaced ({}): it holds a value nothing \
             reaches now. Delete it with Keychain Access.",
            Value::from(format!("{}:{}", secrets::keychain::SERVICE, left.account)),
            Foreign::new(&left.why)
        )));
    }
    link_actions::answer::reply::<cli::Kept>(
        out,
        &serde_json::json!({"resource": secrets::resource(&id)}),
    )?;
    Ok(Done::success(sentence!(
        "{} is kept on this machine as {}, {}, given to no one yet.",
        Name::new("secret", &name),
        Value::from(secrets::resource(&id)),
        Foreign::new(&how),
    ))
    .next(
        cmd!("lnk secret add <name> --to <kind>:<name>"),
        phrase!("give it to a workload"),
    ))
}

/// Who `texts` name, as `kind:name` each, parsed.
fn parsed_grants(texts: &[String]) -> Result<Vec<Grantee>, Report> {
    texts
        .iter()
        .map(|to| Grantee::parse(to, &secrets::grantee))
        .collect::<anyhow::Result<Vec<_>>>()
        .map_err(|err| refused(sentence!("That isn't a grant."), err))
}

/// `lnk secret add <NAME> --to <KIND>:<NAME> ...`: a secret given where it
/// wasn't.
fn add(_out: &mut Out, args: cli::SecretAddArgs) -> Result<Done, Report> {
    let name = args.secret.clone();
    let which = called(&name)?;
    let to = parsed_grants(&args.to)?;
    let granted = to.iter().map(Grantee::to_string).collect::<Vec<_>>();
    let changed = secrets::grant(&which, to, Vec::new()).map_err(|err| {
        failed(sentence!("The secret could not be given."), err).exit(Exit::Usage)
    })?;
    let secret = Name::new("secret", &name);
    Ok(Done::success(match changed {
        true => sentence!(
            "{} is given to {}.",
            secret,
            Foreign::new(&granted.join(" and "))
        ),
        false => sentence!(
            "{} was given to {} already.",
            secret,
            Foreign::new(&granted.join(" and "))
        ),
    })
    .next(cmd!("lnk secret list"), phrase!("see what is kept")))
}

/// `lnk secret remove <NAME> --to <KIND>:<NAME> ...`: a secret no longer
/// given where it was.
fn remove(_out: &mut Out, args: cli::SecretRemoveArgs) -> Result<Done, Report> {
    let name = args.secret.clone();
    let which = called(&name)?;
    let from = parsed_grants(&args.to)?;
    let ungranted = from.iter().map(Grantee::to_string).collect::<Vec<_>>();
    let changed = secrets::grant(&which, Vec::new(), from).map_err(|err| {
        failed(sentence!("The secret could not be taken from them."), err).exit(Exit::Usage)
    })?;
    let secret = Name::new("secret", &name);
    Ok(Done::success(match changed {
        true => sentence!(
            "{} is no longer given to {}.",
            secret,
            Foreign::new(&ungranted.join(" and "))
        ),
        false => sentence!(
            "{} was not given to {}.",
            secret,
            Foreign::new(&ungranted.join(" and "))
        ),
    })
    .next(cmd!("lnk secret list"), phrase!("see what is kept")))
}

/// `lnk secret list [--to <kind>:<name>] [--json]`: the secrets kept here
/// and, without `--to`, every credential the modules keep on this machine.
fn list(out: &mut Out, to: Option<String>) -> Result<Done, Report> {
    let mut listed =
        secrets::list().map_err(|err| failed(sentence!("The secrets can't be listed."), err))?;
    if let Some(to) = &to {
        let here = secrets::grantee(to)
            .map_err(|err| refused(sentence!("That isn't a workload here."), err))?;
        let of = here.service().clone();
        listed.retain(|l| l.to.iter().any(|g| g.principal == of));
    }
    let held = match to {
        Some(_) => Vec::new(),
        None => credentials()?,
    };
    link_actions::answer::reply::<cli::Listed>(
        out,
        &serde_json::json!({
        "secrets": listed
            .iter()
            .map(|l| {
                let sealed = l.sealed.as_ref();
                serde_json::json!({
                    "resource": l.resource(),
                    "kind": l.kind,
                    "name": l.name,
                    "owner": l.owner.as_ref().map(|o| serde_json::json!({
                        "principal": o.principal,
                        "as": o.r#as,
                    })),
                    "host": sealed.map(|s| &s.host),
                    "header": sealed.map(|s| &s.header),
                    "format": sealed.map(Sealing::format),
                    "basic": sealed.and_then(|s| s.basic.as_ref()),
                    // Each grant as `grant` declares it, how it's given
                    // tagged by its kind.
                    "to": &l.to,
                    "copies": l.copies.iter().map(copy_json).collect::<Vec<_>>(),
                    "kept": l.kept,
                })
            })
            .collect::<Vec<_>>(),
        "credentials": held,
        }),
    )?;
    if out.is_json() {
        return Ok(Done::shown());
    }
    let mut table = Table::new(["Secret", "Given"], sentence!("No secrets are kept here."));
    for l in &listed {
        table.row([
            Value::from(Name::new("secret", l.called())),
            Value::from(Foreign::new(&described(l))),
        ]);
    }
    let empty = table.is_empty();
    out.block(table);
    for one in &held {
        let heading = match one.of.is_empty() {
            true => phrase!("{}", Foreign::new(&one.what)),
            false => phrase!("{}, {}", Foreign::new(&one.what), Foreign::new(&one.of)),
        };
        out.block(
            Section::new(heading).with(
                Fields::new()
                    .row("Kept", Foreign::new(&one.kept))
                    .row("May", Foreign::new(&one.may)),
            ),
        );
    }
    Ok(match empty {
        true => Done::shown().next(
            cmd!("lnk secret create <name> --internet-host <host>"),
            phrase!("keep one"),
        ),
        false => Done::shown().next(cmd!("lnk secret delete <name>"), phrase!("take one away")),
    })
}

/// `lnk secret show <NAME>`: how one secret is given and where, never its
/// value: the state `lnk issue create` puts in an issue.
fn show(out: &mut Out, name: &str) -> Result<Done, Report> {
    let which = called(name)?;
    let listed =
        secrets::list().map_err(|err| failed(sentence!("The secrets can't be listed."), err))?;
    let Some(found) = listed.iter().find(|l| l.is(&which)) else {
        return Err(Report::error(sentence!(
            "There is no secret {} here.",
            Name::new("secret", name)
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk secret list"), phrase!("see what is kept")));
    };
    out.block(
        Fields::new()
            .key("Secret", Name::new("secret", found.called()))
            .row("ID", Value::from(found.resource()))
            .row("Given", Foreign::new(&given(found)))
            .row(
                "Kept",
                Foreign::new(if found.kept { "here" } else { "elsewhere" }),
            ),
    );
    Ok(Done::shown().next(cmd!("lnk secret list"), phrase!("see what is kept")))
}

/// Every credential the modules keep on this machine, where each is kept
/// and what it may do, as each says (`lnk uninstall --parts --json`,
/// `link_core::uninstall::Held`). Never a value.
fn credentials() -> Result<Vec<link_core::uninstall::Held>, Report> {
    let output = link_core::lnk()
        .args(["uninstall", "--parts", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .output()
        .within("running lnk uninstall --parts")?;
    if !output.status.success() {
        return Err(
            Report::error(sentence!("lnk couldn't say which credentials it holds."))
                .why(prose!("Its uninstall --parts failed."))
                .next(cmd!("lnk upgrade"), phrase!("upgrade it")),
        );
    }
    let kept: link_core::uninstall::Kept =
        serde_json::from_slice(&output.stdout).within("reading what lnk uninstall --parts said")?;
    Ok(kept.parts.into_iter().flat_map(|p| p.credentials).collect())
}

/// `lnk secret delete <NAME|secret:<id>> [--here]`, or on a box a move
/// gave a copy to, `lnk secret delete --here --service service:<id> --own
/// <use>`.
fn delete(
    out: &mut Out,
    cli::SecretDeleteArgs {
        secret: name,
        here,
        service,
        own,
        json: _,
    }: cli::SecretDeleteArgs,
) -> Result<Done, Report> {
    let what = match (name, service, own) {
        (Some(name), None, None) => Removal::Secret(called(&name)?),
        (None, Some(service), Some(own)) => {
            let principal = service
                .as_str()
                .strip_prefix("service:")
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{:?}: service:<its id>",
                        link_format::clean(service.as_str())
                    )
                })
                .and_then(secrets::service_principal)
                .within("--service")
                .map_err(|err| refused(sentence!("That isn't a service."), err))?;
            let r#as = secrets::Use::from_flag(&own)
                .within("--own")
                .map_err(|err| refused(sentence!("That isn't a service's own secret."), err))?;
            Removal::Own(secrets::Owner { principal, r#as })
        }
        _ => {
            return Err(refused(
                sentence!("That isn't a secret to delete."),
                anyhow::anyhow!("give a secret's name or ID"),
            ));
        }
    };
    let shown = what.to_string();
    let removed = secrets::remove(&what, (!here).then_some(&secrets::reach as _))
        .map_err(|err| failed(sentence!("The secret could not be removed."), err))?;
    link_actions::answer::reply::<cli::Deleted>(
        out,
        &serde_json::json!({
        "resource": removed.id.as_deref().map(secrets::resource),
        "removed": removed.here,
        "copies": removed.reached.iter().map(copy_json).collect::<Vec<_>>(),
        "left": removed
            .unreached
            .iter()
            .map(|(copy, _)| copy_json(copy))
            .chain(removed.with_service.iter().map(copy_json))
            .collect::<Vec<_>>(),
        }),
    )?;
    let nothing = !removed.here
        && removed.reached.is_empty()
        && removed.unreached.is_empty()
        && removed.with_service.is_empty();
    match (nothing, here) {
        // A box the copy already left: nothing to do there.
        (true, true) => {
            out.block(Note(sentence!(
                "There is no secret {} here: nothing to remove.",
                Name::new("secret", &shown)
            )));
            return Ok(Done::shown());
        }
        (true, false) => {
            return Err(Report::error(sentence!(
                "There is no secret {} here.",
                Name::new("secret", &shown)
            ))
            .exit(Exit::Usage)
            .next(cmd!("lnk secret list"), phrase!("see what is kept")));
        }
        _ => {}
    }
    let secret = Name::new("secret", &shown);
    if removed.here {
        out.block(Success::new(sentence!(
            "{} is removed from this machine, and {} is never given again.",
            &secret,
            Value::from(
                removed
                    .id
                    .as_deref()
                    .map(secrets::resource)
                    .unwrap_or_default()
            )
        )));
    }
    for copy in &removed.reached {
        out.block(Success::new(sentence!(
            "{} is removed from {}.",
            &secret,
            Foreign::new(&copy.to_string())
        )));
    }
    for copy in &removed.with_service {
        out.block(Note(sentence!(
            "{} is left on {}: removed with its service there.",
            &secret,
            Foreign::new(&copy.to_string())
        )));
    }
    if !removed.unreached.is_empty() {
        let left = removed
            .unreached
            .iter()
            .map(|(copy, why)| format!("{copy} ({})", link_format::clean(why)))
            .collect::<Vec<_>>()
            .join(", ");
        let again = removed
            .id
            .as_deref()
            .map(secrets::resource)
            .unwrap_or_else(|| shown.clone());
        return Err(
            Report::error(sentence!("{} is left on {}.", &secret, Foreign::new(&left))).next(
                cmd!("lnk secret delete {}", again.as_str()),
                phrase!("remove it again once they can be reached"),
            ),
        );
    }
    Ok(Done::shown().next(cmd!("lnk secret list"), phrase!("see what is kept")))
}

/// A secret as a command line names it, by its name or `secret:<id>`.
fn called(text: &str) -> Result<Called, Report> {
    Called::parse(text).map_err(|err| {
        refused(
            sentence!("That isn't a secret's name or its ID, secret:<id>."),
            err,
        )
    })
}

/// A copy's place as `--json` writes it: its name, whether it's an
/// account, and a box's machine as `resource: "box:<id>"`.
fn copy_json(place: &Place) -> serde_json::Value {
    serde_json::json!({
        "name": place.name,
        "cloud": place.cloud,
        "resource": place.resource(),
    })
}

/// How a secret is given and where, in words, and where a move gave a
/// copy of it.
fn described(listed: &Listed) -> String {
    let copies = listed
        .copies
        .iter()
        .map(Place::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if !listed.kept {
        return format!("not kept here, left on {copies}");
    }
    let given = given(listed);
    match copies.is_empty() {
        true => given,
        false => format!("{given}; copies on {copies}"),
    }
}

/// How a secret is given and where, in words.
fn given(listed: &Listed) -> String {
    let to = listed
        .to
        .iter()
        .map(Grant::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    match &listed.sealed {
        Some(sealing) => format!(
            "sealed, for {}, in {}: {}, to {to}",
            sealing.host,
            sealing.header,
            sealing.format()
        ),
        None => format!("plain, to {to}"),
    }
}

/// A secret's value: asked without echo on a terminal, else its input's
/// first line.
fn read_secret(out: &mut Out, name: &str) -> Result<link_secretive::Secret, Report> {
    let mut value = link_secretive::Secret::default();
    if std::io::stdin().is_terminal() {
        value = out.ask(Question::hidden(sentence!(
            "What is the value of {}?",
            Name::new("secret", name)
        )))?;
    } else if let Some(read) = link_secretive::Secret::read_line(&mut std::io::stdin().lock())
        .within("reading the secret")?
    {
        value = read;
    }
    Ok(value)
}
