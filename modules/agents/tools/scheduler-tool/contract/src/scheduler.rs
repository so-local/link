//! Link scheduler's flags (`lnk-scheduler-tool serve` and `run`), from
//! the agent's spec (`[tool.scheduler]`) and its teams: what the harness
//! module starts the agent's scheduler with, and what any machine
//! running the scheduler for an agent passes it, so the numbers are
//! always the spec's, never the program's nor the machine's.

use std::sync::LazyLock;

use anyhow::Context;
use serde::Deserialize;

use crate::agent_spec::{Late, SchedulerRecurring, Spec, Team, Tool};
use crate::cli::{Flags, FlagsFloor};

/// The tool the scheduler of the agent whose ID is `id` is served as,
/// on its proxy (`lnk sandbox tool add`).
pub fn tool_of(id: &str) -> String {
    format!("scheduler-{id}")
}

/// Its caps, from the spec's `[tool.scheduler]` (`spec.tool("scheduler")`),
/// as its program takes them: `--per-day`, `--subagents`, `--hops`,
/// `--interval` where it sets one, `--late` and `--time-zone`. An error naming the key a spec doesn't
/// set; Link's own spec sets each.
pub fn caps(scheduler: &Tool) -> anyhow::Result<Flags> {
    let number = |n: Option<u32>, key: &str| -> anyhow::Result<u32> {
        n.with_context(|| format!("its spec sets no [tool.scheduler] {key}"))
    };
    let late = match scheduler.late {
        Some(Late::Skip) => crate::cli::FlagsLate::Skip,
        _ => crate::cli::FlagsLate::Run,
    };
    let zone = scheduler
        .zone
        .clone()
        .context("its spec sets no [tool.scheduler] zone")?;
    Ok(Flags {
        per_day: Some(number(scheduler.per_day, "per_day")?),
        subagents: Some(number(scheduler.subagents, "subagents")?),
        hops: Some(number(scheduler.hops, "hops")?),
        interval: scheduler.interval,
        late: Some(late),
        time_zone: Some(zone),
        ..Flags::default()
    })
}

/// Its teams, as its program takes them: the agent's ID `id`, each of
/// `teams` it's on, and each other member (`--agent-id`, `--team-id`,
/// `--teammate`), whom `send` may reach; none for an agent on no team.
pub fn teams(id: &str, teams: &[Team]) -> Flags {
    let teams: Vec<&Team> = teams.iter().filter(|team| team.has(id)).collect();
    if teams.is_empty() {
        return Flags::default();
    }
    let mut flags = Flags {
        agent_id: Some(id.to_string()),
        ..Flags::default()
    };
    for team in teams {
        flags.team_id.push(format!("{}={}", team.name, team.id));
        for (name, member) in team.members.iter().filter(|(_, m)| m.id != id) {
            flags
                .teammate
                .push(format!("{}/{name}={}", team.name, member.id));
        }
    }
    flags
}

/// A team an account that hosts agents carries through its team relay,
/// as its `agent-team list` says it: the team's ID, its owner's account
/// and the relay's address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayedTeam {
    pub id: String,
    pub owner: String,
    pub relay: String,
}

/// The runner's team relay (`--team-relay`, `--team-owner`) for agent
/// `id` on `teams`, of those `relayed` carries: the first relay, by the
/// order of `teams`, and each team the agent is on that it carries, by
/// its name here and its owner. None for an agent on no relayed team.
/// The runner takes one relay, so a team carried by another is left
/// out.
pub fn relay(id: &str, teams: &[Team], relayed: &[RelayedTeam]) -> Flags {
    let on: Vec<(&Team, &RelayedTeam)> = teams
        .iter()
        .filter(|team| team.has(id))
        .filter_map(|team| Some((team, relayed.iter().find(|r| r.id == team.id)?)))
        .collect();
    let Some((_, first)) = on.first() else {
        return Flags::default();
    };
    Flags {
        team_relay: Some(first.relay.clone()),
        team_owner: on
            .iter()
            .filter(|(_, r)| r.relay == first.relay)
            .map(|(team, carried)| format!("{}={}", team.name, carried.owner))
            .collect(),
        ..Flags::default()
    }
}

/// How long the runner waits on a firing (`--timeout`), the spec's
/// `[tool.scheduler] firing`: the runner's alone, never `serve`'s. None
/// when the spec sets none.
pub fn timeout(scheduler: &Tool) -> anyhow::Result<Option<Flags>> {
    let Some(firing) = &scheduler.firing else {
        return Ok(None);
    };
    Ok(Some(Flags {
        timeout: Some(*firing),
        ..Flags::default()
    }))
}

