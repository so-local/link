//! `lnk agent list`: every agent, this machine's and its boxes', and
//! each as JSON.

use link_agent::{
    moved, service,
    settings::{self, Settings, ids},
};
use link_format::{
    Done, Money, Name, Out, Path, Report, Sentence, Size, Table, Value, cmd, sentence, states,
};

use crate::permissions::{beyond_defaults, granted_spec};
use crate::services::measured;

/// One agent, as `lnk agent list --json` and `show --json` print it;
/// `machine`, this one's.
/// `running`: whether it runs ([`service::running_anywhere`]), and, when
/// it does, the processes it runs as ([`service::processes`]).
pub(crate) fn agent_json(
    agent: &Settings,
    machine: Option<link_machine::installation::Installation>,
    running: Option<serde_json::Value>,
) -> serde_json::Value {
    // Each connection's bot by its id, never a token.
    let bots: serde_json::Map<_, _> = agent
        .connections
        .iter()
        .map(|(name, connection)| {
            let bot = serde_json::json!({
                "id": connection.bot.id,
                "name": connection.bot.name,
                "team": connection.bot.team,
                "title": connection.about.title,
            });
            (name.clone(), bot)
        })
        .collect();
    let mut json = serde_json::json!({
        "workload": agent.id.as_ref().map(ToString::to_string),
        "principal": agent.service.as_deref().map(|s| ids::SERVICES.written(s)),
        // Whose it is, by its kind: an owner its settings hold is one.
        "owner": agent
            .owner
            .as_deref()
            .and_then(|owner| link_machine::owner::Owner::of(owner).ok()),
        "name": agent.name,
        "harness": agent.harness(),
        // The model it answers with, by name (its provider's when it has
        // none); null before its first start.
        "model": agent
            .model
            .as_ref()
            .map(|m| m.name.clone().unwrap_or_else(|| m.provider.clone())),
        "machine": machine.as_ref().map(machine_json),
        "running": running.is_some(),
        // Its own folder (`~/Link/Agents/<name>`), and its files.
        "folder": link_agent_contract::agents_dir().join(&agent.name),
        "files": agent.safe_files(),
        "port": agent.port,
        "connections": agent.connections(),
        "bots": bots,
        "location": null,
        // What it runs as, and where its harness puts its own measures,
        // for `lnk measure`.
        "processes": running.unwrap_or_else(|| serde_json::json!({"pids": [], "group": null, "groups": []})),
        "measures": link_agent::adapter::measures_of(&agent.box_of(agent.harness())),
        // Where this module times its starts and stops, and its teams,
        // for `lnk measure`.
        "times": link_agent::times::path_of(&agent.name),
        "teams": teams_json(agent),
    });
    // What it may do here on its harness, both layers, and of that,
    // what's beyond Link's defaults.
    let harness = agent.harness();
    json["spec"] = serde_json::to_value(granted_spec(agent, harness)).unwrap_or_default();
    json["beyond_defaults"] = serde_json::json!(beyond_defaults(agent, harness));
    json
}

/// The teams whose files list `agent`, each `{"resource": "group:<id>"}`:
/// none while a team's file can't be read.
pub(crate) fn teams_json(agent: &Settings) -> Vec<serde_json::Value> {
    let Some(id) = agent.agent_id() else {
        return Vec::new();
    };
    settings::teams_of(id)
        .unwrap_or_default()
        .into_iter()
        .map(|team| serde_json::json!({"resource": link_agent_team::resource(&team.id)}))
        .collect()
}

/// A machine as an agent's JSON names it: `{"resource": "installation:<id>",
/// "name"}`, never its saved file's bare `id`.
pub(crate) fn machine_json(
    machine: &link_machine::installation::Installation,
) -> serde_json::Value {
    serde_json::json!({"resource": machine.resource(), "name": machine.name})
}

/// Where an agent went, as its JSON names it: a box by its machine's
/// ID, `box:<id>` as `lnk box list --json` writes it, when it's known,
/// else a cloud account by its name.
fn place_json(place: &moved::Place) -> serde_json::Value {
    let mut json = serde_json::json!({
        "resource": place.resource(),
        "name": place.name,
    });
    if place.cloud {
        json["cloud"] = true.into();
    }
    if let Some(owner) = &place.owner {
        json["owner"] = owner.as_str().into();
    }
    json
}

