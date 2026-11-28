//! The Policy Spec: a principal's grants and its `[limits]`; and a
//! resource's `[access]`, who outside its account may do what to it.

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};

use crate::name::{AccountId, Action, Actions, Kind, PrincipalId, Resources};

/// The version a policy Link makes starts at. A policy's `version` is its
/// author's to bump as they change it; its shape's version is its
/// `schema`, the one counter Link reads.
pub const VERSION: u32 = 1;

/// The most grants a policy holds: a policy is a page a person reads.
pub const MAX_GRANTS: usize = 64;

/// A Policy Spec, its grants and its limits, generated from the package's
/// declarations (`types.toml`). What its shape can't hold is
/// [`Policy::check`], the spec's declared check (`checks.rs`).
pub use crate::declared::{ActionPattern, Grant, Limits, Policy, ResourcePattern};

/// What a principal would have, once a box is made: the boxes it then
/// has, and the new one's size and life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub boxes: u32,
    pub cpus: u32,
    pub memory_gb: u32,
    pub disk_gb: u32,
    pub hours: u32,
}

/// A limit a usage would pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exceeded {
    pub limit: &'static str,
    pub most: u32,
    pub asked: u32,
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} {} is past the policy's limit, {} = {}",
            self.asked, self.limit, self.limit, self.most
        )
    }
}

impl std::error::Error for Exceeded {}

impl Limits {
    pub fn is_none(&self) -> bool {
        *self == Limits::default()
    }

    /// Whether `usage` is within every limit, or the first it passes.
    pub fn admits(&self, usage: &Usage) -> Result<(), Exceeded> {
        let each = [
            ("boxes", self.boxes, usage.boxes),
            ("cpus", self.cpus, usage.cpus),
            ("memory_gb", self.memory_gb, usage.memory_gb),
            ("disk_gb", self.disk_gb, usage.disk_gb),
            ("hours", self.hours, usage.hours),
        ];
        for (limit, most, asked) in each {
            if let Some(most) = most.filter(|most| asked > *most) {
                return Err(Exceeded { limit, most, asked });
            }
        }
        Ok(())
    }
}

/// `text` as a Policy Spec, strictly: no unknown key, no empty table,
/// and every grant one that can reach something.
pub fn parse(text: &str) -> anyhow::Result<Policy> {
    let policy: Policy = link_spec::parse_spec(text)?;
    policy.check()?;
    Ok(policy)
}

impl Policy {
    /// A policy named `name` that grants nothing: a principal whose
    /// grants are its roles' alone.
    pub fn none(name: &str) -> Policy {
        Policy {
            name: name.into(),
            version: VERSION,
            allow: Vec::new(),
            limits: None,
        }
    }

    /// An access token's `scope` as a policy named `name`: each grant,
    /// `<action>` (on every resource of the account) or
    /// `<action>@<resource>`, as written; one this list doesn't hold (a
    /// newer kind, a verb it hasn't) skipped, never refused, so an older
    /// checker takes a token with a newer grant for the ones it knows.
    pub fn from_scope(name: &str, scope: &str) -> Policy {
        let grants = scope
            .split(' ')
            .filter(|g| !g.is_empty())
            .filter_map(|grant| {
                let (actions, on) = grant.split_once('@').unwrap_or((grant, "*"));
                let actions: Actions = actions.parse().ok()?;
                let on: Resources = on.parse().ok()?;
                // Of one kind, as a written grant is.
                let fits = match (actions.kind(), on.kind()) {
                    (Some(of), Some(kind)) => of == kind,
                    _ => true,
                };
                fits.then(|| Grant::new(&[actions], &[on]))
            })
            .take(MAX_GRANTS)
            .collect();
        Policy {
            allow: grants,
            ..Policy::none(name)
        }
    }

    /// Whether `usage` is within every limit it sets, or the first it
    /// passes: none set, none passed.
    pub fn admits(&self, usage: &Usage) -> Result<(), Exceeded> {
        self.limits
            .as_ref()
            .map_or(Ok(()), |limits| limits.admits(usage))
    }

    /// The first grant, from 1, that allows `action` on every resource
    /// `on` reaches, for a principal of `own` account.
    pub fn grant_for_all(&self, action: Action, on: &Resources, own: &AccountId) -> Option<usize> {
        self.allow
            .iter()
            .position(|grant| {
                grant.allowed().iter().any(|a| a.covers(action))
                    && grant.reached().iter().any(|r| r.reaches_all(on, own))
            })
            .map(|n| n + 1)
    }

    /// Its grants as an access token's `scope` holds them: each action
    /// on each resource, `<action>@<resource>`, an action on `*` alone,
    /// separated by spaces, each once.
    pub fn scope(&self) -> String {
        let mut said = Vec::new();
        for grant in &self.allow {
            for actions in grant.allowed() {
                for on in grant.reached() {
                    let one = match on {
                        Resources::All => actions.to_string(),
                        on => format!("{actions}@{on}"),
                    };
                    if !said.contains(&one) {
                        said.push(one);
                    }
                }
            }
        }
        said.join(" ")
    }