/// The calls of its tool that never ask you: those that only read, and,
/// unless the spec says to ask first (`[tool.scheduler] ask`), every one,
/// as its declarations name them ([`crate::TOOLS`]). The harness's own
/// `compact` never asks: no model makes it, and no one is there to ask
/// as a turn ends.
pub fn allowed(scheduler: &Tool) -> anyhow::Result<Vec<String>> {
    match scheduler.ask.unwrap_or(false) {
        true => {
            let mut allowed = crate::reads()?;
            allowed.push(crate::compaction::compaction().call.clone());
            Ok(allowed)
        }
        false => crate::calls(),
    }
}

/// What `asking.toml` names: the argument the owner's `[tool.scheduler]
/// recurring` lets through, and the hint that names it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asking {
    /// The calls' argument the setting governs (`recurring`).
    pub argument: String,
    /// What lets a call asking by it through, for its refusal.
    pub hint: String,
}

/// `asking.toml`, read once.
pub fn asking() -> &'static Asking {
    static READ: LazyLock<Asking> = LazyLock::new(|| {
        toml::from_str(include_str!("asking.toml")).expect("asking.toml is an Asking")
    });
    &READ
}

/// The calls of its tool that ask by an argument, as the sandbox takes
/// them (`--ask-if`): each the declarations mark ([`crate::asks`]), but,
/// when the spec lets the agent's recurring schedules through
/// (`[tool.scheduler] recurring = "allow"`), those asking by
/// [`Asking::argument`]. Its spec, never what the agent writes, says.
pub fn ask_if(scheduler: &Tool) -> anyhow::Result<Vec<String>> {
    let allowed = scheduler.recurring == Some(SchedulerRecurring::Allow);
    let argument = &asking().argument;
    Ok(crate::asks()?
        .into_iter()
        .filter(|each| !(allowed && each.split_once('.').is_some_and(|(_, a)| a == argument)))
        .collect())
}

/// What `lnk sandbox tool add` serves its tool with, before its command:
/// each call [`allowed`] (`--add`), and each [`ask_if`] (`--ask-if`)
/// with the hint that names the setting letting them through
/// (`--ask-if-hint`, [`Asking::hint`]).
pub fn sandbox_flags(scheduler: &Tool) -> anyhow::Result<Vec<String>> {
    let mut flags = Vec::new();
    for call in allowed(scheduler)? {
        flags.extend(["--add".into(), call]);
    }
    let ask_if = ask_if(scheduler)?;
    if !ask_if.is_empty() {
        flags.extend(["--ask-if-hint".into(), asking().hint.clone()]);
    }
    for each in ask_if {
        flags.extend(["--ask-if".into(), each]);
    }
    Ok(flags)
}

/// The agent's floor (`--floor`), the least standing anyone reaches it
/// with: owner while only its owner may message it, member once its spec
/// shares a chat with it (`[access] message`) or it's on one of `teams`.
/// A call that says no standing makes what keeps it, and a call's own
/// never goes above it. Its one source: from the spec and the teams.
pub fn floor_of(spec: &Spec, id: Option<&str>, teams: &[Team]) -> Flags {
    let shared = spec
        .access
        .as_ref()
        .and_then(|access| access.message.as_ref())
        .is_some_and(|chats| !chats.is_empty());
    let on_team = id.is_some_and(|id| teams.iter().any(|team| team.has(id)));
    Flags {
        floor: Some(match shared || on_team {
            true => FlagsFloor::Member,
            false => FlagsFloor::Owner,
        }),
        ..Flags::default()
    }
}

/// `serve`'s flags for agent `id` on `teams`, from its spec: its caps
/// (`[tool.scheduler]`), its teams, then its floor ([`floor_of`]).
pub fn serve_flags(spec: &Spec, id: &str, teams: &[Team]) -> anyhow::Result<Flags> {
    Ok(caps(&spec.tool("scheduler"))?
        .with(self::teams(id, teams))
        .with(floor_of(spec, Some(id), teams)))
}

