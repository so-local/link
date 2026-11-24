//! Roles: a Policy Spec given to principals, so many share one set of
//! grants and a change to it reaches them all. A role is named by its
//! policy's `name`, which never changes: a renamed role is a new one.
//! Data a service keeps and acts on, as TOML; nothing here reads a file.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::name::PrincipalId;
use crate::policy::Policy;

/// A role's name: its policy's, lowercase letters, digits and `-`,
/// beginning with a letter, at most 64 long (`research`).
pub fn check_name(name: &str) -> anyhow::Result<()> {
    let plain = (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ensure!(
        plain,
        "{:?} isn't a role's name: lowercase letters, digits and -, from a letter",
        name.chars().take(80).collect::<String>()
    );
    Ok(())
}

/// An error for a role's grant on agents, teams or buckets whose `*`
/// doesn't end a segment (`team:r7*`, `bucket:team-r7*`): such ids are
/// made, not chosen, so one beginning as another does is another's, and a
/// role's pattern reaches whole segments alone (`bucket:team-r7/*`).
fn check_patterns(policy: &Policy) -> anyhow::Result<()> {
    use crate::name::Kind;
    for (n, grant) in policy.allow.iter().enumerate() {
        for on in grant.reached() {
            let made = matches!(on.kind(), Some(Kind::Agent | Kind::Team | Kind::Bucket));
            ensure!(
                !made || on.ends_segments(),
                "grant {}: {on} reaches ids that only begin as one does: a role's * is \
                 alone or after a / (bucket:team-r7/*)",
                n + 1
            );
        }
    }
    Ok(())
}

/// Every role, by its name, and who holds each.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roles {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    roles: BTreeMap<String, Policy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    given: BTreeMap<PrincipalId, BTreeSet<String>>,
}

impl Roles {
    /// `policy` kept as the role of its name, replacing the one of that
    /// name: who holds it then holds the new grants.
    pub fn define(&mut self, policy: Policy) -> anyhow::Result<()> {
        check_name(&policy.name)?;
        policy.check()?;
        check_patterns(&policy)?;
        self.roles.insert(policy.name.clone(), policy);
        Ok(())
    }

    /// An error for a role kept that [`Roles::define`] refuses: as a file
    /// is read, so one written before a rule tightened grants nothing.
    pub fn check(&self) -> anyhow::Result<()> {
        for (name, policy) in &self.roles {
            ensure!(
                *name == policy.name,
                "the role {name} holds a policy named {}",
                policy.name
            );
            check_name(name)?;
            policy.check()?;
            check_patterns(policy).with_context(|| format!("the role {name}"))?;
        }
        Ok(())
    }

    /// The role `name`, if there's one.
    pub fn get(&self, name: &str) -> Option<&Policy> {
        self.roles.get(name)
    }

    /// Every role, by name.
    pub fn all(&self) -> impl Iterator<Item = &Policy> {
        self.roles.values()
    }

    /// The role `name` given to `to`. Refused for a role there isn't.
    pub fn give(&mut self, name: &str, to: PrincipalId) -> anyhow::Result<()> {
        if !self.roles.contains_key(name) {
            bail!("there's no role {name}: make it first");
        }
        self.given.entry(to).or_default().insert(name.to_string());
        Ok(())
    }

    /// The role `name` taken back from `from`: whether it held it.
    pub fn take(&mut self, name: &str, from: &PrincipalId) -> bool {
        let Some(held) = self.given.get_mut(from) else {
            return false;
        };
        let had = held.remove(name);
        if held.is_empty() {
            self.given.remove(from);
        }
        had
    }

    /// The role `name` removed, and taken back from everyone holding it:
    /// whether there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        for held in self.given.values_mut() {
            held.remove(name);
        }
        self.given.retain(|_, held| !held.is_empty());
        self.roles.remove(name).is_some()
    }

    /// Every role `who` was given was removed with it: what it held.
    pub fn forget(&mut self, who: &PrincipalId) -> BTreeSet<String> {
        self.given.remove(who).unwrap_or_default()
    }

    /// The roles `who` holds, each its policy, for
    /// [`crate::Principal::with_roles`].
    pub fn of(&self, who: &PrincipalId) -> Vec<Policy> {
        self.given
            .get(who)
            .into_iter()
            .flatten()
            .filter_map(|name| self.roles.get(name).cloned())
            .collect()
    }

    /// Who holds the role `name`.
    pub fn holders<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a PrincipalId> + 'a {
        self.given
            .iter()
            .filter(move |(_, held)| held.contains(name))
            .map(|(who, _)| who)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::{Access, Principal, Resource, allows};

    fn role(text: &str) -> Policy {
        crate::parse(&format!("schema = 1\n{text}")).unwrap()
    }

    fn p(text: &str) -> PrincipalId {
        text.parse().unwrap()
    }

    const RESEARCH: &str = r#"
name = "research"
version = 1

