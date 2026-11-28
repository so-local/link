//! `lnk-cloud`: the cloud module's commands. `lnk` runs it for `lnk cloud
//! ...`.

use link_format::WithinExt;
use std::process::Stdio;

use link_cloud::cli::{self, Command};
use link_cloud::{Accounts, adapter_found, check_account, listed};
use link_cloud_contract::{self as cloud, Account};
use link_format::clean;
use link_format::{
    Done, Exit, Fields, Foreign, Name, Out, Path, Report, Sentence, Table, Value, cmd, prose,
    sentence,
};
use link_secretive::Duplicate;

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::saved::own(&[link_cloud::saved, link_cloud_contract::prices::saved]);
    link_core::describe_component!();
    link_settings::output::run(try_main)
}

fn try_main(out: &mut Out) -> Result<Done, Report> {
    match cli::parse().command.command {
        Command::CloudAccountInputSchema(_) => {
            out.data(
                &serde_json::to_vec(&cloud::account_input::Schema { version: 1 })
                    .expect("version serializes"),
            );
            Ok(Done::shown())
        }
        Command::CloudAccountInput(_) => {
            import_account(out).map_err(|_| Report::error(sentence!("Couldn't accept the connected account input."))
                .why(prose!("The versioned input, its account identity and writable private storage must be valid; an existing account is never replaced.")))
        }
        Command::Uninstall(given) => {
            use link_core::uninstall as u;
            let kept = u::Kept {
                parts: vec![u::Part {
                    name: "clouds".into(),
                    about: "your cloud accounts' keys, and Link's SSH key to your boxes (the boxes keep running, and billing)".into(),
                    paths: vec![u::settings(link_files::layout::Hidden::Clouds)?],
                    keys: vec!["cloud/".into()],
                    credentials: Accounts::load_without_keychain()
                        .map(|accounts| accounts.held())
                        .unwrap_or_default(),
                    ..u::Part::default()
                }],
                ..u::Kept::default()
            };
            u::run(&given, &kept, || Ok(()), |_| Ok(()))?;
            Ok(Done::shown())
        }
        Command::CloudConnect(args) => connect_account(out, args),
        Command::CloudList(_) => list(out),
        Command::CloudShow(args) => show(out, &args.name),
        Command::CloudDisconnect(args) => disconnect(out, &args.name),
        Command::CloudEnv(args) => env(out, &args.name, args.json),
    }
}

/// A caller-controlled stdin boundary; no input or underlying parse error is
/// printed because even malformed account data can contain credentials.
fn import_account(out: &mut Out) -> anyhow::Result<Done> {
    let input: cloud::account_input::Input = serde_json::from_reader(std::io::stdin().lock())?;
    anyhow::ensure!(input.version == 1, "unsupported input version");
    check_account(&input.account.name, &input.account.kind, adapter_found)?;
    cloud::check_kind(&input.account.kind)?;
    anyhow::ensure!(!input.account.place.is_empty(), "missing place");
    let mut accounts = Accounts::load()?;
    anyhow::ensure!(
        accounts.get(&input.account.name).is_none(),
        "account already exists"
    );
    let accepted = cloud::account_input::Accepted {
        version: 1,
        name: input.account.name.clone(),
        kind: input.account.kind.clone(),
    };
    accounts.put(input.account);
    accounts.save()?;
    out.data(&serde_json::to_vec(&accepted)?);
    Ok(Done::shown())
}

/// An error a person caused, as a report: what it says, and where to look.
fn refused(err: anyhow::Error) -> Report {
    Report::error(Sentence::adopt(&format!("{err:#}")))
        .next(
            cmd!("lnk cloud connect --help"),
            "see what a cloud account takes",
        )
        .exit(Exit::Usage)
}

/// An account's name.
fn account_name(name: &str) -> Name {
    Name::new("account", clean(name))
}

/// What an account says it is: its identity, else its login.
fn who(account: &Account) -> &str {
    match account.identity.is_empty() {
        true => &account.login,
        false => &account.identity,
    }
}

