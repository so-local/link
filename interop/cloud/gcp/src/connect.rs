//! `lnk-gcp connect`: a Google Cloud account that gcloud, rclone and
//! Google's libraries all act as. It is one credential file, a service
//! account's key (`--credential-file`) or your own login's application
//! default credentials (`gcloud auth application-default login`, once),
//! kept in Link's settings (`gcp/credentials/<name>.json`, only yours), so
//! every part of Link using the account is the same identity. Or it is
//! this Compute Engine machine's own service account (`--machine`), which
//! each of them gets from the machine's metadata server, and no file at
//! all. Questions and gcloud's own output go to stderr: stdout is the
//! account, as JSON.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, bail, ensure};
use link_cloud_contract::{Account, Reply};
use link_format::{
    Exit, Foreign, Note, Out, Progress, Question, Report, WithinExt, cmd, phrase, prose, sentence,
};
use link_gcp::flags::GcpConnectArgs as Connect;
use link_secretive::Duplicate;
use link_secretive::Secret;

use crate::metadata::{self, Metadata, Reading};

/// The variable naming an account's credential file, for rclone and
/// Google's libraries.
pub const CREDENTIALS: &str = "GOOGLE_APPLICATION_CREDENTIALS";

/// gcloud's variable for acting as the saved credential file instead of
/// its independently configured login, for service and personal accounts.
pub(crate) const OVERRIDE: &str = "CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE";

/// Who an account acts as, and where its credentials are.
enum Who {
    /// A service account, by its email, with its key's project and the
    /// key itself.
    Service {
        email: String,
        project: Option<String>,
        key: Secret,
    },
    /// A person's login (application default credentials), by gcloud's
    /// account when it's known.
    Person {
        account: Option<String>,
        credentials: Secret,
    },
    /// This machine's attached service account, by its email, with the
    /// machine's project and zone where the flags don't name them. Its
    /// credentials stay with the metadata server.
    Machine {
        email: String,
        project: Option<String>,
        zone: Option<String>,
    },
}

/// Where a connected account's credentials come from, as its settings
/// name it: a credential file ([`CREDENTIALS`]), or this machine's
/// metadata server (`GCE_METADATA_HOST`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Credential {
    File,
    Machine,
}

/// The kind of credential settings name, where `has` says which settings
/// there are; none when they name neither, or both.
pub(crate) fn credential(has: impl Fn(&str) -> bool) -> Option<Credential> {
    match (has(CREDENTIALS), has(&metadata::settings().host_variable)) {
        (true, false) => Some(Credential::File),
        (false, true) => Some(Credential::Machine),
        _ => None,
    }
}

