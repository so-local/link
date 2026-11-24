//! The task namespace's commands as the owner types them, beside their
//! JSON: `/cron:create 10m check the build` (and `/loop`, its alias),
//! `/task:stop --all` or a kind of work, and `/task:list --agents`. Each
//! is read into the requests its providers already take: a schedule's
//! `cron`, one `stop` for each thing listed, and the list's subagents.

use anyhow::{bail, ensure};
use link_format::WithinExt;
use serde_json::{Value, json};

/// A kind of the agent's work, as `/task:stop` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// Background commands (`tasks`): the run tool's.
    Commands,
    /// Subagents (`agents`): the scheduler's tasks.
    Agents,
    /// Schedules (`schedules`): the scheduler's.
    Schedules,
}

impl Work {
    /// Each kind, in the order they're stopped.
    pub const ALL: [Work; 3] = [Work::Commands, Work::Agents, Work::Schedules];

    /// The word `/task:stop` takes for it.
    pub fn word(self) -> &'static str {
        match self {
            Self::Commands => "tasks",
            Self::Agents => "agents",
            Self::Schedules => "schedules",
        }
    }
}

/// What `/task:stop`'s words ask: all the agent's work, or one kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopping {
    All,
    Kind(Work),
}

impl Stopping {
    /// Whether it stops `work`.
    pub fn stops(self, work: Work) -> bool {
        match self {
            Self::All => true,
            Self::Kind(kind) => kind == work,
        }
    }
}

/// What `/task:stop`'s `arguments` ask, when they're words, not JSON:
/// `--all`, `tasks`, `agents` or `schedules`. None for JSON, which goes
/// to the providers as it is.
pub fn stopping(arguments: &str) -> Option<anyhow::Result<Stopping>> {
    let words = arguments.trim();
    if words.is_empty() || words.starts_with('{') {
        return None;
    }
    Some(match words {
        "--all" | "all" => Ok(Stopping::All),
        word => Work::ALL
            .into_iter()
            .find(|work| work.word() == word)
            .map(Stopping::Kind)
            .within("stop --all, or one of tasks, agents and schedules"),
    })
}

/// Whether `/task:list`'s `arguments` ask for the subagents alone.
pub fn agents_only(arguments: &str) -> bool {
    arguments.trim() == "--agents"
}

/// `/cron:create`'s request, from `arguments` that are words: a
/// duration, then the message sent every so often (`10m check the
/// build`). None for JSON, which goes to the scheduler as it is.
pub fn schedule(arguments: &str) -> Option<anyhow::Result<Value>> {
    let words = arguments.trim();
    if words.is_empty() || words.starts_with('{') {
        return None;
    }
    let (every, text) = words.split_once(char::is_whitespace).unwrap_or((words, ""));
    Some((|| {
        let cron = cron_every(every)?;
        let text = text.trim();
        ensure!(
            !text.is_empty(),
            "what's sent every {every}? Say it after the interval"
        );
        Ok(json!({"prompt": text, "cron": cron}))
    })())
}

/// The cron of every `every`: `5m` every five minutes from the hour,
/// `2h` every two hours on the hour, `1d` each midnight, in the agent's
/// time zone. A duration cron can't keep evenly is refused.
pub fn cron_every(every: &str) -> anyhow::Result<String> {
    let split = every
        .find(|c: char| !c.is_ascii_digit())
        .within("an interval first: 10m, 2h or 1d")?;
    let (n, unit) = every.split_at(split);
    let n: u32 = n.parse().within("an interval first: 10m, 2h or 1d")?;
    Ok(match unit {
        "m" if n > 0 && 60 % n == 0 => format!("*/{n} * * * *"),
        "h" if n > 0 && 24 % n == 0 => format!("0 */{n} * * *"),
        "d" if n == 1 => "0 0 * * *".into(),
        "m" | "h" | "d" => bail!(
            "{every} can't be kept evenly: minutes that divide an hour, hours that divide a \
             day, or 1d; a cron for others"
        ),
        _ => bail!("an interval first: 10m, 2h or 1d"),
    })
}

/// One thing of the agent's work, as its provider names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub work: Work,
    /// The provider's kind in the task namespace: `run` or `schedule`.
    pub provider: &'static str,
    pub id: String,
    /// Still running or due: what stopping would end.
    pub going: bool,
}

