//! An environment's stages, as every side that runs one has them: the
//! environment file's, a job's, and what a machine is sent to run
//! ([`Run`]), alone or in parallel groups. Their user and timeout are
//! typed here, so each side reads them alike.

use std::time::Duration;

use anyhow::bail;
use link_format::WithinExt;
use serde::{Deserialize, Deserializer, Serialize};

pub use link_sandbox_contract::box_users::User;

/// How long something may run: core's duration (`link-types`), held
/// from a second to a day where it's read ([`check_timeout`]).
pub use link_types::Duration as Timeout;

/// A timeout from a second to a day: what every reader of one checks,
/// naming `what` it limits.
pub fn check_timeout(what: &str, timeout: Option<Timeout>) -> anyhow::Result<()> {
    if let Some(timeout) = timeout {
        let length = timeout.get();
        let whole = length.subsec_nanos() == 0;
        anyhow::ensure!(
            whole && (1..=86_400).contains(&length.as_secs()),
            "{what}'s timeout {timeout}: from a second to a day, as 30s, 30m or 2h"
        );
    }
    Ok(())
}

/// `30s`, `2m`, `1h`: a second to a day.
pub fn duration(text: &str) -> anyhow::Result<Duration> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let unit = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => bail!("{text:?}: say how long as 30s, 30m or 2h"),
    };
    let secs = number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .filter(|s| (1..=86_400).contains(s))
        .within(format!(
            "{text:?}: from a second to a day, as 30s, 30m or 2h"
        ))?;
    Ok(Duration::from_secs(secs))
}

/// A stage as the machine it runs on is sent it: its sandbox spec as
/// text, so nothing is read there from a path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// A caller-owned proxy route. Absence preserves direct execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub name: String,
    /// A command line, for `sh -c`.
    pub run: String,
    /// Its sandbox spec, as text; none for `root`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    /// A protected sandbox here ([`link_sandbox_client::protected`]): the
    /// runner verifies `sandbox` against the file before using it as
    /// grants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected: Option<std::path::PathBuf>,
    /// Who it runs as; none, the machine's own user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// Its own limit, within the whole run's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
}

impl Run {
    pub fn root(&self) -> bool {
        self.user.as_ref().is_some_and(User::is_root)
    }
}

/// The one stage shape, shared by environments, jobs and compositions.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Stage<T> {
    Command(T),
    Parallel(Parallel<T>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parallel<T> {
    /// The group's name, unique among the stages and their members.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// How long each member still running may run, within the whole
    /// run's limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Timeout>,
    /// Its members, started together, each in its own sandbox, its output
    /// lines starting with its name.
    ///
    /// A failing member lets its peers finish, then ends the build or job
    /// with the first failing member's code, and later stages don't run.
    /// A member is never a group itself, and a group has at least one. In
    /// an environment, a group's members share their `when`, and its image
    /// hash counts the grouping, the members' order and the group's
    /// timeout.
    pub parallel: Vec<T>,
}

pub trait Member {
    fn name(&self) -> &str;
    fn root(&self) -> bool;
    fn timeout(&self) -> Option<Timeout>;
    fn saved(&self) -> bool {
        true
    }
}

impl<T> From<T> for Stage<T> {
    fn from(command: T) -> Self {
        Self::Command(command)
    }
}
impl<T: Default> Default for Stage<T> {
    fn default() -> Self {
        Self::Command(T::default())
    }
}
impl<T> Stage<T> {
    pub fn members(&self) -> &[T] {
        match self {
            Self::Command(command) => std::slice::from_ref(command),
            Self::Parallel(group) => &group.parallel,
        }
    }
    pub fn members_mut(&mut self) -> &mut [T] {
        match self {
            Self::Command(command) => std::slice::from_mut(command),
            Self::Parallel(group) => &mut group.parallel,
        }
    }
    pub fn try_map<U>(
        &self,
        mut f: impl FnMut(&T) -> anyhow::Result<U>,
    ) -> anyhow::Result<Stage<U>> {
        Ok(match self {
            Self::Command(command) => Stage::Command(f(command)?),
            Self::Parallel(group) => Stage::Parallel(Parallel {
                name: group.name.clone(),
                timeout: group.timeout,
                parallel: group
                    .parallel
                    .iter()
                    .map(f)
                    .collect::<anyhow::Result<_>>()?,
            }),
        })
    }
}
impl<T: Member> Stage<T> {
    pub fn name(&self) -> &str {
        match self {
            Self::Command(command) => command.name(),
            Self::Parallel(group) => &group.name,
        }
    }
    pub fn root(&self) -> bool {
        self.members().iter().any(Member::root)
    }
    pub fn saved(&self) -> bool {
        self.members().iter().all(Member::saved)
    }
    pub fn timeout(&self) -> Option<Timeout> {
        match self {
            Self::Command(command) => command.timeout(),
            Self::Parallel(group) => group.timeout,
        }
    }
}

/// A stage as it is sent, its parallel groups kept.
pub type Shipped = Stage<Run>;
impl Member for Run {
    fn name(&self) -> &str {
        &self.name
    }
    fn root(&self) -> bool {
        self.root()
    }
    fn timeout(&self) -> Option<Timeout> {
        self.timeout
    }
}

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Stage<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let parsed = if value.get("parallel").is_some() {
            serde_json::from_value(value).map(Self::Parallel)
        } else {
            serde_json::from_value(value).map(Self::Command)
        };
        parsed.map_err(serde::de::Error::custom)
    }
}

