//! `lnk env approval`: your yes to giving an environment file a secret
//! it asks for, given ahead of a run ([`link_env::approvals`]).

use link_env::approvals::{self, Approval, Yours};
use link_env::cli::{self, Command};
use link_env::file;
use link_format::{
    Done, Exit, Foreign, Name, Out, Path as Shown, Report, Table, Value, WithinExt, cmd, prose,
    sentence,
};

/// `lnk env approval ...`, whose yes is kept for the environment file as
/// written: its actions bind no spec and take the file as it is, so they
/// take no field flags.
pub fn run(out: &mut Out, command: Command) -> Result<Done, Report> {
    match command {
        Command::ApprovalAdd(of) => add(&of.secret, &of.spec),
        Command::ApprovalRemove(of) => remove(&of.secret, &of.spec),
        Command::ApprovalList(list) => self::list(out, &list),
        Command::ApprovalShow(show) => self::show(out, &show),
        _ => unreachable!("main hands approval commands alone here"),
    }
}

/// Your secrets kept now, and whether one of their IDs is still kept.
fn yours() -> Result<Vec<Yours>, Report> {
    approvals::yours().map_err(|err| {
        Report::error(sentence!("Your secrets can't be read.")).why(prose!(
            "{}.",
            Foreign::new(format!("{err:#}").trim_end_matches('.'))
        ))
    })
}

/// The spec at `path`, read: its real path and its `name`.
fn spec(path: &std::path::Path) -> Result<(std::path::PathBuf, file::Read), Report> {
    let read = file::read(path).map_err(|err| {
        Report::error(sentence!(
            "The environment file {} doesn't read.",
            Shown::new(path)
        ))
        .why(prose!(
            "{}.",
            Foreign::new(format!("{err:#}").trim_end_matches('.'))
        ))
        .exit(Exit::Usage)
    })?;
    Ok((approvals::spec_path(path), read))
}

fn refused(what: link_format::Sentence, err: anyhow::Error) -> Report {
    Report::error(what)
        .why(prose!(
            "{}.",
            Foreign::new(format!("{err:#}").trim_end_matches('.'))
        ))
        .exit(Exit::Usage)
}

/// `lnk env approval add <secret> --spec <file>`.
fn add(secret: &str, file: &std::path::Path) -> Result<Done, Report> {
    let (path, read) = spec(file)?;
    let yours = yours()?;
    let secret = approvals::find(&yours, secret)
        .map_err(|err| refused(sentence!("That isn't a secret of yours."), err))?;
    let f = &read.file;
    if !f
        .secret_names()
        .iter()
        .chain(f.sealed_names())
        .any(|n| *n == secret.name)
    {
        return Err(Report::error(sentence!(
            "Environment {} doesn't ask for {}.",
            Name::new("environment", &f.name),
            Name::new("secret", &secret.name)
        ))
        .why(prose!(
            "Its file names the secrets it's given under [secrets], each by its name."
        ))
        .exit(Exit::Usage));
    }
    let kept = |id: &str| yours.iter().any(|y| y.id == id);
    let file = approvals::path()?;
    approvals::add_at(
        &file,
        &kept,
        Approval {
            spec: path.clone(),
            name: f.name.clone(),
            resource: format!("secret:{}", secret.id),
            asks: secret.name.clone(),
        },
    )
    .within("keeping the approval")?;
    Ok(Done::success(sentence!(
        "Environment {} at {} is given {} ({}) from its next run.",
        Name::new("environment", &f.name),
        Shown::new(&path),
        Name::new("secret", &secret.name),
        Value::from(format!("secret:{}", secret.id))
    ))
    .next(cmd!("lnk env approval list"), "see what you've approved"))
}