/// What `task list` answered, combined or of one provider: each
/// background command, subagent and schedule.
pub fn listed(answer: &Value) -> Vec<Listed> {
    let structured = &answer["structuredContent"];
    let results = match structured.get("results") {
        Some(results) => results.clone(),
        None => {
            let provider = match structured.get("commands") {
                Some(_) => "run",
                None => "schedule",
            };
            json!({ provider: answer })
        }
    };
    let mut all = Vec::new();
    for command in results["run"]["structuredContent"]["commands"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(id) = command["id"].as_str() {
            all.push(Listed {
                work: Work::Commands,
                provider: "run",
                id: id.into(),
                going: command["state"] == "running",
            });
        }
    }
    for job in results["schedule"]["structuredContent"]["jobs"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let work = match job["kind"].as_str() {
            Some("task") => Work::Agents,
            Some("schedule") => Work::Schedules,
            _ => continue,
        };
        if let Some(id) = job["id"].as_str() {
            all.push(Listed {
                work,
                provider: "schedule",
                id: id.into(),
                going: true,
            });
        }
    }
    all
}

/// The `task stop` request for `listed`, in `thread`.
pub fn stop_request(listed: &Listed, thread: &str) -> Value {
    json!({"kind": listed.provider, "id": listed.id, "thread": thread})
}

/// `task list`'s `answer` with its subagents alone.
pub fn agents_of(answer: &Value) -> Value {
    let agents: Vec<Value> = listed(answer)
        .into_iter()
        .filter(|l| l.work == Work::Agents)
        .map(|l| json!({"id": l.id, "thread": format!("task-{}", l.id)}))
        .collect();
    json!({"agents": agents})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schedule_is_an_interval_and_its_message() {
        let asked = schedule("10m check the build").unwrap().unwrap();
        assert_eq!(
            asked,
            json!({"prompt": "check the build", "cron": "*/10 * * * *"})
        );
        assert_eq!(cron_every("2h").unwrap(), "0 */2 * * *");
        assert_eq!(cron_every("1d").unwrap(), "0 0 * * *");
        for uneven in ["7m", "5h", "2d", "0m", "m", "10", "10s"] {
            assert!(cron_every(uneven).is_err(), "{uneven}");
        }
        assert!(schedule("10m").unwrap().is_err());
        assert!(schedule("check the build").unwrap().is_err());
        assert!(schedule(r#"{"prompt": "a", "cron": "*/10 * * * *"}"#).is_none());
        assert!(schedule("").is_none());
    }

    #[test]
    fn stop_names_all_the_work_or_one_kind() {
        assert_eq!(stopping("--all").unwrap().unwrap(), Stopping::All);
        assert_eq!(
            stopping("agents").unwrap().unwrap(),
            Stopping::Kind(Work::Agents)
        );
        assert!(stopping("everything").unwrap().is_err());
        assert!(stopping(r#"{"kind": "run", "id": "a"}"#).is_none());
        assert!(stopping("").is_none());
        assert!(Stopping::All.stops(Work::Schedules));
        assert!(!Stopping::Kind(Work::Agents).stops(Work::Commands));
    }

    #[test]
    fn a_list_is_read_combined_or_of_one_provider() {
        let run = json!({"structuredContent": {"commands": [
            {"id": "r1", "state": "running"}, {"id": "r2", "state": "exited"}]}});
        let schedule = json!({"structuredContent": {"jobs": [
            {"id": "a1", "kind": "task"}, {"id": "s1", "kind": "schedule"},
            {"id": "m1", "kind": "send"}]}});
        let combined =
            json!({"structuredContent": {"results": {"run": run, "schedule": schedule}}});
        let all = listed(&combined);
        let works: Vec<(Work, &str, bool)> = all
            .iter()
            .map(|l| (l.work, l.id.as_str(), l.going))
            .collect();
        assert_eq!(
            works,
            [
                (Work::Commands, "r1", true),
                (Work::Commands, "r2", false),
                (Work::Agents, "a1", true),
                (Work::Schedules, "s1", true),
            ]
        );
        assert_eq!(listed(&run).len(), 2);
        assert_eq!(listed(&schedule).len(), 2);
        assert_eq!(
            stop_request(&all[2], "telegram:42"),
            json!({"kind": "schedule", "id": "a1", "thread": "telegram:42"})
        );
        assert_eq!(
            agents_of(&combined),
            json!({"agents": [{"id": "a1", "thread": "task-a1"}]})
        );
        assert!(agents_only(" --agents "));
    }
}