/// The ID of the agent `agent` (one of `lnk agent list --json`'s, or
/// `show --json`), from its `"workload": "agent:<id>"`.
pub(crate) fn workload_of(agent: &serde_json::Value) -> Option<String> {
    agent["workload"]
        .as_str()
        .and_then(|w| link_id::candidate_of(ids::KIND, w))
        .map(str::to_string)
}

/// The service the agent `agent` (one of `lnk agent list --json`'s) acts
/// as, its ID, from its `"principal": "service:<id>"`.
pub(crate) fn service_of(agent: &serde_json::Value) -> Option<String> {
    agent["principal"]
        .as_str()
        .and_then(|p| link_id::candidate_of(ids::SERVICE, p))
        .map(str::to_string)
}

/// The installation ID of the machine an agent's JSON names, from its
/// `"machine": {"resource": "installation:<id>"}`.
pub(crate) fn machine_of(agent: &serde_json::Value) -> Option<String> {
    agent["machine"]["resource"]
        .as_str()
        .and_then(|m| link_id::candidate_of(link_machine::installation::KIND, m))
        .map(str::to_string)
}

/// An agent that moved away, as its note here says: its machine's to say
/// the rest, when it answers. `location` is that machine too.
pub(crate) fn moved_json(note: &moved::Note) -> serde_json::Value {
    serde_json::json!({
        "workload": note.id.as_deref().map(ids::written),
        "name": note.name,
        "harness": null,
        "model": null,
        "machine": place_json(&note.to),
        "running": null,
        "files": null,
        "port": null,
        "connections": null,
        "bots": null,
        "location": note.to.name,
        "processes": null,
        "measures": null,
        "times": null,
        "teams": null,
        "spec": null,
        "beyond_defaults": null,
    })
}

