//! Environment wire requests and process client helpers.
use crate::file::Secrets;
use crate::stage::Shipped;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
/// What `lnk env here` is asked, on its input's first line, once it has
/// said what to seal to on its output ([`main`]).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// The environment's name: its files are in [`files_dir`]. A run with
    /// none has its own name here.
    pub name: String,
    /// The file, in that folder; none for a run with no environment, on
    /// the machine as it is.
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// An explicit private working directory beneath this machine's home.
    /// Absent preserves the environment's existing working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<PathBuf>,
    pub action: Action,
    /// Its secrets and sealed secrets ([`Given`](crate::values::Given)), sealed to the key this
    /// run of `lnk env here` made for it and announced ([`about`]): a
    /// computer or a log the stream passes through reads none of them.
    pub given: link_secretive::Sealed<link_secretive::Secret<Vec<u8>>>,
    /// What each line saying how it went starts with, so a command's own
    /// output can't pass for one.
    pub mark: String,
    /// Its stages and services run directly, in no sandbox, as the
    /// machine's user: what `--no-sandbox` asks, refused where the
    /// machine's policy forbids it. A sealed secret, which a sandbox's
    /// proxy holds, is never given to one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub direct: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "kebab-case")]
pub enum Action {
    /// The stages saved in an image.
    Image,
    /// The same stages, run to set this machine up with no image saved
    /// (`lnk env setup`): on a computer, only those in a sandbox.
    Setup,
    /// The stages run at every start, then the services, as background
    /// services that outlive this.
    Start,
    /// The stages run at every start, the services it needs, then the
    /// stages given, in order, the first that fails ending it with its
    /// exit code (124 at its timeout); the services stopped at its end.
    Run {
        /// Its stages, each sandbox spec as text.
        stages: Vec<Shipped>,
        /// The variables it adds to the environment's.
        #[serde(default)]
        variables: BTreeMap<String, String>,
        /// The secrets its stages are given, by name, its environment's
        /// among them: their values are in what the request was given.
        #[serde(default)]
        secrets: Secrets,
        /// A folder made new for its stages to leave files in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        out: Option<Out>,
        /// Sampled as a job's profile is, when it's given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<crate::profile::Profiling>,
    },
    /// A service's reset.
    Reset { service: String },
    /// The services a start started as background services, stopped.
    Stop,
}

/// The folder a [`Action::Run`]'s stages leave files in: made new, written
/// besides what each stage's sandbox lets it write, its path given to them
/// as `variable`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Out {
    /// From the home folder, without `..`.
    pub folder: PathBuf,
    /// A variable Link sets (`LNK_...`).
    pub variable: String,
}

/// Where an environment's files are on a machine: in Link's settings,
/// which no sandbox reads or writes.
pub fn files_dir(name: &str) -> PathBuf {
    link_files::layout::Hidden::Envs
        .path()
        .join("envs")
        .join(name)
}

/// The same, from the home folder, as `lnk box copy` is given it.
pub fn files_dir_from_home(name: &str) -> String {
    link_files::layout::Hidden::Envs
        .relative_to_home()
        .join("envs")
        .join(name)
        .display()
        .to_string()
}

/// A bare job's private workspace, from the selected machine's home.
pub fn bare_workdir(name: &str) -> PathBuf {
    link_files::layout::Hidden::Jobs
        .relative_to_home()
        .join("work")
        .join(name)
}

/// A service's launchd label and systemd unit: its environment's and
/// its own name.
pub fn service_names(env: &str, service: &str) -> (String, String) {
    (
        format!("link.env.{env}.{service}"),
        format!("lnk-env-{env}-{service}.service"),
    )
}

/// What a request's secrets for environment `name` are sealed as.
pub fn about(name: &str) -> link_secretive::About {
    link_secretive::About {
        owner: format!("env:{name}"),
        kind: "given".into(),
        version: 1,
    }
}

/// The privilege helper explicitly configured for a test, or the host's sudo.
pub fn sudo() -> PathBuf {
    link_secretive::test_var("LNK_SUDO")
        .map(PathBuf::from)
        .unwrap_or_else(|| "sudo".into())
}
