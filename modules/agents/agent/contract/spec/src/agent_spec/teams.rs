//! A team's file: its head, members and shared folder, and the opinions
//! it holds ([`parse_team`]).

use super::*;
use anyhow::Context;
use link_format::WithinExt;

#[cfg(test)]
mod tests;

/// A team's file: its head (`[access]` among it, which takes
/// [`TEAM_ACCESS`]), its roster and its shared settings, the opinions
/// every member shares in `[settings.agent]`, as `types.toml` declares
/// it. Its checks are the declaration's `team` hook ([`check_team`]).
pub use crate::declared::{Team as TeamFile, TeamFiles};

/// A team's file in the forms its runtime reads: each member, its
/// `[access]` and its settings as the team and identity contracts own
/// them. The same values as the file, typed by their owners; its
/// settings the file's own.
struct Head {
    id: Option<String>,
    name: String,
    member: BTreeMap<String, MemberDefinition>,
    lead: Option<String>,
    files: Option<TeamFiles>,
    access: crate::access::MemberAccess,
    settings: TeamSettings,
}

/// `value` as `T`, the type its owner reads the same shape as.
fn retyped<T: serde::de::DeserializeOwned>(value: &impl Serialize) -> anyhow::Result<T> {
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

impl Head {
    fn of(file: &TeamFile) -> anyhow::Result<Self> {
        let member = file
            .member
            .iter()
            .map(|(name, member)| {
                let mut member: MemberDefinition =
                    retyped(member).within(format!("[member.{name}]"))?;
                // Written `service:<id>`, as its type declares; the runtime
                // holds the service's ID.
                member.service = member
                    .service
                    .map(|service| {
                        service
                            .strip_prefix("service:")
                            .map(str::to_string)
                            .within(format!("[member.{name}] service is service:<id>"))
                    })
                    .transpose()?;
                Ok((name.clone(), member))
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self {
            id: file.id.clone(),
            name: file.name.clone(),
            member,
            lead: file.lead.clone(),
            files: file.files.clone(),
            access: match &file.access {
                Some(access) => retyped(access).within("[access]")?,
                None => Default::default(),
            },
            settings: file.settings.clone(),
        })
    }
}

/// What a team's `[access]` takes: agents of other accounts, by their
/// services, as members.
pub const TEAM_ACCESS: &[crate::access::Verb] = &[crate::access::Verb::Member];

/// Desired membership and team policy from one canonical document.
/// New managed members may omit IDs until the apply operation assigns
/// and persists them; runtime authorization always requires [`Team`].
#[derive(Clone, Debug, PartialEq)]
pub struct TeamDefinition {
    pub id: Option<String>,
    pub name: String,
    pub members: BTreeMap<String, MemberDefinition>,
    pub lead: Option<String>,
    pub shared: Option<std::path::PathBuf>,
    pub access: crate::access::Access,
    pub settings: TeamSettings,
}

impl TeamDefinition {
    /// Runtime form, only after every identity has been assigned. A
    /// declaration with unresolved IDs never becomes membership proof.
    pub fn into_team(self) -> anyhow::Result<Team> {
        let id = self
            .id
            .context("the team has no ID: apply its definition first")?;
        crate::machine::check_id(&id)?;
        let mut ids = std::collections::BTreeSet::new();
        let members = self
            .members
            .into_iter()
            .map(|(name, member)| {
                let id = member.id.clone().with_context(|| {
                    format!("member {name} has no ID: apply its definition first")
                })?;
                crate::machine::check_id(&id)?;
                ensure!(ids.insert(id.clone()), "agent {id} is a member twice");
                Ok((name, member.map_id(|_| id)))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
        Ok(Team {
            id,
            name: self.name,
            members,
            lead: self.lead,
            shared: self.shared,
            access: self.access,
            settings: self.settings,
        })
    }
}

/// Parses a team's file ([`TeamFile`]), whose `[settings.agent]` holds
/// opinion tables only. Identity, harness, environment, message access and
/// stranger-tool authority remain each agent's own.
/// Another account's agent is a member and in `[access] member` both, or
/// neither.
pub fn parse_team(text: &str) -> anyhow::Result<Team> {
    parse_team_definition(text)?.into_team()
}

/// Validate the desired team before any provisioning or mutation. The
/// roster defines agents; settings define shared behavior and resources.
pub fn parse_team_definition(text: &str) -> anyhow::Result<TeamDefinition> {
    let head = Head::of(&link_spec::parse_spec::<TeamFile>(text)?)?;
    let shared = check_head(&head)?;
    Ok(TeamDefinition {
        id: head.id,
        name: head.name,
        members: head.member,
        lead: head.lead,
        shared,
        access: head.access.into(),
        settings: head.settings,
    })
}

/// A team's file held to its rules, the declaration's `team` hook: its
/// members, access, lead, folder and settings, each against the others.
pub(crate) fn check_team(file: &TeamFile) -> anyhow::Result<()> {
    check_head(&Head::of(file)?).map(drop)
}

fn check_head(head: &Head) -> anyhow::Result<Option<std::path::PathBuf>> {
    if let Some(id) = &head.id {
        crate::machine::check_id(id)?;
    }
    crate::machine::check_name(&head.name)?;
    crate::access::Access::from(head.access.clone()).only(TEAM_ACCESS)?;
    let mut ids = std::collections::BTreeSet::new();
    let mut outside = std::collections::BTreeSet::new();
    for (name, member) in &head.member {
        crate::machine::check_name(name)?;
        if let Some(id) = &member.id {
            crate::machine::check_id(id).within(format!("[member.{name}]"))?;
            ensure!(
                ids.insert(id.as_str()),
                "[member.{name}]: agent {id} is a member twice"
            );
        } else {
            ensure!(
                member.agent.is_some(),
                "[member.{name}] needs an existing id or an agent definition"
            );
        }
        if let Some(agent) = &member.agent {
            check(agent).within(format!("[member.{name}.agent]"))?;
            ensure!(
                member.owner.is_none(),
                "an outside member's agent definition belongs to its own account"
            );
            ensure!(
                agent.name.as_deref().is_none_or(|own| own == name),
                "[member.{name}.agent] names another agent"
            );
        }
        if let Some(purpose) = &member.purpose {
            ensure!(
                purpose.chars().count() <= PURPOSE_MAX && !purpose.chars().any(char::is_control),
                "[member.{name}] purpose: one line of at most {PURPOSE_MAX} characters"
            );
        }
        match &member.owner {
            Some(account) => {
                let id = member
                    .id
                    .as_deref()
                    .context("an outside member needs its existing ID")?;
                let agent = crate::access::teammate(account, id)
                    .within(format!("[member.{name}] owner"))?;
                ensure!(
                    head.access.member.contains(&agent),
                    "[member.{name}] is another account's agent, which [access] member \
                     doesn't name ({})",
                    agent.as_str()
                );
                member.folders().check(&format!("[member.{name}]"))?;
                outside.insert(agent);
            }
            None => ensure!(
                member.write.is_empty() && member.read.is_empty(),
                "[member.{name}] is the team owner's agent: what it reaches of the folder is \
                 in its sandbox file, not here"
            ),
        }
    }
    if let Some(stray) = head.access.member.iter().find(|a| !outside.contains(*a)) {
        bail!(
            "[access] member names {}, which no [member.<name>] with its owner is",
            link_format::clean(stray)
        );
    }
    if let Some(lead) = &head.lead {
        crate::machine::check_name(lead).within("lead")?;
        ensure!(
            head.member.contains_key(lead),
            "the lead is not a team member: {lead}"
        );
    }
    let shared = head
        .files
        .as_ref()
        .map(|files| shared_folder(&files.shared))
        .transpose()?;
    let settings = &head.settings;
    check_opinions(&settings.agent)?;
    if settings.memory.semantic {
        ensure!(
            settings.memory.daily_credits.is_some(),
            "semantic team memory needs an owner-set daily_credits cap"
        );
    }
    ensure!(
        settings.memory.daily_credits != Some(0),
        "daily_credits is positive; disable semantic search to opt out"
    );
    let mut selected = Vec::new();
    if let TeamIndexer::Member(named) = &settings.memory.indexer {
        selected.push(("indexer", &named.member));
    }
    if let TeamPayer::Member(named) = &settings.memory.payer {
        selected.push(("payer", &named.member));
    }
    for (setting, member) in selected {
        ensure!(
            head.member.contains_key(member),
            "memory {setting} names no team member: {member}"
        );
    }
    Ok(shared)
}

/// Shared opinions never grant authority that belongs to an agent's own spec.
pub(super) fn check_opinions(spec: &Spec) -> anyhow::Result<()> {
    check(spec).within("[settings.agent]")?;
    ensure!(
        spec.name.is_none()
            && spec.version.is_none()
            && spec.extends.is_none()
            && spec.harness.is_none()
            && spec.isolation.as_ref().is_none_or(|i| i.is_empty())
            && spec.env.is_none()
            && spec.access.is_none()
            && spec.tools.as_ref().is_none_or(|t| t.strangers.is_none()),
        "a team's spec holds opinions only: name, version, extends, harness, isolation, env, \
         [access] and [tools] strangers are each agent's own"
    );
    Ok(())
}

/// The document in its current schema, keeping comments and existing IDs.
pub fn team_document(text: &str) -> anyhow::Result<String> {
    parse_team_definition(text)?;
    let (doc, _) = TeamFile::format().document(text)?;
    let written = doc.to_string();
    parse_team_definition(&written)?;
    Ok(written)
}

/// A team's file holding `table`'s keys, as Link writes one: naming its
/// kind and schema, checked as a team's.
pub fn team_text(table: &toml::Table) -> anyhow::Result<String> {
    let mut body = table.clone();
    body.remove("schema");
    body.remove("kind");
    let written = TeamFile::format().print(&body)?;
    parse_team_definition(&written)?;
    Ok(written)
}

/// Materialize a desired roster before creating agents. Assignments may
/// fill an absent identity, never change one already kept in the file.
/// The caller persists the returned document before provisioning.
pub fn assign_team_ids(
    text: &str,
    team_id: &str,
    members: &BTreeMap<String, String>,
) -> anyhow::Result<(String, Team)> {
    let definition = parse_team_definition(text)?;
    crate::machine::check_id(team_id)?;
    ensure!(
        definition.id.as_deref().is_none_or(|id| id == team_id),
        "cannot replace the team's ID"
    );
    ensure!(
        members
            .keys()
            .all(|name| definition.members.contains_key(name)),
        "an assigned agent is not in the roster"
    );
    let mut doc: toml_edit::DocumentMut = team_document(text)?.parse()?;
    if definition.id.is_none() {
        doc["id"] = toml_edit::value(team_id);
    }
    for (name, member) in definition.members {
        let assigned = members
            .get(&name)
            .or(member.id.as_ref())
            .with_context(|| format!("member {name} has no assigned ID"))?;
        crate::machine::check_id(assigned)?;
        ensure!(
            member.id.as_ref().is_none_or(|id| id == assigned),
            "cannot replace member {name}'s ID"
        );
        if member.id.is_none() {
            doc["member"][&name]["id"] = toml_edit::value(assigned);
        }
    }
    let written = doc.to_string();
    let team = parse_team(&written)?;
    Ok((written, team))
}

/// The longest a member's purpose is, in characters: a line, for people
/// and its teammates' prompts.
pub const PURPOSE_MAX: usize = 200;

/// A team's shared folder as its file says it (`~/Link/Teams/research/shared`,
/// or an absolute path), `~` made the home folder; an error for one
/// that's relative or climbs out with `..`.
pub fn shared_folder(text: &str) -> anyhow::Result<std::path::PathBuf> {
    let path = match text.strip_prefix("~/") {
        Some(rest) => link_files::home_dir().join(rest),
        None => std::path::PathBuf::from(text),
    };
    ensure!(
        path.is_absolute()
            && path
                .components()
                .all(|c| !matches!(c, std::path::Component::ParentDir)),
        "[files] shared {:?} isn't a folder's absolute path (or ~/...)",
        link_format::clean(text)
    );
    Ok(path)
}
