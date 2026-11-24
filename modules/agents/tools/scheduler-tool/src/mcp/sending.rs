//! `send`: a message to a teammate, or to each of a team's, and who it
//! reaches.

use super::*;

/// A message to a teammate, or to each of a team's: now, to its
/// conversation with this agent (`agent-<this agent's ID>`) or the
/// team's (`team-<its ID>`), each answer back here, as a message from
/// the teammate.
pub(super) fn send(
    tool: &Tool,
    made: &Made,
    arguments: &Value,
    hops: u32,
) -> anyhow::Result<Value> {
    let back = made
        .thread
        .clone()
        .context("a message to a teammate needs a conversation for its answer to come back to")?;
    let me = tool
        .teams
        .agent
        .clone()
        .context("this agent is on no team")?;
    let to = arguments
        .get("to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .context("'to' is a teammate's name, or your team's")?;
    let text = message(arguments, "message")?;
    let targets = recipients(&tool.teams, &me, to)?;
    ensure!(
        tool.store.waiting() + targets.len() <= limits().jobs_max,
        "Not sent: {} jobs waiting.",
        limits().jobs_max
    );
    let mut sent = Vec::new();
    for (name, agent, thread) in targets {
        let job = Job {
            occurrence: None,
            portable: None,
            id: link_id::candidate()?,
            kind: Kind::Send,
            thread,
            text: text.clone(),
            answer: AnswerTo::Thread(back.clone()),
            due: now(),
            cron: None,
            hops,
            created: now(),
            tries: 0,
            tools: None,
            standing: made.standing()?,
            model: None,
            to: Some(Peers {
                from: me.clone(),
                to: agent,
            }),
            parent: super::making::coordinator(made),
            after: Vec::new(),
            description: None,
            outcome: None,
        };
        let disposition = crate::interactions::prepare(&tool.store, &job, &made.meta)?;
        tool.store.put(&job)?;
        sent.push(link_agent_contract::namespace::Recipient {
            to: name,
            id: job.id,
            delegation: disposition,
            held: false,
        });
    }
    Ok(serde_json::to_value(
        link_agent_contract::namespace::Sent { sent },
    )?)
}

/// Whom `to` names: a teammate by its name (or `<team>/<name>` where two
/// teams' members share one), or a team, each of its members but this
/// agent; each once, with the conversation it lands in there.
fn recipients(teams: &Teams, me: &str, to: &str) -> anyhow::Result<Vec<(String, String, String)>> {
    let names = || {
        let mut names: Vec<String> = teams.teams.keys().cloned().collect();
        for mate in &teams.teammates {
            if !names.contains(&mate.name) {
                names.push(mate.name.clone());
            }
        }
        names.join(", ")
    };
    let mut found: Vec<(String, String, String)> = Vec::new();
    if let Some(team_id) = teams.teams.get(to) {
        for mate in teams.teammates.iter().filter(|m| m.team == to) {
            if !found.iter().any(|(_, agent, _)| *agent == mate.agent) {
                found.push((
                    format!("{}/{}", mate.team, mate.name),
                    mate.agent.clone(),
                    crate::teams_thread(team_id),
                ));
            }
        }
        ensure!(
            !found.is_empty(),
            "Not sent: team {to} has no one else on it."
        );
        return Ok(found);
    }
    let (team, name) = match to.split_once('/') {
        Some((team, name)) => (Some(team), name),
        None => (None, to),
    };
    let named: Vec<&crate::Teammate> = teams
        .teammates
        .iter()
        .filter(|m| m.name == name && team.is_none_or(|t| m.team == t))
        .collect();
    let first = named
        .first()
        .with_context(|| format!("Not sent: no teammate {to}. Yours: {}.", names()))?;
    ensure!(
        named.iter().all(|m| m.agent == first.agent),
        "Not sent: {to} names a teammate on more than one team: say which, as {}.",
        named
            .iter()
            .map(|m| format!("{}/{}", m.team, m.name))
            .collect::<Vec<_>>()
            .join(" or ")
    );
    Ok(vec![(
        to.to_string(),
        first.agent.clone(),
        crate::teammates_thread(me),
    )])
}