pub fn connect(out: &mut Out, options: Connect) -> Result<Account, Report> {
    let name = options.name.clone();
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(
            Report::error(sentence!("{} can't name an account.", Foreign::new(&name)))
                .why(prose!("Use lowercase letters, digits and dashes."))
                .exit(Exit::Usage),
        );
    }
    let gcloud = match crate::cli::find("gcloud") {
        Some(gcloud) => Some(gcloud),
        None => match crate::install::offer(out) {
            Ok(installed) => installed,
            Err(err) => {
                out.block(link_cloud_contract::warned(
                    sentence!("Gcloud didn't install."),
                    err,
                ));
                None
            }
        },
    };
    let get = |key: &str| gcloud.as_deref().and_then(|g| config_value(g, key));
    let who = match (&options.credential_file, options.machine) {
        (Some(_), true) => {
            return Err(Report::error(sentence!(
                "An account is a credential file or this machine's service account, never both."
            ))
            .next(
                cmd!("lnk cloud connect gcp --machine"),
                "act as this machine",
            )
            .exit(Exit::Usage));
        }
        (Some(file), false) => credential_file(file)?,
        (None, true) => this_machine(&metadata::Server::new()?, &options)?,
        (None, false) => your_login(out, gcloud.as_deref(), &get)?,
    };
    let found = match &who {
        Who::Service { project, .. } | Who::Machine { project, .. } => project.clone(),
        Who::Person { .. } => None,
    };
    let project = match options.project.clone().or(found).or_else(|| get("project")) {
        Some(project) => project,
        None => out
            .ask(
                Question::text(sentence!("What is the ID of your Google Cloud project?"))
                    .flag("--project"),
            )?
            .trim()
            .to_string(),
    };
    if project.is_empty() {
        return Err(Report::error(sentence!("A project is needed."))
            .next(cmd!("lnk cloud connect gcp --project <id>"), "name its ID")
            .exit(Exit::Usage));
    }
    let found = match &who {
        Who::Machine { zone, .. } => zone.clone(),
        Who::Service { .. } | Who::Person { .. } => None,
    };
    let zone = options
        .zone
        .clone()
        .or(found)
        .or_else(|| get("compute/zone"))
        .unwrap_or_else(|| "us-central1-a".into());
    let mut env = BTreeMap::from([
        ("CLOUDSDK_CORE_PROJECT".to_string(), project.clone()),
        ("CLOUDSDK_COMPUTE_ZONE".to_string(), zone.clone()),
    ]);
    let (identity, login) = match &who {
        Who::Service { email, key, .. } => {
            saved(&mut env, save(&name, key)?);
            (
                format!("Google Cloud as {email}"),
                format!("the service account {email} (its key)"),
            )
        }
        Who::Person {
            account: Some(email),
            credentials,
        } => {
            saved(&mut env, save(&name, credentials)?);
            env.insert("CLOUDSDK_CORE_ACCOUNT".into(), email.clone());
            (
                format!("Google Cloud as {email}"),
                format!("gcloud as {email}"),
            )
        }
        Who::Person {
            account: None,
            credentials,
        } => {
            saved(&mut env, save(&name, credentials)?);
            (
                "Google Cloud (a login's credential file)".to_string(),
                "the credential file you gave".to_string(),
            )
        }
        Who::Machine { email, .. } => {
            // A file this name kept before, as another kind, goes.
            remove_kept(&credentials_dir()?.join(format!("{name}.json")))?;
            let settings = metadata::settings();
            env.insert(settings.host_variable.clone(), settings.host.clone());
            env.insert("CLOUDSDK_CORE_ACCOUNT".into(), email.clone());
            (
                format!("Google Cloud as {email}"),
                format!("this machine's service account {email}"),
            )
        }
    };
    // Held as secrets from here: the credentials' path among them.
    let env: BTreeMap<String, Secret> = env.into_iter().map(|(k, v)| (k, v.into())).collect();
    match &gcloud {
        Some(_) => {
            let cli = crate::cli::Cli::new("gcloud", env.duplicate())?;
            cli.run(&["projects", "describe", &project, "--format", "json"])
                .within(format!("checking {project} with Google Cloud"))?;
        }
        None => out.block(Note(sentence!(
            "Boxes need gcloud, which isn't installed: lnk cloud connect gcp in a terminal \
             installs it, or https://cloud.google.com/sdk/docs/install (or brew install --cask \
             google-cloud-sdk). Buckets don't."
        ))),
    }
    Ok(Account {
        name,
        kind: "gcp".into(),
        place: zone,
        identity,
        login,
        region: None,
        profile: None,
        project: Some(project),
        settings: env
            .into_iter()
            .map(|(name, value)| (name, value.into_exposed()))
            .collect(),
        env: BTreeMap::new(),
        machine: None,
    })
}

/// The settings naming a saved credential file, for Google's libraries
/// and for gcloud.
fn saved(env: &mut BTreeMap<String, String>, file: PathBuf) {
    let path = file.display().to_string();
    env.insert(CREDENTIALS.to_string(), path.clone());
    env.insert(OVERRIDE.to_string(), path);
}

