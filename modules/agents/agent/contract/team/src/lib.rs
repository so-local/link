//! Team membership and shared-folder grants.

pub use link_agent_identity::access;
pub use link_core::*;
pub use link_harness_base as harness;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod moving;

/// A team's settings as far as membership reads them: the member its
/// spec names to keep the team memory's index. The spec's owner
/// declares the whole shape and implements this for it.
pub trait Settings {
    /// The member named to keep the index, by its name in the team's
    /// file: none, one elected among the eligible.
    fn indexer(&self) -> Option<&str>;
}

/// No settings: any eligible member may keep the index.
impl Settings for () {
    fn indexer(&self) -> Option<&str> {
        None
    }
}

/// The kind a team's ID is written with in output: a group's, `group:<id>`,
/// the one ID a team has.
pub const KIND: &str = "group";

/// Team `id` as output writes it, `group:<id>`.
pub fn resource(id: &str) -> String {
    format!("{KIND}:{id}")
}
/// A team: its name and ID, its members by ID, a folder they share, the
/// agents of other accounts it lets on, and its spec, the opinions its
/// members share beneath their own (`~/Link/Safe/Teams/<name>.toml`),
/// its settings `T`.
#[derive(Clone, Debug, PartialEq)]
pub struct Team<S, T = ()> {
    pub id: String,
    pub name: String,
    pub members: BTreeMap<String, Member<S>>,
    /// The member a message addressed to the team reaches. None until
    /// the owner selects a lead; never inferred from member ordering.
    pub lead: Option<String>,
    /// The folder its members share (`[files] shared`), `~` made the
    /// home folder: none, a team with no folder.
    pub shared: Option<std::path::PathBuf>,
    /// Who outside may be on it (`[access] member`): agents of other
    /// accounts, each a member here too ([`Member::owner`]).
    pub access: crate::access::Access,
    /// Team resource policy and the opinions every member shares.
    pub settings: T,
}

/// A team's member, by its name in the file.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "S: Deserialize<'de>, I: Deserialize<'de>"))]
pub struct Member<S = (), I = String> {
    /// The agent's ID.
    pub id: I,
    /// The service the agent acts as, its bare ID: the role holder a
    /// teammate's check names. An own agent's is read from its settings
    /// as the team is loaded; another account's is named in the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// A managed member's own agent definition, applied by team
    /// lifecycle controls; absent for an existing independent agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<S>,
    /// What it's for on the team, for people and its teammates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// Its account, for another account's agent, which the team's
    /// `[access] member` names: none, the team owner's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// For another account's agent, the folders of the team's folder it
    /// may write, and those it may only read, as `[teams.<team>]`
    /// names them: the most it reaches, the team owner's to say. An own
    /// agent's are in its sandbox file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write: Vec<String>,
    /// For another account's agent, the folders of the team's folder it
    /// may only read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<String>,
}

impl<S, I> Member<S, I> {
    /// Resolve a declaration's optional identity without copying its
    /// settings or losing any of its membership bounds.
    pub fn map_id<J>(self, id: impl FnOnce(I) -> J) -> Member<S, J> {
        Member {
            id: id(self.id),
            service: self.service,
            agent: self.agent,
            purpose: self.purpose,
            owner: self.owner,
            write: self.write,
            read: self.read,
        }
    }
    /// The folders of the team's folder another account's agent reaches,
    /// as its file says them.
    pub fn folders(&self) -> crate::harness::TeamFolders {
        crate::harness::TeamFolders {
            write: self.write.clone(),
            read: self.read.clone(),
        }
    }
}

/// `text` as one of IAM's names.
fn parse<T: std::str::FromStr<Err = String>>(text: String) -> anyhow::Result<T> {
    text.parse().map_err(anyhow::Error::msg)
}

impl<S, T: Settings> Team<S, T> {
    /// The spec's choice of index keeper. Current membership, accepted
    /// access and whole-folder write authority are still required.
    pub fn allows_indexer(&self, agent: &str) -> bool {
        self.has(agent)
            && match self.settings.indexer() {
                None => true,
                Some(member) => self
                    .members
                    .get(member)
                    .is_some_and(|member| member.id == agent),
            }
    }
}