[[allow]]
actions = ["bucket.read", "bucket.write"]
on = ["bucket:team-r7/drafts/*"]
"#;

    #[test]
    fn a_role_grants_what_its_holder_may_do() {
        let mut roles = Roles::default();
        roles.define(role(RESEARCH)).unwrap();
        roles.give("research", p("service:scoutsimjo7n")).unwrap();
        let own = role("name = \"scout\"\nversion = 1");
        let scout = Principal::new(
            p("service:scoutsimjo7n"),
            "acct-a1".parse().unwrap(),
            None,
            own.clone(),
        )
        .with_roles(roles.of(&p("service:scoutsimjo7n")));
        let drafts =
            Resource::new("bucket:team-r7/drafts/".parse().unwrap(), Access::default()).unwrap();
        let said = allows(
            &scout,
            "bucket.write".parse().unwrap(),
            &drafts,
            SystemTime::now(),
        );
        assert_eq!(said.to_string(), "allowed by grant 1 of the role research");
        let other = Resource::new("bucket:team-r7/".parse().unwrap(), Access::default()).unwrap();
        assert!(
            !allows(
                &scout,
                "bucket.read".parse().unwrap(),
                &other,
                SystemTime::now()
            )
            .is_allowed()
        );

        // Taken back, or the role removed, it grants nothing.
        assert!(roles.take("research", &p("service:scoutsimjo7n")));
        assert!(roles.of(&p("service:scoutsimjo7n")).is_empty());
        roles.give("research", p("service:scoutsimjo7n")).unwrap();
        assert!(roles.remove("research"));
        assert!(roles.of(&p("service:scoutsimjo7n")).is_empty());
        assert_eq!(roles.holders("research").count(), 0);
    }

    #[test]
    fn only_a_role_there_is_is_given() {
        let mut roles = Roles::default();
        assert!(roles.give("research", p("service:scoutsimjo7n")).is_err());
        assert!(
            roles
                .define(role("name = \"Research\"\nversion = 1"))
                .is_err()
        );
        roles.define(role(RESEARCH)).unwrap();
        roles.give("research", p("service:scoutsimjo7n")).unwrap();
        roles.give("research", p("service:quillrch4don")).unwrap();
        let holders: Vec<_> = roles.holders("research").map(|h| h.to_string()).collect();
        assert_eq!(holders, ["service:quillrch4don", "service:scoutsimjo7n"]);
        assert_eq!(roles.forget(&p("service:quillrch4don")).len(), 1);
    }

    /// All of a team's folder is a grant of all of it: one on its top
    /// (`bucket:team-r7/`) or one folder reaches no more, whatever name
    /// is asked of it.
    #[test]
    fn all_of_a_folder_is_asked_of_a_pattern_never_a_name() {
        let own: crate::AccountId = "acct-a1".parse().unwrap();
        let held = |on: &str| {
            let role = role(&format!(
                "name = \"r\"\nversion = 1\n[[allow]]\nactions = [\"bucket.read\"]\non = [\"{on}\"]"
            ));
            Principal::new(p("service:scoutsimjo7n"), own.clone(), None, role.clone())
                .with_roles([role])
        };
        let all: crate::Resources = "bucket:team-r7/*".parse().unwrap();
        let read = "bucket.read".parse().unwrap();
        let now = SystemTime::now();
        for reaching in ["bucket:team-r7/*", "bucket:*", "*", "bucket:team-*"] {
            let whole = crate::allows_all(&held(reaching), read, &all, now);
            assert!(whole.is_allowed(), "{reaching}: {whole}");
        }
        for not in [
            "bucket:team-r7/",
            "bucket:team-r7/drafts/*",
            "bucket:team-r70/*",
            "acct-b2/bucket:*",
        ] {
            let whole = crate::allows_all(&held(not), read, &all, now);
            assert!(!whole.is_allowed(), "{not}");
        }
        // Writing all of it is never reading it, nor another account's.
        assert!(
            !crate::allows_all(&held("*"), "bucket.write".parse().unwrap(), &all, now).is_allowed()
        );
        let theirs: crate::Resources = "acct-b2/bucket:team-r7/*".parse().unwrap();
        assert!(!crate::allows_all(&held("*/bucket:*"), read, &theirs, now).is_allowed());
    }

    #[test]
    fn a_roles_pattern_ends_a_segment() {
        let mut roles = Roles::default();
        let on = |pattern: &str| {
            role(&format!(
                "name = \"r\"\nversion = 1\n[[allow]]\nactions = [\"*\"]\non = [\"{pattern}\"]"
            ))
        };
        for fine in [
            "bucket:team-r7/*",
            "bucket:*",
            "team:*",
            "*",
            "agent:a1b2c3d4e5f6",
            "cloud:ci-*",
        ] {
            roles.define(on(fine)).unwrap();
        }
        for over in [
            "bucket:team-r7*",
            "team:r*",
            "agent:a1*",
            "bucket:team-r7/dra*",
        ] {
            assert!(roles.define(on(over)).is_err(), "{over}");
        }
    }

    #[test]
    fn a_role_kept_before_a_rule_tightened_is_refused_as_read() {
        let mut roles = Roles::default();
        roles.define(role(RESEARCH)).unwrap();
        roles.check().unwrap();
        let text = toml::to_string(&roles)
            .unwrap()
            .replace("bucket:team-r7/drafts/*", "bucket:team-r7*");
        let read: Roles = toml::from_str(&text).unwrap();
        assert!(read.check().is_err());
    }

    #[test]
    fn it_is_kept_as_toml() {
        let mut roles = Roles::default();
        roles.define(role(RESEARCH)).unwrap();
        roles.give("research", p("service:scoutsimjo7n")).unwrap();
        let text = toml::to_string(&roles).unwrap();
        assert_eq!(toml::from_str::<Roles>(&text).unwrap(), roles);
    }
}
