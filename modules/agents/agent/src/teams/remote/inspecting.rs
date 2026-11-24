//! Read or end hosted membership without creating local team state.
use super::*;
use link_format::{Note, Question, prose};

pub(in crate::teams) fn show(
    out: &mut Out,
    named: &str,
    only: Option<&str>,
    json: bool,
) -> Result<Done, Report> {
    let (account, hosted) = find(named)?;
    let (kept, _) = read(&account, &hosted.id)?;
    let team = link_agent_spec::parse_team(&kept.file)?;
    if let Some(member) = only {
        team.members.get(member).within("no such member")?;
    }
    let mut table = Table::new(
        ["Member", "Agent", "Purpose", "Membership", "Lead"],
        sentence!("No one is on it yet."),
    );
    let mut members = Vec::new();
    for (name, member) in team
        .members
        .iter()
        .filter(|(name, _)| only.is_none_or(|only| only == name.as_str()))
    {
        let joined = match &member.owner {
            Some(owner) => kept
                .joined
                .contains(&link_agent_identity::access::teammate(owner, &member.id)?.to_string()),
            None => true,
        };
        table.row([
            Name::new("member", name).into(),
            Value::from(member.id.clone()),
            Value::from(member.purpose.clone().unwrap_or_default()),
            Value::from(if joined { "joined" } else { "invited" }),
            Value::from(if team.lead.as_ref() == Some(name) {
                "yes"
            } else {
                ""
            }),
        ]);
        members.push(serde_json::json!({
            "name": name, "workload": link_agent::settings::ids::written(&member.id),
            "owner": member.owner,
            "purpose": member.purpose, "joined": joined,
        }));
    }
    if json {
        link_actions::answer::line::<link_agent_spec::declared::TeamsHostedShown>(
            out,
            &serde_json::json!({
                "name": team.name, "resource": link_agent_team::resource(&team.id),
                "account": account.name,
                "version": kept.version, "members": members,
                "lead": team.lead,
            }),
        )?;
    } else {
        out.block(table);
    }
    Ok(Done::shown())
}

pub(in crate::teams) fn delete(out: &mut Out, named: &str, yes: bool) -> Result<Done, Report> {
    let (account, team) = find(named)?;
    let shown = Name::new("team", format!("{}@{}", team.name, account.name));
    if !out.ask(
        Question::confirm(sentence!("Delete team {}?", &shown), false)
            .why(prose!(
                "Its members leave it; their agents and the shared folder stay."
            ))
            .yes(yes),
    )? {
        out.block(Note(sentence!("Nothing deleted.")));
        return Ok(Done::shown());
    }
    call(
        &account,
        Action::Delete(link_agent_place::teams::TeamActionDelete { team: team.id }),
    )?;
    Ok(Done::success(sentence!("Team {} is deleted.", &shown)))
}