/// `lnk env approval remove <secret> --spec <file>`.
fn remove(secret: &str, file: &std::path::Path) -> Result<Done, Report> {
    let (path, read) = spec(file)?;
    let yours = yours()?;
    let kept = |id: &str| yours.iter().any(|y| y.id == id);
    let file = approvals::path()?;
    let approved = approvals::read_at(&file, &kept).within("reading the approvals")?;
    let named = approvals::find(&yours, secret)
        .map_err(|err| refused(sentence!("That isn't a secret of yours."), err))?;
    let removed = approvals::remove_at(&file, &kept, &path, &read.file.name, &named.id)
        .within("removing the approval")?;
    let shown = Name::new("environment", &read.file.name);
    if !removed {
        let had = approved.iter().any(|a| a.is_for(&path, &read.file.name));
        return Err(Report::error(sentence!(
            "Environment {} isn't given {}.",
            shown,
            Name::new("secret", &named.name)
        ))
        .why(match had {
            true => prose!("You approved other secrets for it: lnk env approval list lists them."),
            false => prose!("You've approved no secret for it."),
        })
        .exit(Exit::Usage));
    }
    Ok(Done::success(sentence!(
        "Environment {} isn't given {} from its next run.",
        shown,
        Name::new("secret", &named.name)
    )))
}

/// An approval as `--json` writes it: the secret's ID under `resource`.
fn json(a: &Approval) -> serde_json::Value {
    serde_json::json!({
        "spec": a.spec,
        "name": a.name,
        "resource": a.resource,
        "asks": a.asks,
    })
}

/// `lnk env approval list [--spec <file>]`.
fn list(out: &mut Out, list: &cli::ApprovalListArgs) -> Result<Done, Report> {
    let yours = yours()?;
    let kept = |id: &str| yours.iter().any(|y| y.id == id);
    let mut approved =
        approvals::read_at(&approvals::path()?, &kept).within("reading the approvals")?;
    if let Some(spec) = &list.spec {
        let path = approvals::spec_path(spec);
        approved.retain(|a| a.spec == path);
    }
    link_actions::answer::reply::<link_env_contract::declared::Approvals>(
        out,
        &serde_json::json!({
            "approvals": approved.iter().map(json).collect::<Vec<_>>(),
        }),
    )?;
    let mut table = Table::new(
        ["Environment", "File", "Secret", "ID"],
        sentence!("You've approved no secret for an environment."),
    );
    for a in &approved {
        table.row([
            Value::from(Name::new("environment", &a.name)),
            Value::from(Shown::new(&a.spec)),
            Value::from(Name::new("secret", &a.asks)),
            Value::from(a.resource.clone()),
        ]);
    }
    out.block(table);
    Ok(Done::shown())
}

/// `lnk env approval show <secret> --spec <file>`.
fn show(out: &mut Out, show: &cli::ApprovalShowArgs) -> Result<Done, Report> {
    let (path, read) = spec(&show.spec)?;
    let yours = yours()?;
    let named = approvals::find(&yours, &show.secret)
        .map_err(|err| refused(sentence!("That isn't a secret of yours."), err))?;
    let kept = |id: &str| yours.iter().any(|y| y.id == id);
    let approved =
        approvals::read_at(&approvals::path()?, &kept).within("reading the approvals")?;
    let found = approved
        .iter()
        .find(|a| a.is_for(&path, &read.file.name) && a.secret_id() == named.id);
    link_actions::answer::reply::<link_env_contract::declared::Found>(
        out,
        &serde_json::json!({ "approval": found.map(json) }),
    )?;
    let shown = Name::new("environment", &read.file.name);
    Ok(match found {
        Some(_) => Done::success(sentence!(
            "Environment {} is given {} at its runs.",
            shown,
            Name::new("secret", &named.name)
        )),
        None => Done::success(sentence!(
            "Environment {} isn't given {}: a run asks you first.",
            shown,
            Name::new("secret", &named.name)
        ))
        .next(
            cmd!(
                "lnk env approval add {} --spec {}",
                Name::new("secret", &named.name),
                Shown::new(&path)
            ),
            "give it ahead",
        ),
    })
}