/// `lnk cloud connect`: the cloud's adapter signs in on this terminal and
/// describes the account, which is kept.
fn connect_account(out: &mut Out, args: cli::CloudConnectArgs) -> Result<Done, Report> {
    let cli::CloudConnectArgs {
        kind,
        name,
        region,
        profile,
        key_id,
        project,
        zone,
        credential_file,
        machine,
        setting,
        quiet,
    } = args;
    let name = name.unwrap_or_else(|| kind.clone());
    check_account(&name, &kind, adapter_found).map_err(refused)?;
    let mut args = vec!["connect".to_string(), "--name".into(), name.clone()];
    for (flag, value) in [
        ("--region", region),
        ("--profile", profile),
        ("--key-id", key_id),
        ("--project", project),
        ("--zone", zone),
        (
            "--credential-file",
            credential_file.map(|f| f.to_string_lossy().into_owned()),
        ),
    ] {
        if let Some(value) = value {
            args.extend([flag.to_string(), value]);
        }
    }
    if machine {
        args.push("--machine".into());
    }
    for setting in setting {
        check_setting(&setting)?;
        args.extend(["--setting".to_string(), setting]);
    }
    // The adapter signs in on this terminal.
    let _ = out.hand_over();
    let account = connect(&kind, &args)?;
    if account.name != name || account.kind != kind {
        return Err(Report::error(sentence!(
            "The adapter for {} described another account, {} on {}.",
            Name::new("cloud", clean(&kind)),
            account_name(&account.name),
            Name::new("cloud", clean(&account.kind))
        )));
    }
    let mut accounts = Accounts::load()?;
    let again = accounts.get(&name).is_some();
    accounts.put(account.duplicate());
    accounts.save()?;
    let (named, identity, place) = (
        account_name(&name),
        Foreign::new(who(&account)),
        Foreign::new(&account.place),
    );
    let said = match again {
        true => sentence!("Reconnected {}: {}, {}.", named.clone(), identity, place),
        false => sentence!("Connected {}: {}, {}.", named.clone(), identity, place),
    };
    let done = Done::success(said).changed("Kept in", Path::new(Accounts::path()?));
    if quiet {
        return Ok(done);
    }
    let kind = clean(&account.kind);
    let buckets = match name == account.kind {
        true => cmd!("lnk storage create {}", kind.as_str()),
        false => cmd!(
            "lnk storage create {} --cloud {}",
            kind.as_str(),
            named.clone()
        ),
    };
    Ok(done
        .next(buckets, "make buckets there")
        .next(cmd!("lnk box create {}", named), "make a machine there")
        .next(cmd!("lnk cloud list"), "list your accounts"))
}

/// `lnk cloud disconnect`: an account forgotten, its key too.
fn disconnect(out: &mut Out, name: &str) -> Result<Done, Report> {
    let mut accounts = Accounts::load()?;
    let Some(account) = accounts.remove(name) else {
        return Err(Report::error(sentence!(
            "No cloud account is named {}.",
            account_name(name)
        ))
        .next(cmd!("lnk cloud list"), "see them")
        .exit(Exit::Usage));
    };
    accounts.save()?;
    // What the adapter kept for it (a credential file) goes too.
    let forget = cloud::Request {
        account: account.duplicate(),
        ..cloud::Request::default()
    };
    if let Err(err) = cloud::call(&account.kind, "disconnect", &forget) {
        out.block(cloud::warned(
            sentence!(
                "What the adapter for {} kept for the account is still there.",
                Name::new("cloud", clean(&account.kind))
            ),
            err,
        ));
    }
    if account.env.keys().any(|k| k.contains("SECRET")) {
        out.block(Report::warning(sentence!(
            "The access key still works until you delete it in the cloud's console."
        )));
    }
    let named = account_name(name);
    Ok(Done::success(sentence!(
        "Forgot {} and its key, and nothing in the cloud was deleted.",
        named.clone()
    ))
    .next(
        cmd!("lnk storage disconnect {}", named.clone()),
        "forget its buckets",
    )
    .next(
        cmd!(
            "lnk cloud connect {}",
            Name::new("cloud", clean(&account.kind))
        ),
        "connect it again; its buckets and boxes wait until you do",
    ))
}