/// This machine's attached service account, and its project and zone
/// where the flags don't name them, as its metadata server says.
fn this_machine(metadata: &dyn Metadata, options: &Connect) -> Result<Who, Report> {
    let read = |path: &str| -> Result<Option<String>, Report> {
        match metadata.read(path)? {
            Reading::Found(value) if !value.is_empty() => Ok(Some(value)),
            Reading::Found(_) | Reading::Missing => Ok(None),
            Reading::NoServer => Err(Report::error(sentence!(
                "This machine has no Google metadata server, so it isn't a Compute Engine machine."
            ))
            .why(prose!(
                "Only a Compute Engine machine has its own service account. \
                 Elsewhere, connect with a service account's key or your gcloud login."
            ))
            .next(
                cmd!("lnk cloud connect gcp --credential-file key.json"),
                "act as a service account",
            )
            .exit(Exit::Usage)),
        }
    };
    let settings = metadata::settings();
    let Some(email) = read(&settings.email)? else {
        return Err(
            Report::error(sentence!("This machine has no service account attached."))
                .why(prose!(
                    "Attach one to the machine in Google Cloud, then connect again."
                ))
                .exit(Exit::Usage),
        );
    };
    let project = match &options.project {
        Some(_) => None,
        None => read(&settings.project)?,
    };
    // `projects/<number>/zones/<zone>`: its last part.
    let zone = match &options.zone {
        Some(_) => None,
        None => read(&settings.zone)?
            .and_then(|zone| zone.rsplit('/').next().map(str::to_string))
            .filter(|zone| !zone.is_empty()),
    };
    Ok(Who::Machine {
        email,
        project,
        zone,
    })
}

/// A credential file you gave: a service account's key, or a login's
/// application default credentials.
fn credential_file(file: &Path) -> anyhow::Result<Who> {
    let text = std::fs::read_to_string(file).within(format!("reading {}", file.display()))?;
    let key: serde_json::Value = serde_json::from_str(&text)
        .within(format!("{} isn't a Google credential file", file.display()))?;
    let who = match key["type"].as_str() {
        Some("service_account") => Who::Service {
            email: key["client_email"]
                .as_str()
                .within("the key has no client_email")?
                .to_string(),
            project: key["project_id"].as_str().map(str::to_string),
            key: Secret::new(text),
        },
        Some("authorized_user") => Who::Person {
            account: None,
            credentials: Secret::new(text),
        },
        _ => bail!(
            "{} isn't a service account's key (a JSON file from the Google Cloud console, \
             IAM, Service accounts, Keys)",
            file.display()
        ),
    };
    Ok(who)
}

/// Your own login: gcloud's (signing in if there's none), and its
/// application default credentials, which rclone and Google's libraries
/// act as, made once now.
fn your_login(
    out: &mut Out,
    gcloud: Option<&Path>,
    get: &dyn Fn(&str) -> Option<String>,
) -> anyhow::Result<Who> {
    let Some(gcloud) = gcloud else {
        bail!(
            "signing in to Google Cloud needs gcloud (https://cloud.google.com/sdk/docs/install), \
             or a service account's key: lnk cloud connect gcp --credential-file key.json"
        );
    };
    let mut signed_in = get("account");
    if signed_in.is_none() {
        out.block(Note(sentence!("Sign in to Google Cloud with gcloud.")));
        gcloud_shown(out, gcloud, &["auth", "login"])?;
        signed_in = get("account");
    }
    let who = signed_in.within("gcloud has no account signed in")?;
    ensure!(
        !who.ends_with(".gserviceaccount.com"),
        "gcloud is signed in as the service account {}: Link acts as a service account \
         through its key, which gcloud doesn't hand out: lnk cloud connect gcp --credential-file key.json",
        link_format::clean(&who)
    );
    out.block(Progress::new(phrase!(
        "Signing in {} once more, for your buckets (application default credentials)",
        Foreign::new(&who)
    )));
    gcloud_shown(out, gcloud, &["auth", "application-default", "login"]).within(
        "the sign-in for buckets didn't finish: in a terminal, lnk cloud connect gcp; or \
         a service account's key: lnk cloud connect gcp --credential-file key.json",
    )?;
    let dir = config_value(gcloud, "info").within("gcloud didn't say where its settings are")?;
    let adc = Path::new(&dir).join("application_default_credentials.json");
    let text = std::fs::read_to_string(&adc)
        .within(format!("reading what gcloud saved ({})", adc.display()))?;
    Ok(Who::Person {
        account: Some(who),
        credentials: Secret::new(text),
    })
}

/// `gcloud config get-value <key>`, or with `info`, gcloud's settings
/// folder; None when unset.
fn config_value(gcloud: &Path, key: &str) -> Option<String> {
    let mut command = link_secretive::process::command(gcloud);
    match key {
        "info" => command.args(["info", "--format=value(config.paths.global_config_dir)"]),
        key => command.args(["config", "get-value", key]),
    };
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !value.is_empty() && value != "(unset)").then_some(value)
}

