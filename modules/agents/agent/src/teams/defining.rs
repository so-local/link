//! Apply desired members and policy, saving identity before provisioning.
use super::*;
use anyhow::{Context, ensure};
use std::collections::BTreeMap;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    id: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    members: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pending: BTreeSet<String>,
}

link_files::specification!(Identity, include_str!("identity.schema.toml"));

/// Each team's identity, which an upgrade converts.
pub(crate) fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Identity>()
}

pub(super) struct Applied {
    pub text: String,
    /// The members it changed, and the agents it took off the team.
    pub affected: Vec<String>,
}

struct Member {
    agent: Settings,
    definition: Option<link_agent_spec::Spec>,
    new: bool,
}

/// An apply may leave configured members when a later member fails. The
/// canonical IDs remain, so retry resumes them without creating copies.
pub(super) fn apply(
    out: &mut Out,
    source: &link_spec::Read<link_agent_spec::TeamFile>,
) -> anyhow::Result<Applied> {
    let desired = link_agent_spec::parse_team_definition(&source.text)?;
    let path = settings::team_path(&desired.name);
    let identity_path = settings::team_dir(&desired.name).join(constants::DEFINITION_IDENTITY);
    let before = match path.try_exists()? {
        true => Some(link_spec::read::<toml::Table>(&path)?.text),
        false => None,
    };
    let previous = before
        .as_deref()
        .map(link_agent_spec::parse_team_definition)
        .transpose()?;
    let identity: Option<Identity> = link_files::saved::read_optional(&identity_path)?;
    // Its IDs are read from the team's data folder, so each is checked
    // before it's used: an ID, and the team's never one retired here.
    if let Some(identity) = &identity {
        link_machine::machine::check_id(&identity.id)
            .within(format!("the team's ID in {}", identity_path.display()))?;
        for id in identity.members.values().chain(&identity.pending) {
            link_machine::machine::check_id(id)
                .within(format!("a member's ID in {}", identity_path.display()))?;
        }
        ensure!(
            !settings::ids::TEAMS.is_retired(&identity.id)?,
            "{} names a team deleted here: remove it and apply again",
            identity_path.display()
        );
    }
    // A team's ID is reserved in its ledger as an agent's is: one the
    // definition gives a new team is claimed there once its identity is
    // saved, refused while the ledger holds it or once it's retired. One
    // read from the team's own definition, the team here, is checked as
    // its identity's is: never one retired here.
    let kept = identity.as_ref().map(|i| i.id.clone());
    let (id, given) = match (
        kept,
        previous.as_ref().and_then(|d| d.id.clone()),
        &desired.id,
    ) {
        (Some(id), ..) => (id, false),
        (None, Some(read), _) => {
            link_machine::machine::check_id(&read)
                .within(format!("the team's ID in {}", path.display()))?;
            ensure!(
                !settings::ids::TEAMS.is_retired(&read)?,
                "{} names a team deleted here: remove it and apply again",
                path.display()
            );
            (read, false)
        }
        (None, None, Some(given)) => (given.clone(), true),
        (None, None, None) => (settings::ids::TEAMS.reserve()?, false),
    };
    // The members whose IDs a definition gave, each checked as it arrives
    // once it's known which agent here it names.
    let mut read = BTreeSet::new();
    let mut assigned = BTreeMap::new();
    for (name, member) in &desired.members {
        // An ID the definition names stands, checked as it arrives below;
        // one the team's files remember is the member's only
        // while it isn't retired: a member deleted since is another agent
        // when it comes again, with an ID reserved for it.
        let saved = identity.as_ref().and_then(|i| i.members.get(name).cloned());
        let remembered = saved.clone().or_else(|| {
            previous
                .as_ref()
                .and_then(|d| d.members.get(name)?.id.clone())
        });
        let remembered = match remembered {
            Some(id) if settings::ids::is_retired(&id)? => None,
            other => other,
        };
        if member.id.is_some() || (saved.is_none() && remembered.is_some()) {
            read.insert(name.clone());
        }
        let known = member.id.clone().or(remembered);
        assigned.insert(
            name.clone(),
            match known {
                Some(id) => id,
                None => settings::ids::reserve()?,
            },
        );
    }
    let (text, team) = link_agent_spec::assign_team_ids(&source.text, &id, &assigned)?;
    if let Some(shared) = &team.shared {
        ensure!(
            settings::unsafe_files(shared).is_none(),
            "the shared folder is private agent or system state"
        );
        let agents = settings::agents();
        ensure!(
            !shared.starts_with(&agents) && !agents.starts_with(shared),
            "the shared folder overlaps agents' private folders"
        );
    }
    let ids = by_id();
    let others = settings::teams_except(&team.name)?;
    let owner = link_core::owner::current_owner()?;
    let mut members = Vec::new();
    for (name, member) in &team.members {
        ensure!(
            member.owner.is_none(),
            "an outside member needs its account's accepted hosted membership"
        );
        let local_name = ids.get(&member.id).map(String::as_str).unwrap_or(name);
        if read.contains(name) {
            link_machine::machine::check_id(&member.id)
                .within(format!("member {name}'s ID in its definition"))?;
            settings::ids::arrive(local_name, &member.id)?;
        }
        let new =
            !Settings::path_of(local_name).exists() && !settings::records::has_identity(local_name);
        if new {
            ensure!(
                member.agent.is_some(),
                "member {name} isn't here; provide its agent definition or move it here first"
            );
            ensure!(
                link_agent::moved::of_id(&member.id)?.is_none()
                    && link_agent::moved::of_name(local_name)?.is_none(),
                "member {name} moved away: move it here before applying local settings"
            );
        }
        let mut agent = Settings::load(local_name)?;
        ensure!(
            agent.agent_id().is_none_or(|id| id == member.id),
            "member {name} conflicts with an existing agent: use its permanent ID"
        );
        if desired.members[name].id.is_none()
            && identity.is_none()
            && previous
                .as_ref()
                .is_none_or(|p| !p.members.contains_key(name))
        {
            ensure!(
                new && !settings::records::has_identity(local_name),
                "member {name} exists already: name its permanent ID explicitly"
            );
        }
        agent.settle_owner()?;
        ensure!(
            agent.owner.as_deref() == Some(&owner),
            "member {name} belongs to another account"
        );
        agent.id = Some(crate::settings::ids::workload(&member.id)?);
        if let Some(spec) = &member.agent {
            ensure!(
                spec.isolation.as_ref().is_none_or(|i| i.is_empty()) && spec.env.is_none(),
                "member {name} requests another machine: create or move it there before adding its ID"
            );
            ensure!(
                spec.name.as_deref().is_none_or(|name| name == agent.name),
                "the member definition cannot rename an existing agent"
            );
        }
        let own = agent.own_spec()?;
        let mut on = others
            .iter()
            .filter(|t| t.has(&member.id))
            .cloned()
            .collect::<Vec<_>>();
        on.push(team.clone());
        link_agent_spec::resolve_member(
            own.as_ref(),
            &on,
            &link_agent_spec::Known {
                name: agent.name.clone(),
            },
            &member.id,
        )
        .with_context(|| format!("member {name}'s effective spec"))?;
        members.push(Member {
            agent,
            definition: member.agent.clone(),
            new: new
                || identity
                    .as_ref()
                    .is_some_and(|identity| identity.pending.contains(&member.id)),
        });
    }
    ensure!(
        link_spec::read::<toml::Table>(&source.path)?.text == source.text,
        "the source changed: apply it again"
    );
    let current = match path.try_exists()? {
        true => Some(link_spec::read::<toml::Table>(&path)?.text),
        false => None,
    };
    ensure!(current == before, "the team changed: apply it again");
    let mut identity = Identity {
        id,
        members: assigned,
        pending: members
            .iter()
            .filter(|member| member.new)
            .map(|member| member.agent.key())
            .collect(),
    };
    link_files::saved::write(&identity_path, &identity)?;
    // Claimed once its identity is saved, so a save that fails holds
    // nothing a retry would meet; refused, the identity goes with it.
    if given && let Err(refused) = settings::ids::TEAMS.claim(&identity.id) {
        let _ = std::fs::remove_file(&identity_path);
        return Err(refused);
    }
    link_files::write_private(&path, text.as_bytes())?;
    if let Some(shared) = &team.shared {
        make_shared(&team.name, shared)?;
    }
    for member in &mut members {
        provision(member).with_context(|| {
            format!(
                "applying {}: IDs are saved; retry this apply to continue",
                member.agent.name
            )
        })?;
        identity.pending.remove(&member.agent.key());
        link_files::saved::write(&identity_path, &identity)?;
    }
    let mut affected = members
        .iter()
        .map(|m| m.agent.name.clone())
        .collect::<Vec<_>>();
    for name in settings::names()? {
        let agent = Settings::load_without_keys(&name)?;
        if !team.has(&agent.key()) && sandbox_file::of(&name).spec.teams.contains_key(&team.name) {
            drop_grant(out, &name, &team.name);
            affected.push(name);
        }
    }
    Ok(Applied { text, affected })
}

fn provision(member: &mut Member) -> anyhow::Result<()> {
    if let Some(spec) = &member.definition
        && let Some(harness) = &spec.harness
    {
        link_agent::adapter::Adapter::find_or_add(&harness.name)?;
        member.agent.harness = Some(harness.name.clone());
    }
    // Membership alone grants none of the folder: `lnk agent team add
    // --write` and `--read` grant it.
    member.agent.save()?;
    Ok(())
}

/// Delete intentionally releases only the team identity; member data stays.
pub(super) fn forget_identity(team: &str) -> anyhow::Result<()> {
    let path = settings::team_dir(team).join(constants::DEFINITION_IDENTITY);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}
