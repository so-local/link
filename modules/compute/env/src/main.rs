//! `lnk-env`: the env module's commands. `lnk` runs it for `lnk env ...`.

use link_env::cli::{self, At, Command};
use link_env::{file, here, remote, values};

mod approving;
use link_format::{
    Done, Exit, Fields, Foreign, Name, Out, Path as Shown, Report, Section, Table, Value,
    WithinExt, cmd, prose, sentence,
};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[
        link_env_contract::approvals::saved,
        link_env_contract::remote::saved_made,
        link_env_contract::workloads::saved,
    ]);
    link_core::describe_component!(link_env_contract::file::Env);
    link_settings::output::run(try_main)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    let parsed = cli::parse();
    if let Command::Uninstall(given) = &parsed.command.command {
        let args = given;
        use link_core::uninstall as u;
        let kept = u::Kept {
            parts: vec![u::Part {
                name: "environments".into(),
                about: "saved environment settings".into(),
                paths: vec![u::settings(link_files::layout::Hidden::Envs)?.join("secrets.toml")],
                secrets: Some(vec![
                    u::settings(link_files::layout::Hidden::Envs)?.join("secrets.toml"),
                ]),
                ..u::Part::default()
            }],
            stays: "Your boxes and the images of your environments stay in your cloud \
                    accounts, and your jobs' output in your buckets."
                .into(),
            ..u::Kept::default()
        };
        u::run(args, &kept, || Ok(()), |_| Ok(()))?;
        return Ok(Done::shown());
    }

    let fields = parsed.fields;
    // The environment a command names, with its field flags over it.
    let given = |path: &std::path::Path| file::read_with(path, &fields);
    match parsed.command.command {
        Command::Uninstall(_) => unreachable!("uninstall handled above"),
        Command::EnvCheck(args) => check(out, &args.spec, &fields),
        Command::SpecShow(show) => {
            let shown = link_spec::show::<link_env_contract::file::Env>(&show, &fields, None)?;
            out.data(shown.as_bytes());
            Ok(Done::shown())
        }
        Command::ImageCreate(args) => {
            let timeout = args.timeout.map(|t| t.get());
            let slots = cli::Slots {
                place: args.place,
                cloud: args.cloud,
                here: args.here,
                ..cli::Slots::default()
            };
            match placed(
                &slots,
                "env image",
                cmd!("lnk env image create --help"),
                &["--cloud <cloud>", "--here"],
            )? {
                At::Cloud { cloud, zone: None } => {
                    remote::image(out, &given(&args.spec)?, &cloud, timeout, args.yes)
                }
                At::Cloud { zone: Some(_), .. } => Err(Report::error(sentence!(
                    "Couldn't build an image in one zone of a cloud."
                ))
                .why(prose!(
                    "An image is the cloud account's, whatever the zone of the box that builds it."
                ))
                .next(
                    cmd!("lnk env image create <cloud> --spec <file>"),
                    "build it in the account",
                )
                .exit(Exit::Usage)),
                At::Box(_) => unreachable!("placed refuses a box"),
                At::Here => remote::image_here(&args.spec, &fields, timeout),
            }
        }
        Command::EnvSetup(args) => {
            let slots = cli::Slots {
                place: args.place,
                on: args.on,
                here: args.here,
                ..cli::Slots::default()
            };
            let on = match placed(
                &slots,
                "env setup",
                cmd!("lnk env setup --help"),
                &["--on <box>", "--here"],
            )? {
                At::Box(name) => Some(name),
                At::Here => None,
                At::Cloud { .. } => unreachable!("placed refuses a cloud"),
            };
            remote::setup(
                &given(&args.spec)?,
                on.as_deref(),
                args.timeout.map(|t| t.get()),
            )
        }
        Command::EnvCreate(args) => remote::start(
            out,
            &given(&args.spec)?,
            &args.cloud,
            args.name.as_deref(),
            args.r#box.as_deref(),
            args.yes,
        ),
        Command::EnvReset(args) => {
            let slots = cli::Slots {
                place: args.place,
                on: args.on,
                ..cli::Slots::default()
            };
            let (machine, spec) =
                on_box(&slots, "env reset", cmd!("lnk env reset --help"), args.spec)?;
            remote::reset(&given(&spec)?, &machine, &args.service)
        }
        Command::EnvExport(args) => {
            out.data(&line(&link_env::carried::export(&given(&args.spec)?)?)?);
            Ok(Done::shown())
        }
        Command::EnvPrepare(args) => {
            out.data(&line(&link_env::carried::prepare(&given(&args.spec)?)?)?);
            Ok(Done::shown())
        }
        Command::EnvImport(_) => {
            out.data(&line(&link_env::carried::import(std::io::stdin().lock())?)?);
            Ok(Done::shown())
        }
        Command::EnvHere(_) => Ok(Done::passed(here::main(out)?)),
        approval @ (Command::ApprovalAdd(_)
        | Command::ApprovalRemove(_)
        | Command::ApprovalList(_)
        | Command::ApprovalShow(_)) => approving::run(out, approval),
        Command::EnvServe(args) => {
            out.hand_over();
            Ok(Done::passed(here::serve(
                &args.spec,
                &args.service,
                args.no_sandbox,
            )?))
        }
    }
}