/// Every agent: this machine's, and, unless `here_only`, each running
/// box's (`lnk box list --json`, then `lnk agent list --json --here`
/// there), with the boxes that couldn't be asked. An agent that moved
/// from here is the machine's it went to to say; while that doesn't
/// answer, its note here says where it went.
/// `ask_clouds`: find boxes made elsewhere too, asking every cloud
/// account, rather than only the boxes this machine knows.
pub(crate) fn gather(here_only: bool, ask_clouds: bool) -> anyhow::Result<Gathered> {
    let machine = link_machine::installation::installation().ok();
    let names = settings::names()?;
    // Every agent's service asked at once, and the running ones'
    // processes at once.
    let running = service::running_anywhere_each(&names);
    let up: Vec<String> = names
        .iter()
        .zip(&running)
        .filter(|(_, running)| **running)
        .map(|(name, _)| name.clone())
        .collect();
    let mut processes = service::processes_each(&up).into_iter();
    let mut agents = Vec::new();
    for (name, running) in names.iter().zip(running) {
        let running = running.then(|| processes.next().unwrap_or_default());
        agents.push(agent_json(
            &Settings::load_without_keys(name)?,
            machine.clone(),
            running,
        ));
    }
    let mut unreached = Vec::new();
    let mut answered = Vec::new();
    if here_only {
        return Ok(Gathered {
            agents,
            unreached,
            answered,
            unreadable: Vec::new(),
        });
    }
    let boxes = boxes(ask_clouds);
    // Every running box asked at once, each over its own SSH connection.
    // Each answer, or none; and whether the box has no harness module.
    let answers: Vec<(bool, Option<serde_json::Value>)> = std::thread::scope(|scope| {
        let asking: Vec<_> = boxes
            .iter()
            .map(|b| {
                let name = b.name.clone();
                let running = b.state == "running";
                scope.spawn(move || {
                    if !running {
                        return (false, None);
                    }
                    let output = link_core::lnk()
                        .args([
                            "box", "run", &name, "--", "agent", "list", "--json", "--here",
                        ])
                        .stdin(std::process::Stdio::null())
                        .output()
                        .ok();
                    // A box without the harness module runs no agent: it
                    // starts with lnk alone, until an agent moves there.
                    let bare = output
                        .as_ref()
                        .is_some_and(|o| link_core::modules::not_installed(&o.status));
                    let listed = output
                        .filter(|o| o.status.success())
                        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
                    (bare, listed)
                })
            })
            .collect();
        asking
            .into_iter()
            .map(|asked| asked.join().unwrap_or((false, None)))
            .collect()
    });
    for (b, (bare, listed)) in boxes.iter().zip(answers) {
        let name = b.name.clone();
        let mut miss = |error: String| {
            unreached.push(serde_json::json!({
                "resource": b.resource(), "name": name, "error": error,
            }));
        };
        if b.state != "running" {
            let state = match b.state.is_empty() {
                true => "not running",
                false => &b.state,
            };
            miss(format!("it's {state}"));
            continue;
        }
        if bare {
            continue;
        }
        match listed {
            Some(listed) => {
                // The box by the name you know it by, whatever it calls itself.
                agents.extend(
                    listed["agents"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .cloned()
                        .map(|mut agent| {
                            agent["machine"]["name"] = name.clone().into();
                            agent
                        }),
                );
            }
            None => miss("it didn't answer lnk agent list".into()),
        }
    }
    // The agents each of your cloud accounts that hosts agents runs, by
    // what its adapter says, checked.
    let owner = link_core::owner::account_owner();
    for (account, hosted) in link_agent::hosted::listed(owner.as_deref()) {
        // An answer without its agents says nothing of them: unreached,
        // as one that failed is.
        let hosted = hosted.and_then(|hosted| {
            hosted.ok_or_else(|| anyhow::anyhow!("it answered without its agents"))
        });
        let hosted = match hosted {
            Ok(hosted) => hosted,
            Err(err) => {
                unreached.push(serde_json::json!({
                    "resource": null, "name": account.name, "cloud": true,
                    "error": link_format::clean(&format!("{err:#}")),
                }));
                continue;
            }
        };
        answered.push(serde_json::json!({"name": account.name, "cloud": true}));
        for agent in hosted {
            agents.push(serde_json::json!({
                "workload": link_id::is_candidate(&agent.id).then(|| ids::written(&agent.id)),
                "name": agent.name,
                "harness": null,
                "model": null,
                "machine": {"name": account.name, "cloud": true},
                "running": agent.state() == "running",
                "state": agent.state(),
                "files": null,
                "port": null,
                "connections": null,
                "bots": null,
                "location": null,
                "processes": null,
                "measures": null,
                "times": null,
                "teams": null,
                "spec": null,
                "beyond_defaults": null,
            }));
        }
    }
    let moved::Read {
        notes,
        unreadable: unread,
    } = moved::listed()?;
    let unreadable = unread.iter().map(|err| format!("{err:#}")).collect();
    for mut note in notes {
        let listed = agents.iter().find(|a| match &note.id {
            Some(id) => workload_of(a).as_ref() == Some(id),
            // A note without an ID (its box was silent when it was made): the box says it.
            None => a["name"] == note.name.as_str() && a["machine"]["name"] == note.to.name,
        });
        match (listed, &note.id) {
            (Some(agent), None) => {
                if let Some(id) = workload_of(agent) {
                    note.to.id = machine_of(agent);
                    let _ = note.learn_id(&id);
                }
            }
            (Some(_), Some(_)) => {}
            (None, _) => agents.push(moved_json(&note)),
        }
    }
    Ok(Gathered {
        agents,
        unreached,
        answered,
        unreadable,
    })
}

/// What [`gather`] found: every agent, the machines and accounts it
/// couldn't ask, and each cloud account that runs agents and answered
/// with its agents, there or not (`{"name", "cloud": true}`).
pub(crate) struct Gathered {
    pub agents: Vec<serde_json::Value>,
    pub unreached: Vec<serde_json::Value>,
    pub answered: Vec<serde_json::Value>,
    /// Why each note of an agent that moved away couldn't be read, naming
    /// its file: the rest are listed all the same.
    pub unreadable: Vec<String>,
}

/// Each agent this machine moved to a cloud account that runs agents,
/// by its note: `{"workload", "name", "account", "home"}`, `home` the folder
/// here its Home is pushed from and pulled to: what `lnk agent list
/// --json --here` and this module's answer to `storage.folders` say.
pub(crate) fn hosted_here() -> anyhow::Result<Vec<serde_json::Value>> {
    let mut notes: Vec<moved::Note> = moved::notes()?
        .into_iter()
        .filter(|note| note.to.cloud && note.id.is_some())
        .collect();
    notes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(notes
        .iter()
        .map(|note| {
            serde_json::json!({
                "workload": note.id.as_deref().map(ids::written),
                "name": note.name,
                "account": note.to.name,
                "home": settings::home(&note.name),
            })
        })
        .collect())
}

/// Your boxes (`lnk box list --json`): those this machine knows, or with
/// `ask_clouds` those your clouds know too; none when it fails (no box
/// module), or on a box, which knows no other.
pub(crate) fn boxes(ask_clouds: bool) -> Vec<link_box_contract::Listed> {
    crate::moving::machines::boxes(!ask_clouds).unwrap_or_default()
}

states! {
    /// An agent's state in the list: its service's, or its cloud's word
    /// where it is hosted.
    pub(crate) enum ListedState {
        Running("running") => Good,
        Stopped("stopped") => Quiet,
    }
}

/// `lnk agent list`: every agent, here and on your boxes.
pub(crate) fn list_agents(
    out: &mut Out,
    json: bool,
    here: bool,
    refresh: bool,
) -> Result<Done, Report> {
    let Gathered {
        agents,
        unreached,
        answered,
        unreadable,
    } = gather(here, refresh)?;
    if json {
        let mut listed =
            serde_json::json!({"agents": agents, "unreached": unreached, "answered": answered});
        if here {
            listed["in_clouds"] = hosted_here()?.into();
        }
        link_actions::answer::reply::<link_agent_spec::declared::LifeAgents>(out, &listed)?;
    }
    // The boxes that couldn't be asked, by the name you know each by.
    let text = |v: &serde_json::Value| link_format::clean(v.as_str().unwrap_or("-"));
    for b in &unreached {
        out.block(
            Report::warning(sentence!(
                "Couldn't ask {}.",
                Name::new("box", text(&b["name"]))
            ))
            .why(Sentence::adopt(&text(&b["error"]))),
        );
    }
    // The notes of agents that moved away that couldn't be read, each by
    // its file: listed without them.
    for why in &unreadable {
        out.block(
            Report::warning(sentence!(
                "Couldn't read where an agent that moved away went."
            ))
            .why(Sentence::adopt(&link_format::clean(why))),
        );
    }
    // Measures are only read for agents to show them beside.
    let measured = match agents.is_empty() {
        true => Default::default(),
        false => measured(),
    };
    let mut table = Table::new(
        [
            "Name",
            "Harness",
            "Machine",
            "State",
            "Memory",
            "Spend (day)",
            "Files",
            "Connections",
        ],
        sentence!("No agent here yet."),
    );
    for agent in &agents {
        // By its ID.
        let (memory, spend) = workload_of(agent)
            .and_then(|id| measured.get(&id))
            .copied()
            .unwrap_or_default();
        let memory = match memory.filter(|_| agent["running"] == true) {
            Some(bytes) => Value::Size(Size(bytes as u64)),
            None => Value::from("-"),
        };
        // Micro-dollars.
        let spend = match spend {
            Some(s) => Value::Money(Money {
                cents: (s / 1e4).round() as u64,
                per: None,
            }),
            None => Value::from("-"),
        };
        let hosted = agent["machine"]["cloud"] == true;
        let state: Value = match (&agent["running"], agent["location"].as_str()) {
            (_, Some(place)) if hosted => Value::from(format!(
                "on your cloud account {}",
                link_format::clean(place)
            )),
            (_, Some(place)) => Value::from(format!("on your box {}", link_format::clean(place))),
            (serde_json::Value::Bool(true), _) => ListedState::Running.into(),
            // A cloud's word for it, shown as text.
            _ if hosted => Value::from(text(&agent["state"])),
            _ => ListedState::Stopped.into(),
        };
        // Each connection by its title, else its name.
        let bots: Vec<Value> = agent["connections"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str())
            .map(|c| {
                let title = agent["bots"][c]["title"].as_str().filter(|t| !t.is_empty());
                Value::from(link_format::clean(title.unwrap_or(c)))
            })
            .collect();
        let files = match agent["files"].as_str() {
            Some(files) => Value::Path(Path::new(files)),
            None => Value::from("-"),
        };
        table.row([
            Name::new("agent", text(&agent["name"])).into(),
            Value::from(text(&agent["harness"])),
            Value::from(text(&agent["machine"]["name"])),
            state,
            memory,
            spend,
            files,
            Value::List(bots),
        ]);
    }
    let empty = agents.is_empty() && unreached.is_empty();
    out.block(table);
    Ok(match empty {
        true => Done::shown().next(cmd!("lnk agent start"), "make the first, main"),
        false => Done::shown()
            .next(cmd!("lnk agent create <name>"), "make another")
            .next(cmd!("lnk agent show <name>"), "look at one"),
    })
}