/// A stage of any side's: its members, and its group's name and timeout
/// when it's a group.
pub trait Staged {
    type Member: Member;
    fn members(&self) -> &[Self::Member];
    fn group(&self) -> Option<(&str, Option<Timeout>)>;
}

impl<T: Member> Staged for Stage<T> {
    type Member = T;
    fn members(&self) -> &[T] {
        Stage::members(self)
    }
    fn group(&self) -> Option<(&str, Option<Timeout>)> {
        match self {
            Self::Command(_) => None,
            Self::Parallel(group) => Some((&group.name, group.timeout)),
        }
    }
}

impl<T: Member> Member for &T {
    fn name(&self) -> &str {
        T::name(self)
    }
    fn root(&self) -> bool {
        T::root(self)
    }
    fn timeout(&self) -> Option<Timeout> {
        T::timeout(self)
    }
    fn saved(&self) -> bool {
        T::saved(self)
    }
}

/// Checks the groups before any command is started, where stages are sent
/// from and where they run. An unnamed group contributes no name.
pub fn check_groups<S: Staged>(stages: &[S]) -> anyhow::Result<()> {
    use anyhow::ensure;
    let mut names = std::collections::BTreeSet::new();
    for stage in stages {
        if let Some((name, timeout)) = stage.group() {
            link_spec::check_name(name)?;
            ensure!(names.insert(name), "two stages named {name}");
            let members = stage.members();
            ensure!(!members.is_empty(), "parallel stage {name} has no members");
            ensure!(
                members.iter().all(|s| s.saved() == members[0].saved()),
                "a parallel group's members have the same when"
            );
            check_timeout(&format!("parallel stage {name}"), timeout)?;
        }
        for member in stage.members() {
            link_spec::check_name(member.name())?;
            check_timeout(&format!("stage {}", member.name()), member.timeout())?;
            ensure!(
                names.insert(member.name()),
                "two stages named {}",
                member.name()
            );
        }
    }
    Ok(())
}

/// What a run's step was: its setup, a service made ready, or a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Setup,
    Service,
    Stage,
}

/// How a step ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Passed,
    Failed,
    TimedOut,
}

impl Outcome {
    /// As `lnk env here` says it after a stage's seconds.
    pub fn word(self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::TimedOut => "timed-out",
        }
    }
}

/// One step of a run, timed whatever its outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timed {
    pub name: String,
    pub kind: Step,
    pub seconds: f64,
    pub outcome: Outcome,
}

impl Timed {
    pub fn stage(name: impl Into<String>, seconds: f64, outcome: Outcome) -> Timed {
        Timed {
            name: name.into(),
            kind: Step::Stage,
            seconds,
            outcome,
        }
    }
}