/// A document for another program: one compact line.
fn line(document: &impl serde::Serialize) -> Result<Vec<u8>, Report> {
    let mut bytes = serde_json::to_vec(document).within("writing the document")?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The box and the file of its environment `lnk env reset`
/// acts on: `--spec`, else the file the box was made from.
fn on_box(
    slots: &cli::Slots,
    command: &str,
    help: link_format::Command,
    spec: Option<std::path::PathBuf>,
) -> Result<(String, std::path::PathBuf), Report> {
    let At::Box(on) = placed(slots, command, help, &["--on <box>"])? else {
        unreachable!("placed refuses all but a box");
    };
    let spec = match spec {
        Some(spec) => spec,
        None => remote::made_from(&on)?,
    };
    Ok((on, spec))
}

/// `lnk env check --spec <file>`: reads it as every command does, with its
/// field flags, and says what it sets up and the hash its image is found
/// by.
fn check(
    out: &mut Out,
    path: &std::path::Path,
    fields: &link_spec::FieldFlags,
) -> Result<Done, Report> {
    let read = file::read_with(path, fields).map_err(|err| {
        Report::error(sentence!(
            "The environment file {} doesn't read.",
            Shown::new(path)
        ))
        .why(prose!(
            "{}.",
            Foreign::new(format!("{err:#}").trim_end_matches('.'))
        ))
        .exit(Exit::Usage)
        .next(cmd!("lnk env --help"), "see the environment commands")
    })?;
    // A secret it names as the other kind than it's kept here: what a
    // start refuses is refused here first.
    values::check(&read)?;
    let f = &read.file;
    let platform = f.platform();
    let hash = file::hash(&read, platform)?;
    link_actions::answer::reply::<link_env_contract::declared::Checked>(
        out,
        &serde_json::json!({
            "name": f.name,
            "hash": hash,
            "os": platform.os.word(),
            "arch": platform.arch.word(),
        }),
    )?;
    let mut about = Fields::new().key("Image hash", hash.as_str()).row(
        "Image for",
        format!(
            "{} {}{}",
            platform.os.word(),
            platform.arch.word(),
            match f.os {
                Some(_) => "",
                None => " (it names no OS: an image for each)",
            }
        ),
    );
    if let Some(workdir) = f.workdir() {
        about = about.row("Workdir", format!("~/{}", workdir.display()));
    }
    if !f.variables.is_empty() {
        let names: Vec<&str> = f.variables.keys().map(String::as_str).collect();
        about = about.row("Variables", names.join(", "));
    }
    if !f.secret_names().is_empty() {
        about = about.row("Secrets", f.secret_names().join(", "));
    }
    if !f.sealed_names().is_empty() {
        about = about.row("Sealed", f.sealed_names().join(", "));
        about = about.row(
            "A build is given",
            match remote::root_stage(f) {
                Some(stage) => format!("no sealed secret, since stage {stage} runs as root"),
                None => "the sealed secrets, for git and the proxy".to_string(),
            },
        );
    }
    if let Some(service) = f.root_service() {
        about = about.row(
            "A job runs",
            format!(
                "on a box of its own, since its service {} runs as root",
                service.name
            ),
        );
    }
    out.block(about);
    let mut stages = Table::new(
        ["Stage", "Runs", "How"],
        sentence!("The environment has no stages."),
    );
    for stage in f.stages().iter().flat_map(|s| s.members()) {
        let how = match (&stage.user, &stage.sandbox) {
            (Some(user), _) if user.is_root() => "as root, outside any sandbox".to_string(),
            (_, Some(spec)) => format!("in {}", spec.display()),
            (_, None) => String::new(),
        };
        let when = match stage.saved() {
            true => "in the image",
            false => "at every start",
        };
        stages.row([
            Value::from(Name::new("stage", &stage.name)),
            Value::from(when),
            Value::from(how),
        ]);
    }
    out.block(Section::new("Stages").with(stages));
    let mut services = Table::new(
        ["Service", "Ready"],
        sentence!("The environment has no services."),
    );
    for service in f.services() {
        let ready = match (service.ready.port, &service.ready.run) {
            (Some(port), _) => format!("on port {port}"),
            _ => "by a command".into(),
        };
        services.row([
            Value::from(Name::new("service", &service.name)),
            Value::from(ready),
        ]);
    }
    out.block(Section::new("Services").with(services));
    Ok(Done::success(sentence!(
        "{} reads: environment {}.",
        Shown::new(path),
        Name::new("environment", &f.name)
    ))
    .next(
        cmd!("lnk env image create <cloud> --spec {}", Shown::new(path)),
        "build its image",
    ))
}

/// Where `slots` say `command` acts, one of `allowed`: a box (`--on`), a
/// cloud account (`--cloud`) or this machine (`--here`). Another, or none,
/// is refused saying which are taken.
fn placed(
    slots: &cli::Slots,
    command: &str,
    help: link_format::Command,
    allowed: &[&str],
) -> Result<At, Report> {
    if *slots != cli::Slots::default() {
        link_core::modules::ensure("box", "--on, --cloud")?;
    }
    let at = cli::at(slots, command)?;
    let takes = |flag: &str| allowed.iter().any(|a| a.starts_with(flag));
    match at {
        Some(At::Here) if takes("--here") => Ok(At::Here),
        Some(on @ At::Box(_)) if takes("--on") => Ok(on),
        Some(cloud @ At::Cloud { .. }) if takes("--cloud") => Ok(cloud),
        _ => Err(Report::error(sentence!(
            "Couldn't tell where {} acts.",
            link_format::Value::from(command.to_string())
        ))
        .why(prose!(
            "It takes {}.",
            link_format::Foreign::new(&allowed.join(" or "))
        ))
        .next(help, "see its flags")
        .exit(Exit::Usage)),
    }
}
