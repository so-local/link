//! The firewall rules Link's Google Cloud boxes share, from `firewall.toml`:
//! the GCP adapter makes them outside a job, and a project's setup makes
//! them for a job's launches, which never own them.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

/// What a rule does with what it matches.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Allow,
    Deny,
}

impl Action {
    /// Its word in `gcloud compute firewall-rules create --action`.
    pub fn word(self) -> &'static str {
        match self {
            Action::Allow => "allow",
            Action::Deny => "deny",
        }
    }
}

/// One ingress rule on the boxes that carry [`tag`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub action: Action,
    /// `PROTOCOL[:PORT]`, as `gcloud --rules` takes them: `tcp:22`, `all`.
    pub rules: Vec<String>,
    /// Address ranges it matches.
    pub sources: Vec<String>,
    pub priority: u32,
    pub description: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Firewall {
    tag: String,
    rule: Vec<Rule>,
}

static FIREWALL: LazyLock<Firewall> = LazyLock::new(|| {
    toml::from_str(include_str!("firewall.toml")).expect("the boxes' firewall rules")
});

/// The network tag every Link box carries, which the rules target.
pub fn tag() -> &'static str {
    &FIREWALL.tag
}

/// The rules, in the order they're made: the deny rule first.
pub fn rules() -> &'static [Rule] {
    &FIREWALL.rule
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_let_in_ssh_alone_with_the_deny_made_first() {
        let rules = rules();
        assert_eq!(rules.len(), 2);
        let (deny, allow) = (&rules[0], &rules[1]);
        assert_eq!(deny.action, Action::Deny);
        assert_eq!(deny.rules, ["all"]);
        assert_eq!(allow.action, Action::Allow);
        assert_eq!(allow.rules, ["tcp:22"]);
        // Google applies the lower number first: the SSH rule wins over
        // the deny, and the deny over the default network's 65534.
        assert!(allow.priority < deny.priority && deny.priority < 65534);
        assert_ne!(deny.name, allow.name);
        assert!(!tag().is_empty());
    }
}