impl<S, T> Team<S, T> {
    /// Resolve a team address or an explicit member name. Delivery must
    /// still check current acceptance and authority for the selected ID.
    pub fn target(&self, member: Option<&str>) -> anyhow::Result<(&str, &Member<S>)> {
        let name = member.or(self.lead.as_deref()).ok_or_else(|| {
            anyhow::anyhow!("the team has no lead: select a member or set its lead")
        })?;
        self.members
            .get_key_value(name)
            .map(|(name, member)| (name.as_str(), member))
            .ok_or_else(|| anyhow::anyhow!("no team member named {name}"))
    }

    /// Whether agent `id` is a member.
    pub fn has(&self, id: &str) -> bool {
        self.members.values().any(|m| m.id == id)
    }

    /// What being on it grants each member, as a role (`team-<id>`): to
    /// read the team and be on it, and to message each of its members.
    /// What each reaches of its folder is its own role on the folder.
    pub fn role(&self) -> anyhow::Result<link_policy::Policy> {
        let member = link_policy::Grant::new(
            &[parse("team.read".into())?, parse("team.member".into())?],
            &[parse(format!("team:{}", self.id))?],
        );
        let mut grants = vec![member];
        if !self.members.is_empty() {
            let on = self
                .members
                .values()
                .map(|m| parse(format!("agent:{}", m.id)))
                .collect::<anyhow::Result<Vec<_>>>()?;
            grants.push(link_policy::Grant::new(
                &[parse("agent.message".into())?],
                &on,
            ));
        }
        let role = link_policy::Policy {
            allow: grants,
            ..link_policy::Policy::none(&format!("team-{}", self.id.to_ascii_lowercase()))
        };
        role.check()?;
        Ok(role)
    }

    /// The service agent `id` acts as, as its entry here names it: none
    /// for an agent that isn't a member, or names none.
    pub fn service_of(&self, id: &str) -> Option<&str> {
        self.member(id)?.1.service.as_deref()
    }

    /// Agent `id`'s name in the team, and its entry, if it's a member.
    pub fn member(&self, id: &str) -> Option<(&String, &Member<S>)> {
        self.members.iter().find(|(_, m)| m.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str) -> Member {
        Member {
            id: id.into(),
            service: None,
            agent: None,
            purpose: None,
            owner: None,
            write: Vec::new(),
            read: Vec::new(),
        }
    }

    #[test]
    fn a_team_is_a_role_of_its_members() {
        let team = Team {
            id: "r7".into(),
            name: "research".into(),
            lead: Some("scout".into()),
            members: BTreeMap::from([
                ("scout".to_string(), member("a1b2c3d4e5f6")),
                ("writer".to_string(), member("k3m4n5p6q7r8")),
            ]),
            shared: None,
            access: Default::default(),
            settings: (),
        };
        let role = team.role().unwrap();
        assert_eq!(team.target(None).unwrap().1.id, "a1b2c3d4e5f6");
        assert_eq!(team.target(Some("writer")).unwrap().1.id, "k3m4n5p6q7r8");
        assert!(team.target(Some("missing")).is_err());
        let unled = Team {
            lead: None,
            ..team.clone()
        };
        assert!(unled.target(None).is_err());
        assert_eq!(unled.target(Some("writer")).unwrap().1.id, "k3m4n5p6q7r8");
        assert_eq!(role.name, "team-r7");
        let printed = link_spec::print_spec(&role).unwrap();
        assert_eq!(link_policy::parse(&printed).unwrap(), role);
        let text = toml::to_string(&role).unwrap();
        assert!(
            text.contains(r#"actions = ["team.read", "team.member"]"#),
            "{text}"
        );
        assert!(
            text.contains(r#"on = ["agent:a1b2c3d4e5f6", "agent:k3m4n5p6q7r8"]"#),
            "{text}"
        );
        let alone = Team {
            members: BTreeMap::new(),
            lead: None,
            ..team
        };
        assert_eq!(alone.role().unwrap().allow.len(), 1);
        assert!(alone.target(None).is_err());
    }
}

#[cfg(test)]
link_test_kit::program!(unit);