/// `lnk cloud env <name> --json`: an account with its keys, for the
/// module that ran it.
fn env(out: &mut Out, name: &str, json: bool) -> Result<Done, Report> {
    if !json {
        return Err(Report::error(sentence!(
            "The command lnk cloud env is for other lnk commands."
        ))
        .why(prose!("Pass --json, or list your accounts instead."))
        .next(cmd!("lnk cloud list"), "see your accounts")
        .exit(Exit::Usage));
    }
    let accounts = Accounts::discover()?;
    let Some(account) = accounts.get(name) else {
        return Err(Report::error(sentence!(
            "No cloud account is named {}.",
            account_name(name)
        ))
        .next(cmd!("lnk cloud connect aws"), "connect an AWS account")
        .next(
            cmd!("lnk cloud connect gcp"),
            "connect a Google Cloud account",
        )
        .exit(Exit::Usage));
    };
    // The account's keys: for the module that ran it, never a screen.
    out.reply_private(account).map_err(|report| {
        report.next(cmd!("lnk cloud list"), "see the account without its keys")
    })?;
    Ok(Done::shown())
}

/// A `--setting`: `KEY=VALUE`, its key lowercase letters, digits and
/// underscores, its value on one line.
fn check_setting(setting: &str) -> Result<(), Report> {
    let plain = setting.split_once('=').is_some_and(|(key, value)| {
        !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            && !value.contains(['\n', '\r'])
    });
    match plain {
        true => Ok(()),
        false => Err(Report::error(sentence!(
            "The setting {} isn't KEY=VALUE.",
            Foreign::new(setting)
        ))
        .why(prose!(
            "Its key is lowercase letters, digits and underscores."
        ))
        .next(cmd!("lnk cloud connect --help"), "see the flags")
        .exit(Exit::Usage)),
    }
}

/// `lnk-<kind> connect`: signs in on this terminal, and describes the
/// account.
fn connect(kind: &str, args: &[String]) -> Result<Account, Report> {
    let adapter = cloud::adapter(kind)?;
    let output = link_secretive::process::ours(&adapter)
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .within(format!("running {}", adapter.display()))?;
    if !output.status.success() {
        return Err(Report::error(sentence!(
            "Connecting {} didn't finish.",
            Name::new("cloud", clean(kind))
        ))
        .why(prose!("The reason is above."))
        .next(
            cmd!("lnk cloud connect {}", Name::new("cloud", clean(kind))),
            "try again",
        ));
    }
    Ok(serde_json::from_slice(&output.stdout).within(format!(
        "reading the account the adapter for {kind} described"
    ))?)
}

/// `lnk cloud show <name>`: one account, without its credential.
fn show(out: &mut Out, name: &str) -> Result<Done, Report> {
    let accounts = Accounts::discover()?;
    let Some(account) = accounts.get(name) else {
        return Err(Report::error(sentence!(
            "No cloud account is named {}.",
            account_name(name)
        ))
        .next(cmd!("lnk cloud list"), "see them")
        .next(cmd!("lnk cloud connect aws"), "connect an AWS account")
        .exit(Exit::Usage));
    };
    link_actions::answer::reply::<cli::Listed>(out, &listed(account))?;
    out.block(
        Fields::new()
            .key("Kind", Name::new("cloud", clean(&account.kind)))
            .row("Place", Foreign::new(&account.place))
            .row("Identity", Foreign::new(who(account))),
    );
    Ok(Done::shown()
        .next(cmd!("lnk cloud list"), "list your accounts")
        .next(
            cmd!("lnk cloud disconnect {}", account_name(name)),
            "forget it",
        ))
}

/// `lnk cloud list`: the accounts connected.
fn list(out: &mut Out) -> Result<Done, Report> {
    let accounts = Accounts::discover()?;
    let listed: Vec<_> = accounts.accounts.iter().map(listed).collect();
    link_actions::answer::reply::<cli::Accounts>(out, &listed)?;
    let mut table = Table::new(
        ["Name", "Kind", "Place", "Identity"],
        sentence!("No cloud accounts are connected yet."),
    );
    for account in &accounts.accounts {
        table.row([
            Value::from(account_name(&account.name)),
            Value::from(Name::new("cloud", clean(&account.kind))),
            Value::from(Foreign::new(&account.place)),
            Value::from(Foreign::new(who(account))),
        ]);
    }
    out.block(table);
    Ok(match accounts.accounts.is_empty() {
        true => Done::shown()
            .next(cmd!("lnk cloud connect aws"), "connect an AWS account")
            .next(
                cmd!("lnk cloud connect gcp"),
                "connect a Google Cloud account",
            ),
        false => Done::shown()
            .next(cmd!("lnk cloud show <name>"), "show one")
            .next(cmd!("lnk cloud connect aws"), "connect another"),
    })
}