/// `run`'s flags for agent `id` on `teams`: [`serve_flags`], then its
/// `--timeout`, which `run` requires: an error when the spec sets no
/// `[tool.scheduler] firing`.
pub fn run_flags(spec: &Spec, id: &str, teams: &[Team]) -> anyhow::Result<Flags> {
    let scheduler = spec.tool("scheduler");
    let timeout = timeout(&scheduler)?.context("its spec sets no [tool.scheduler] firing")?;
    Ok(serve_flags(spec, id, teams)?.with(timeout))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Link's spec alone: every cap its `[tool.scheduler]` sets.
    #[test]
    fn links_spec_gives_every_flag() {
        let spec = crate::agent_spec::link();
        let scheduler = spec.tool("scheduler");
        assert_eq!(
            run_flags(&spec, "a1b2c3d4e5f6", &[]).unwrap().argv(),
            [
                "--floor",
                "owner",
                "--hops",
                "8",
                "--interval",
                "5m",
                "--late",
                "run",
                "--per-day",
                "100",
                "--subagents",
                "5",
                "--time-zone",
                "local",
                "--timeout",
                "30m"
            ]
        );
        assert_eq!(
            allowed(&scheduler).unwrap(),
            [
                "cancel",
                "compact",
                "list",
                "message",
                "read",
                "schedule",
                "schedules",
                "send",
                "task",
                "unschedule"
            ]
        );
        // A recurring schedule asks, allowed or not, unless it says once.
        assert_eq!(crate::asks().unwrap(), ["schedule.recurring"]);
    }

    /// A recurring schedule asks by its argument unless the spec lets it
    /// through (`[tool.scheduler] recurring = "allow"`), which takes its
    /// `--ask-if` away and leaves a whole call's ask (`ask = true`) as it
    /// was; the hint its refusal gives names that setting.
    #[test]
    fn a_recurring_schedule_asks_unless_the_spec_allows_it() {
        let spec_of = |text: &str| {
            let own = crate::agent_spec::parse(text).unwrap();
            let known = crate::agent_spec::Known { name: "max".into() };
            crate::agent_spec::resolve(Some(&own), &[], &known)
                .unwrap()
                .spec
                .tool("scheduler")
        };
        let link = crate::agent_spec::link().tool("scheduler");
        assert_eq!(ask_if(&link).unwrap(), ["schedule.recurring"]);
        let allow = spec_of("schema = 1\n[tool.scheduler]\nrecurring = \"allow\"\n");
        assert!(ask_if(&allow).unwrap().is_empty());
        assert_eq!(allowed(&allow).unwrap(), allowed(&link).unwrap());
        let whole = spec_of("schema = 1\n[tool.scheduler]\nrecurring = \"allow\"\nask = true\n");
        assert!(ask_if(&whole).unwrap().is_empty());
        assert!(!allowed(&whole).unwrap().contains(&"schedule".to_string()));
        assert!(
            asking()
                .hint
                .contains("[tool.scheduler] recurring = \"allow\"")
        );
        assert!(!asking().hint.contains("--add"));
        // What the sandbox is given: the ask and its hint by Link's spec,
        // neither with "allow".
        let given = sandbox_flags(&link).unwrap();
        let tail = &given[given.len() - 4..];
        assert_eq!(
            tail,
            [
                "--ask-if-hint",
                asking().hint.as_str(),
                "--ask-if",
                "schedule.recurring"
            ]
        );
        let given = sandbox_flags(&allow).unwrap();
        assert!(
            !given.iter().any(|flag| flag.starts_with("--ask-if")),
            "{given:?}"
        );
        assert!(given.windows(2).any(|pair| pair == ["--add", "schedule"]));
    }

    /// An agent's own spec over Link's: its numbers, its zone, and
    /// asking first.
    #[test]
    fn an_agents_spec_sets_its_own() {
        let own = crate::agent_spec::parse(
            "schema = 1\n[tool.scheduler]\nper_day = 7\nsubagents = 2\nhops = 3\nlate = \"skip\"\n\
             zone = \"Europe/Paris\"\ninterval = \"15m\"\nfiring = \"5m\"\nask = true\n",
        )
        .unwrap();
        let known = crate::agent_spec::Known { name: "max".into() };
        let spec = crate::agent_spec::resolve(Some(&own), &[], &known).unwrap();
        let scheduler = spec.tool("scheduler");
        assert_eq!(
            run_flags(&spec.spec, "a1b2c3d4e5f6", &[]).unwrap().argv(),
            [
                "--floor",
                "owner",
                "--hops",
                "3",
                "--interval",
                "15m",
                "--late",
                "skip",
                "--per-day",
                "7",
                "--subagents",
                "2",
                "--time-zone",
                "Europe/Paris",
                "--timeout",
                "5m"
            ]
        );
        assert_eq!(
            serve_flags(&spec.spec, "a1b2c3d4e5f6", &[])
                .unwrap()
                .argv()
                .len(),
            14
        );
        // The reads, and the harness's own `compact`.
        assert_eq!(
            allowed(&scheduler).unwrap(),
            ["list", "read", "schedules", "compact"]
        );
    }

    /// A spec missing a cap is refused by name; `serve` needs no firing.
    #[test]
    fn a_missing_cap_is_named() {
        let mut scheduler = crate::agent_spec::link().tool("scheduler");
        scheduler.firing = None;
        assert!(caps(&scheduler).is_ok());
        assert!(timeout(&scheduler).unwrap().is_none());
        scheduler.hops = None;
        let err = caps(&scheduler).unwrap_err();
        assert!(format!("{err}").contains("hops"), "{err}");
    }

    /// The agent's floor: owner while only its owner messages it, member
    /// once its spec shares a chat with it or it's on a team; a team it
    /// isn't on leaves it owner.
    #[test]
    fn the_floor_is_the_least_anyone_reaches_the_agent_with() {
        let floor = |spec: &Spec, teams: &[Team]| floor_of(spec, Some("a1b2c3d4e5f6"), teams).floor;
        let owners = crate::agent_spec::link();
        assert_eq!(floor(&owners, &[]), Some(FlagsFloor::Owner));
        let own =
            crate::agent_spec::parse("schema = 1\n[access]\nmessage = [\"chat:slack:C0TEAM\"]\n")
                .unwrap();
        let known = crate::agent_spec::Known { name: "max".into() };
        let shared = crate::agent_spec::resolve(Some(&own), &[], &known)
            .unwrap()
            .spec;
        assert_eq!(floor(&shared, &[]), Some(FlagsFloor::Member));
        let team = |member: &str| {
            crate::agent_spec::parse_team(&format!(
                "schema = 1\nid = \"t1t2t3t4t5t6\"\nname = \"research\"\n[member.max]\nid = \"{member}\"\n"
            ))
            .unwrap()
        };
        assert_eq!(
            floor(&owners, &[team("a1b2c3d4e5f6")]),
            Some(FlagsFloor::Member)
        );
        assert_eq!(
            floor(&owners, &[team("b1b2c3d4e5f6")]),
            Some(FlagsFloor::Owner)
        );
        let serve = serve_flags(&shared, "a1b2c3d4e5f6", &[]).unwrap();
        assert_eq!(serve.floor, Some(FlagsFloor::Member));
    }

    /// Its teams: the agent, each team it's on and the others in it; a
    /// team it isn't on, nothing.
    #[test]
    fn teams_name_the_agent_its_teams_and_teammates() {
        let team = crate::agent_spec::parse_team(
            "schema = 1\nid = \"t1t2t3t4t5t6\"\nname = \"research\"\n\
             [member.max]\nid = \"a1b2c3d4e5f6\"\n[member.ada]\nid = \"b1b2c3d4e5f6\"\n",
        )
        .unwrap();
        assert_eq!(
            teams("a1b2c3d4e5f6", std::slice::from_ref(&team)).argv(),
            [
                "--agent-id",
                "a1b2c3d4e5f6",
                "--team-id",
                "research=t1t2t3t4t5t6",
                "--teammate",
                "research/ada=b1b2c3d4e5f6"
            ]
        );
        assert_eq!(teams("c1b2c3d4e5f6", &[team]), Flags::default());
        assert!(teams("a1b2c3d4e5f6", &[]).argv().is_empty());
    }

    /// The team relay: the first relayed team's relay, and each team the
    /// agent is on that it carries, by name and owner; a team on no relay,
    /// on another relay or without the agent, nothing.
    #[test]
    fn the_relay_names_the_teams_it_carries() {
        let team = |id: &str, name: &str, member: &str| {
            crate::agent_spec::parse_team(&format!(
                "schema = 1\nid = \"{id}\"\nname = \"{name}\"\n[member.max]\nid = \"{member}\"\n"
            ))
            .unwrap()
        };
        const MAX: &str = "a1b2c3d4e5f6";
        const OWNER: &str = "acct-0123456789abcdef0123456789abcdef";
        let teams = [
            team("t1t2t3t4t5t6", "research", MAX),
            team("u1u2u3u4u5u6", "local", MAX),
            team("v1v2v3v4v5v6", "writers", MAX),
            team("w1w2w3w4w5w6", "elsewhere", MAX),
            team("x1x2x3x4x5x6", "theirs", "b1b2c3d4e5f6"),
        ];
        let carried = |id: &str, relay: &str| RelayedTeam {
            id: id.into(),
            owner: OWNER.into(),
            relay: relay.into(),
        };
        let relayed = [
            carried("t1t2t3t4t5t6", "https://dispatch.example"),
            carried("v1v2v3v4v5v6", "https://dispatch.example"),
            carried("w1w2w3w4w5w6", "https://other.example"),
            carried("x1x2x3x4x5x6", "https://dispatch.example"),
        ];
        assert_eq!(
            relay(MAX, &teams, &relayed).argv(),
            [
                "--team-owner",
                &format!("research={OWNER}"),
                "--team-owner",
                &format!("writers={OWNER}"),
                "--team-relay",
                "https://dispatch.example"
            ]
        );
        assert!(relay(MAX, &teams[1..2], &relayed).argv().is_empty());
        assert!(relay(MAX, &teams, &[]).argv().is_empty());
        assert!(relay("c1b2c3d4e5f6", &teams, &relayed).argv().is_empty());
    }
}