    /// What its type alone can't hold: a name, and grants whose actions
    /// and resources are of one kind. Its `version` is its author's.
    pub fn check(&self) -> anyhow::Result<()> {
        ensure!(
            !self.name.is_empty() && self.name.len() <= 64,
            "a policy's name is 1 to 64 characters"
        );
        ensure!(
            self.allow.len() <= MAX_GRANTS,
            "a policy holds at most {MAX_GRANTS} grants, not {}",
            self.allow.len()
        );
        for (n, grant) in self.allow.iter().enumerate() {
            let n = n + 1;
            ensure!(!grant.actions.is_empty(), "grant {n} names no actions");
            ensure!(!grant.on.is_empty(), "grant {n} is on nothing");
            let actions = grant
                .actions
                .iter()
                .map(|a| a.0.parse::<Actions>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("grant {n}: {e}"))?;
            let on = grant
                .on
                .iter()
                .map(|r| r.0.parse::<Resources>())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("grant {n}: {e}"))?;
            for actions in &actions {
                for on in &on {
                    if let (Some(of), Some(kind)) = (actions.kind(), on.kind())
                        && of != kind
                    {
                        bail!(
                            "grant {n}: {actions} is a {}'s, so never on {on}",
                            of.as_str()
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// The first grant, from 1, that allows `action` on resources `on`
    /// reaches, for a principal of `own` account.
    pub fn grant_for(
        &self,
        action: Action,
        name: &crate::ResourceName,
        own: &AccountId,
    ) -> Option<usize> {
        self.allow
            .iter()
            .position(|grant| {
                grant.allowed().iter().any(|a| a.covers(action))
                    && grant.reached().iter().any(|r| r.covers(name, own))
            })
            .map(|n| n + 1)
    }
}

impl Grant {
    /// A grant of `actions` on `on`, each written as a file writes it.
    pub fn new(actions: &[Actions], on: &[Resources]) -> Grant {
        Grant {
            actions: actions
                .iter()
                .map(|a| ActionPattern(a.to_string()))
                .collect(),
            on: on.iter().map(|r| ResourcePattern(r.to_string())).collect(),
        }
    }

    /// The actions it allows, each read; one that doesn't read, which a
    /// checked policy never holds, allows nothing.
    pub fn allowed(&self) -> Vec<Actions> {
        self.actions
            .iter()
            .filter_map(ActionPattern::parsed)
            .collect()
    }

    /// The resources it allows them on, each read; one that doesn't read,
    /// which a checked policy never holds, reaches nothing.
    pub fn reached(&self) -> Vec<Resources> {
        self.on.iter().filter_map(ResourcePattern::parsed).collect()
    }
}

impl ActionPattern {
    /// The actions it allows, read; none where it doesn't read, which a
    /// checked one always does.
    pub fn parsed(&self) -> Option<Actions> {
        self.0.parse().ok()
    }
}

impl fmt::Display for ActionPattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ResourcePattern {
    /// The resources it reaches, read; none where it doesn't read, which
    /// a checked one always does.
    pub fn parsed(&self) -> Option<Resources> {
        self.0.parse().ok()
    }
}

impl fmt::Display for ResourcePattern {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who outside a resource's account may do what to it, the `[access]`
/// every resource's spec shares: a verb of its kind to a list of who.
/// Inside the account, the principal's policy alone decides.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Access(pub BTreeMap<String, Vec<Who>>);

/// One of an `[access]` list: anyone (`*`), every principal of an
/// account, or one principal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Who {
    Anyone,
    Account(AccountId),
    Principal(PrincipalId),
}

impl TryFrom<String> for Who {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        if text == "*" {
            Ok(Who::Anyone)
        } else if text.starts_with("acct-") {
            text.parse().map(Who::Account)
        } else {
            text.parse()
                .map(Who::Principal)
                .map_err(|e| format!("{e}; or an account, acct-<id>; or *, anyone"))
        }
    }
}

impl From<Who> for String {
    fn from(who: Who) -> String {
        match who {
            Who::Anyone => "*".into(),
            Who::Account(account) => account.to_string(),
            Who::Principal(principal) => principal.to_string(),
        }
    }
}

impl Access {
    /// Its verbs, each one `kind`'s.
    pub fn check(&self, kind: Kind) -> anyhow::Result<()> {
        for verb in self.0.keys() {
            ensure!(
                Action::of(kind, verb).is_some(),
                "[access] {verb} isn't a verb of a {}: its verbs are {}",
                kind.as_str(),
                kind.verbs().join(", ")
            );
        }
        Ok(())
    }

    /// Whether it lets `principal`, of `account`, do `action`.
    pub fn lets(&self, action: Action, principal: &PrincipalId, account: &AccountId) -> bool {
        self.0.get(action.verb()).is_some_and(|who| {
            who.iter().any(|w| match w {
                Who::Anyone => true,
                Who::Account(one) => one == account,
                Who::Principal(one) => one == principal,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_policys_grants_are_a_tokens_scope() {
        let policy = crate::parse(
            "schema = 1\nname = \"scout\"\nversion = 1\n\n[[allow]]\nactions = [\"team.member\", \"team.read\"]\non = [\"team:r7\"]\n\n[[allow]]\nactions = [\"agent.*\"]\non = [\"*\", \"agent:k3m4n5p6q7r8\"]\n\n[[allow]]\nactions = [\"team.member\"]\non = [\"team:r7\"]",
        )
        .unwrap();
        assert_eq!(
            policy.scope(),
            "team.member@team:r7 team.read@team:r7 agent.* agent.*@agent:k3m4n5p6q7r8"
        );
        assert_eq!(super::Policy::none("x").scope(), "");
    }

    #[test]
    fn a_tokens_scope_is_read_as_a_policy_skipping_what_it_doesnt_know() {
        let read = super::Policy::from_scope(
            "token",
            "bucket.write@bucket:home-a1/* tunnel.open@tunnel:* future.thing bucket.read@box:b1 agent.*",
        );
        assert_eq!(read.scope(), "bucket.write@bucket:home-a1/* agent.*");
        read.check().unwrap();
        assert!(super::Policy::from_scope("token", "").allow.is_empty());
    }
}
