//! The tools offered (`tools/list`): each call's description and schema.

use super::*;

/// The calls `actions.toml` declares, as its contract renders them.
const TEMPLATE: &str = link_scheduler_tool_contract::TOOLS;

/// Whom it may send to, by name: each team, then each teammate, by the
/// team's name too where two share a name.
fn names(teams: &Teams) -> Vec<String> {
    let mut names: Vec<String> = teams.teams.keys().cloned().collect();
    for mate in &teams.teammates {
        let shared = teams
            .teammates
            .iter()
            .any(|m| m.name == mate.name && m.agent != mate.agent);
        let name = match shared {
            true => format!("{}/{}", mate.team, mate.name),
            false => mate.name.clone(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The tools offered, from `actions.toml`: `send` only to an agent on a
/// team, its choices whom it may send to, and `schedule`'s answer one of
/// where it may go.
pub(super) fn offered(tool: &Tool) -> anyhow::Result<Value> {
    use link_actions::tools::{Fill, Fills};
    let teams = &tool.teams;
    let on_team = !teams.teammates.is_empty();
    let mut values = std::collections::BTreeMap::from([("answers", Fill::Words(tool.answers()))]);
    // Off a team there's no `send`, and no one to send to.
    if on_team {
        values.insert("teammates", Fill::Words(names(teams)));
    }
    let fills = Fills {
        values: values.into_iter().collect(),
        conditions: on_team.then_some("team").into_iter().collect(),
    };
    link_actions::tools::offer(TEMPLATE, &fills)
}
