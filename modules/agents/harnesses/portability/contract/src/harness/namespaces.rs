//! What each of [`super::NAMESPACES`]' actions reaches, as
//! `namespaces.toml` declares it: one table, which every place that
//! serves the namespaces builds them from, so an agent's tools have one
//! set of names wherever it runs.

use link_sandbox_client::namespace::{Action, Target};
use serde::Deserialize;
use std::collections::BTreeMap;

/// A kind of provider a namespace's action reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Link run: commands, each in a sandbox of its own.
    Run,
    /// Link scheduler: subagents, messages and schedules.
    Schedule,
    /// Link file: files read and written.
    File,
    /// Link code: the project's language servers.
    Code,
    /// The agent's context service, which only a machine runs.
    Context,
}

impl Kind {
    /// Its name, as a namespace's call selects it with `kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Run => "run",
            Kind::Schedule => "schedule",
            Kind::File => "file",
            Kind::Code => "code",
            Kind::Context => "context",
        }
    }
}

/// One provider's tool an action calls.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declared {
    pub kind: Kind,
    pub native: String,
}

/// One action, as declared.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    #[serde(default)]
    pub select: bool,
    #[serde(default)]
    pub broadcast: bool,
    /// The harness's own call, never listed to a model.
    #[serde(default)]
    pub harness: bool,
    pub targets: Vec<Declared>,
    /// Reached only where none of `targets` has a provider.
    #[serde(default)]
    pub otherwise: Vec<Declared>,
}

/// Every namespace's actions, by namespace, then by action.
pub type Namespaces = BTreeMap<String, BTreeMap<String, Declaration>>;

static DECLARED: std::sync::LazyLock<Namespaces> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("../../namespaces.toml")).expect("the harness namespaces.toml")
});

/// The namespaces, as `namespaces.toml` declares them.
pub fn declared() -> &'static Namespaces {
    &DECLARED
}

/// The actions of `namespace` where `provider` names the tool serving
/// each kind there (none: not served), each with the targets it has a
/// provider for. Empty for a namespace with none.
pub fn actions(
    namespace: &str,
    provider: impl Fn(Kind) -> Option<String>,
) -> BTreeMap<String, Action> {
    let reached = |declared: &[Declared]| -> BTreeMap<String, Target> {
        declared
            .iter()
            .filter_map(|target| {
                provider(target.kind).map(|server| {
                    (
                        target.kind.as_str().to_string(),
                        Target {
                            server,
                            tool: target.native.clone(),
                        },
                    )
                })
            })
            .collect()
    };
    let Some(actions) = DECLARED.get(namespace) else {
        return BTreeMap::new();
    };
    actions
        .iter()
        .filter_map(|(name, declaration)| {
            let mut targets = reached(&declaration.targets);
            if targets.is_empty() {
                targets = reached(&declaration.otherwise);
            }
            (!targets.is_empty()).then(|| {
                (
                    name.clone(),
                    Action {
                        select: declaration.select,
                        targets,
                        broadcast: declaration.broadcast,
                        unlisted: declaration.harness,
                    },
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declared_namespaces_are_the_handed_ones() {
        let names: Vec<&str> = declared().keys().map(String::as_str).collect();
        let mut handed = super::super::NAMESPACES.to_vec();
        handed.sort_unstable();
        assert_eq!(names, handed);
        for (namespace, actions) in declared() {
            assert!(!actions.is_empty(), "{namespace} declares no action");
        }
    }

    #[test]
    fn each_action_reaches_only_the_kinds_served() {
        let all = |kind: Kind| Some(format!("{}-one", kind.as_str()));
        let task = actions("task", all);
        assert_eq!(task["run"].targets["run"].tool, "command");
        assert_eq!(task["list"].targets.len(), 2);
        assert!(task["list"].broadcast && task["list"].select);
        assert!(!task["stop"].broadcast && task["stop"].select);
        assert_eq!(task["stop"].targets["schedule"].tool, "cancel");
        // Without run, only the scheduler's half; `kind` kept to select it.
        let scheduled = actions("task", |kind| (kind == Kind::Schedule).then(|| "s".into()));
        assert!(!scheduled.contains_key("run"));
        assert_eq!(scheduled["list"].targets.len(), 1);
        assert!(scheduled["list"].select);
        // The context service sends where it's served; else the scheduler.
        assert_eq!(
            actions("agent", all)["send"]
                .targets
                .keys()
                .collect::<Vec<_>>(),
            ["context"]
        );
        let peer = actions("agent", |kind| (kind == Kind::Schedule).then(|| "s".into()));
        assert_eq!(peer["send"].targets["schedule"].tool, "send");
        assert!(!peer.contains_key("show"));
        // Schedules carry no selector: one provider, its trained arguments.
        assert!(actions("cron", all).values().all(|action| !action.select));
        // Compaction is the harness's own call, in `cron`.
        let compact = &actions("cron", all)["compact"];
        assert!(compact.unlisted);
        assert_eq!(compact.targets["schedule"].tool, "compact");
        assert!(actions("task", all).values().all(|action| !action.unlisted));
        assert!(actions("task", |_| None).is_empty());
        assert!(actions("unknown", all).is_empty());
    }
}