/// Runs gcloud in this terminal, its output on stderr (stdout is the
/// account), a browser left closed where it wouldn't be in front of you.
fn gcloud_shown(out: &mut Out, gcloud: &Path, args: &[&str]) -> anyhow::Result<()> {
    // gcloud writes to the terminal itself.
    let _ = out.hand_over();
    let mut command = link_secretive::process::command(gcloud);
    command.args(args);
    if no_browser() {
        command.arg("--no-launch-browser");
    }
    let status = command
        .stdout(link_format::streams::child_stderr()?)
        .status()
        .within(format!("running gcloud {}", args.join(" ")))?;
    ensure!(status.success(), "gcloud's sign-in didn't finish");
    Ok(())
}

/// The folder of accounts' credential files, only yours.
fn credentials_dir() -> anyhow::Result<PathBuf> {
    Ok(link_files::layout::Hidden::Gcp
        .try_path()?
        .join("credentials"))
}

/// Keeps account `name`'s credential file, replacing one it had.
fn save(name: &str, text: &Secret) -> anyhow::Result<PathBuf> {
    let dir = credentials_dir()?;
    link_files::create_private_dir(&dir)?;
    let file = dir.join(format!("{name}.json"));
    link_files::write_private(&file, text.expose().as_bytes())
        .within(format!("writing {}", file.display()))?;
    Ok(file)
}

/// `storage`: the account's own sign-in for its buckets, as rclone's
/// settings (never saved), and what rclone runs with. A service account's
/// key is the bucket's own setting; a person's login goes by the variable,
/// which is one per rclone run; this machine's service account is the
/// one rclone finds itself, at the metadata server.
pub fn storage(account: &Account) -> anyhow::Result<Reply> {
    let reconnect = || {
        format!(
            "{} has no credential file of Link's (connected before Link kept one): \
             lnk cloud connect gcp --name {} again",
            link_format::clean(&account.name),
            link_format::clean(&account.name)
        )
    };
    let mut settings: BTreeMap<String, Secret> = BTreeMap::from([(
        "type".to_string(),
        "google cloud storage".to_string().into(),
    )]);
    match credential(|name| account.setting(name).is_some()) {
        Some(Credential::Machine) => {
            settings.insert("env_auth".into(), "true".to_string().into());
            return Ok(Reply {
                credential: settings,
                ..Reply::default()
            });
        }
        Some(Credential::File) => {}
        None => anyhow::bail!(reconnect()),
    }
    let file = account.setting(CREDENTIALS).with_context(reconnect)?;
    let text = std::fs::read_to_string(file).with_context(reconnect)?;
    let key: serde_json::Value = serde_json::from_str(&text).with_context(reconnect)?;
    let mut env = BTreeMap::new();
    match key["type"].as_str() {
        Some("service_account") => {
            settings.insert("service_account_file".into(), Secret::new(file.to_string()));
        }
        _ => {
            settings.insert("env_auth".into(), "true".to_string().into());
            env.insert(CREDENTIALS.to_string(), Secret::new(file.to_string()));
        }
    }
    Ok(Reply {
        credential: settings,
        env,
        ..Reply::default()
    })
}

/// `disconnect`: the account's credential file, gone (`lnk cloud
/// disconnect`). This machine's service account has none to remove.
pub fn forget(account: &Account) -> anyhow::Result<()> {
    match (
        credential(|name| account.setting(name).is_some()),
        account.setting(CREDENTIALS),
    ) {
        (Some(Credential::File), Some(file)) => remove_kept(Path::new(file)),
        _ => Ok(()),
    }
}

/// Removes `file` when it's Link's own copy of a credential file, never
/// a file it was pointed at elsewhere; nothing when it's gone already.
fn remove_kept(file: &Path) -> anyhow::Result<()> {
    if file.parent() == Some(credentials_dir()?.as_path()) {
        match std::fs::remove_file(file) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        }
    }
    Ok(())
}

/// Whether a browser opened here would be in front of the user: not over
/// SSH, and on Linux, not without a display.
fn no_browser() -> bool {
    let var = |name| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    var("SSH_CONNECTION")
        || var("SSH_TTY")
        || (cfg!(target_os = "linux") && !var("DISPLAY") && !var("WAYLAND_DISPLAY"))
}

#[cfg(test)]
mod tests;
