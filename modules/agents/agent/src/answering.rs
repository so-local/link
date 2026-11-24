//! `lnk-agent --link-answer <question>`: what this module answers the modules
//! below it, each question theirs ([`link_core::answers`]): where a
//! secret's copies are, where an agent is and its own tokens (`lnk
//! secret`), the
//! folders this module keeps and an owner's ID by its name (`lnk
//! storage`), the agents running here (`lnk measure`), and an agent's
//! files checked and adopted from an archive (`lnk archive`).

use link_format::WithinExt;
use std::path::PathBuf;

use anyhow::bail;
use link_agent::{home, owners};
use link_archive_contract as archive;
use link_measure_contract::client::asked::{self as measure, Agent};
use link_secret_contract::{self as secrets, GranteeAt, Place, Reached};
use link_storage_contract as bucket;

use crate::listing;
use link_sandbox_client::grants;

/// Answers `question`, asked on stdin, on stdout.
pub(crate) fn answer(question: &str) -> anyhow::Result<()> {
    match question {
        q if q == secrets::COPIES.name => {
            let asked = secrets::COPIES.input()?;
            secrets::COPIES.answer(&copies(&asked))
        }
        q if q == REMOVED.name => {
            let gone = REMOVED.input()?;
            REMOVED.answer(&released(&gone))
        }
        q if q == secrets::GRANTEE.name => {
            let named = secrets::GRANTEE.input()?;
            secrets::GRANTEE.answer(&grantee_at(&named)?)
        }
        q if q == secrets::route::TOKEN.name => {
            let asked = secrets::route::TOKEN.input()?;
            secrets::route::TOKEN.answer(&token(&asked)?)
        }
        q if q == grants::GRANTS.name => {
            let asked = grants::GRANTS.input()?;
            grants::GRANTS.answer(&reach_now(&asked)?)
        }
        q if q == bucket::FOLDERS.name => {
            bucket::FOLDERS.input()?;
            bucket::FOLDERS.answer(&folders()?)
        }
        q if q == bucket::HOME.name => {
            let asked = bucket::HOME.input()?;
            bucket::HOME.answer(&home::run(&asked)?)
        }
        q if q == bucket::OWNER.name => {
            let asked = bucket::OWNER.input()?;
            bucket::OWNER.answer(&owner(&asked)?)
        }
        q if q == measure::AGENTS.name || q == measure::INVENTORY.name => {
            measure::AGENTS.input()?;
            let listing::Gathered { agents, .. } = listing::gather(true, false)?;
            let agents = measured(&serde_json::json!({ "agents": agents }));
            if q == measure::AGENTS.name {
                return measure::AGENTS.answer(&agents);
            }
            let traffic = link_sandbox_client::wire_dir()
                .ok()
                .map(|dir| link_measure_contract::client::traffic::Source {
                    provider: "sandbox".into(),
                    directory: link_sandbox_client::traffic_dir(&dir),
                })
                .into_iter()
                .collect();
            measure::INVENTORY.answer(&measure::Inventory { agents, traffic })
        }
        q if q == archive::CHECK.name => {
            let asked = archive::CHECK.input()?;
            archive::CHECK.answer(&link_agent::archives::check(&asked)?)
        }
        q if q == archive::RUNNING.name => {
            let asked = archive::RUNNING.input()?;
            archive::RUNNING.answer(&link_agent::archives::running(&asked)?)
        }
        q if q == archive::CARRY.name => {
            let asked = archive::CARRY.input()?;
            archive::CARRY.answer(&link_agent::archives::carry(&asked)?)
        }
        q if q == archive::MISSING.name => {
            let asked = archive::MISSING.input()?;
            archive::MISSING.answer(&link_agent::archives::missing(&asked)?)
        }
        q if q == archive::ADOPT.name => {
            let asked = archive::ADOPT.input()?;
            archive::ADOPT.answer(&link_agent::archives::adopt(&asked)?)
        }
        _ => bail!(
            "{:?} is no question this module answers",
            link_format::clean(question)
        ),
    }
}

/// What the agent of `asked`'s ID may reach now from its sandbox, for
/// the sandbox's bridge at each call: not this module's when no agent
/// here has that ID. The sandbox is `<agent>-<harness>`, a step's
/// profile adding `-<step>`: its harness is the longest of those that is
/// the agent's own harness or one with grants here, else the agent's own.
fn reach_now(asked: &grants::Asked) -> anyhow::Result<grants::Granted> {
    link_machine::machine::check_id(&asked.agent)?;
    for name in link_agent::settings::names()? {
        let Ok(settings) = link_agent::settings::Settings::load_without_keys(&name) else {
            continue;
        };
        if settings.agent_id() != Some(asked.agent.as_str()) {
            continue;
        }
        let harness = asked
            .sandbox
            .strip_prefix(&format!("{name}-"))
            .and_then(|rest| harness_of(rest, settings.harness()))
            .unwrap_or_else(|| settings.harness().to_string());
        return Ok(grants::Granted::Agent(crate::confine::reach_now(
            &settings, &harness,
        )));
    }
    Ok(grants::Granted::NotMine)
}

/// Of a sandbox's name past its agent's (`claude`, `claude-run`), the
/// harness it's of: the longest leading part that is `own` or a harness
/// with grants here.
fn harness_of(rest: &str, own: &str) -> Option<String> {
    let mut candidate = rest;
    loop {
        if candidate == own || link_agent::grants::exists(candidate) {
            return Some(candidate.to_string());
        }
        candidate = candidate.rsplit_once('-')?.0;
    }
}

/// `lnk box delete`'s question ([`link_box_contract::REMOVED`]), in Box's
/// types.
const REMOVED: link_core::answers::Question<
    link_box_contract::BoxIdentity,
    link_box_contract::Released,
> = link_core::answers::Question::new(
    link_box_contract::REMOVED,
    link_box_contract::REMOVED_PROTOCOL,
);

/// Box `gone` deleted: each agent this computer's note puts there (by its
/// name, and its installation ID where both know it) went with it, so the key
/// made for that box to reach its buckets is revoked (`lnk storage
/// revoke`) and its note ends. A key that won't go keeps its note, and is
/// said with what revokes it.
fn released(gone: &link_box_contract::BoxIdentity) -> link_box_contract::Released {
    let mut stays = Vec::new();
    // A note this computer can't read (a newer release's, or damaged) is
    // said, never taken as no note: its key may still reach the box.
    let notes = match link_agent::moved::notes() {
        Ok(notes) => notes,
        Err(err) => {
            stays.push(format!(
                "This computer's notes of where its agents went couldn't be read, so a key \
                 {} had to reach its buckets may still work: {err:#}",
                gone.name
            ));
            return link_box_contract::Released { stays };
        }
    };
    for note in notes {
        let on_it = !note.to.cloud
            && note.to.name == gone.name
            && match (&note.to.id, gone.machine_id.is_empty()) {
                (Some(noted), false) => *noted == gone.machine_id,
                _ => true,
            };
        if !on_it {
            continue;
        }
        if let Some(id) = &note.id
            && link_core::find_module("storage").is_some()
        {
            let revoked = link_agent::owners::agent_flag(id).is_ok_and(|owner| {
                link_core::lnk()
                    .args(["storage", "revoke"])
                    .args(owner)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            });
            if !revoked {
                stays.push(format!(
                    "The key {} had to reach {}'s buckets may still work: lnk storage revoke --owner agent:{id} revokes it.",
                    gone.name, note.name
                ));
                continue;
            }
        }
        note.remove();
    }
    link_box_contract::Released { stays }
}

/// Each copy of [`secrets::Copies`] reached ([`reach`]), in order: your
/// boxes asked once, when a copy on a box is first reached.
fn copies(asked: &secrets::Copies) -> Vec<Reached> {
    let boxes = std::cell::OnceCell::new();
    asked
        .copies
        .iter()
        .map(|copy| {
            reach(&boxes, &copy.place, &copy.owner)
                .unwrap_or_else(|err| Reached::Unreached(format!("{err:#}")))
        })
        .collect()
}

/// Your boxes, asked once when a copy on a box is first reached; none
/// when `lnk box list` can't say.
type Boxes = std::cell::OnceCell<Option<Vec<link_box_contract::Listed>>>;

/// The agent's own secret `owner` names, by the agent's service and its use,
/// reached where a move gave a copy of it: on a box, removed there (`lnk
/// box run <box> -- secret delete --here --service service:<id> --own
/// <use>`); on a cloud
/// account, gone once the account, listing the agents of the owner the
/// agent went there as, lists it no longer (it removes an agent's keys
/// with it), else held there with it. A box that's no longer yours (`lnk
/// box list` answers without it), or one made since under its name
/// (another installation ID), took the copy with it, and isn't touched. An
/// error says why a box couldn't be reached.
fn reach(boxes: &Boxes, place: &Place, owner: &secrets::Owner) -> anyhow::Result<Reached> {
    if place.cloud {
        return Ok(match hosted_without(place, Some(owner.principal.id())) {
            true => Reached::Removed,
            false => Reached::WithService,
        });
    }
    let listed = boxes.get_or_init(|| crate::moving::machines::boxes(false).ok());
    if let Some(listed) = listed {
        let Some(found) = listed.iter().find(|b| b.name == place.name) else {
            return Ok(Reached::Removed);
        };
        if let (Some(noted), Some(now)) = (&place.id, found.machine())
            && *noted != now
        {
            return Ok(Reached::Removed);
        }
    }
    let mut remove = link_core::lnk();
    remove.args([
        "box",
        "run",
        &place.name,
        "--",
        "secret",
        "delete",
        "--here",
        "--service",
        owner.principal.as_str(),
        "--own",
        &owner.r#as.flag(),
    ]);
    let out = remove
        .stdin(std::process::Stdio::null())
        .output()
        .within("running lnk box run")?;
    if out.status.success() {
        return Ok(Reached::Removed);
    }
    if link_core::modules::not_installed(&out.status) {
        bail!(
            "it has no secret module: lnk box run {} -- module add secret",
            place.name
        );
    }
    let said = String::from_utf8_lossy(&out.stderr);
    let why = said
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("lnk box run failed")
        .trim();
    bail!("{}", link_format::clean(why))
}

/// Whether cloud account `place`, signed in as the owner the agent `of`
/// went there as, answers with that owner's agents, and without it, as
/// the harness module lists them (`lnk agent list --json`). Any other
/// answer, or none, says nothing of it.
fn hosted_without(place: &Place, of: Option<&str>) -> bool {
    let (Some(of), Some(owner)) = (of, &place.owner) else {
        return false;
    };
    if link_core::owner::account_owner().as_ref() != Some(owner) {
        return false;
    }
    let listed = link_core::lnk()
        .args(["agent", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
    let Some(listed) = listed else {
        return false;
    };
    hosted_listing_without(&listed, &place.name, of)
}

/// Whether `lnk agent list --json`'s `listed` heard from cloud account
/// `account` (it answered with its agents, each naming the service it
/// acts as, and isn't unreached) and lists no agent acting as `of`, a
/// service's ID, there. An account it never asked, or one listing an
/// agent without its service, says nothing.
fn hosted_listing_without(listed: &serde_json::Value, account: &str, of: &str) -> bool {
    let on_account = |entry: &serde_json::Value| {
        entry["cloud"] == true && entry["name"] == account
            || entry["machine"]["cloud"] == true && entry["machine"]["name"] == account
    };
    let entries = |key: &str| listed[key].as_array().cloned().unwrap_or_default();
    let answered = entries("answered").iter().any(on_account);
    let unreached = entries("unreached").iter().any(on_account);
    let theirs: Vec<_> = entries("agents").into_iter().filter(on_account).collect();
    let named = theirs
        .iter()
        .all(|agent| listing::service_of(agent).is_some());
    let there = theirs
        .iter()
        .any(|agent| listing::service_of(agent).as_deref() == Some(of));
    listed["agents"].is_array() && answered && !unreached && named && !there
}

/// Where agent `name` is, as its entry in `lnk agent list --json` says.
fn grantee_at(named: &str) -> anyhow::Result<GranteeAt> {
    let Some(name) = named
        .strip_prefix(link_agent::settings::ids::KIND)
        .and_then(|rest| rest.strip_prefix(':'))
    else {
        return Ok(GranteeAt::Unkept);
    };
    link_machine::machine::check_name(name)?;
    let out = link_core::lnk()
        .args(["agent", "list", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .within("running lnk agent list")?;
    if !out.status.success() {
        return Ok(GranteeAt::None);
    }
    let listed: serde_json::Value =
        serde_json::from_slice(&out.stdout).within("reading lnk agent list --json")?;
    let Some(status) = listed["agents"]
        .as_array()
        .and_then(|agents| agents.iter().find(|agent| agent["name"] == name))
    else {
        return Ok(GranteeAt::None);
    };
    if let Some(on) = status["location"].as_str() {
        return Ok(GranteeAt::On {
            place: on.to_string(),
        });
    }
    Ok(
        match (listing::workload_of(status), listing::service_of(status)) {
            (Some(agent), Some(service)) => GranteeAt::Here {
                workload: link_id::Workload::of(link_agent::settings::ids::KIND, &agent)?,
                principal: secrets::service_principal(&service)?,
            },
            _ => GranteeAt::Unstarted {
                how: "start it once (lnk agent start)".into(),
            },
        },
    )
}

/// The own access token of the agent here whose ID is `asked`'s, for its
/// host and grants, as its machine signs for it ([`link_core::owner::TOKEN`]):
/// none when no agent here has that ID.
fn token(asked: &secrets::route::TokenAsk) -> anyhow::Result<Option<secrets::route::Token>> {
    link_machine::machine::check_id(&asked.workload)?;
    let mine = link_agent::settings::names()?.iter().any(|name| {
        link_agent::settings::Settings::load_without_keys(name)
            .is_ok_and(|s| s.agent_id() == Some(asked.workload.as_str()))
    });
    if !mine {
        return Ok(None);
    }
    let made = link_core::owner::signed_token(&link_core::owner::TokenAsk {
        audience: asked.host.clone(),
        workload: Some(asked.workload.clone()),
        grants: Some(asked.grants.clone()),
    })
    .map_err(|err| {
        anyhow::anyhow!(
            "its machine made it no token for {}: {err:#}",
            link_format::clean(&asked.host)
        )
    })?;
    Ok(Some(secrets::route::Token {
        token: made.token,
        expires_at: made.expires_at,
    }))
}

/// The folders this module keeps, each by its owner's ID
/// ([`owners`]): this machine's agents' files folders, each agent's Home
/// on a cloud account that hosts it, and each hosted team's folder, by
/// its group's ID.
fn folders() -> anyhow::Result<bucket::Folders> {
    let listing::Gathered { agents, .. } = listing::gather(true, false)?;
    let mut owned: Vec<bucket::Owned> = agents
        .iter()
        .filter_map(|a| {
            Some(bucket::Owned {
                owner: owners::agent(&listing::workload_of(a)?).ok()?,
                name: a["name"].as_str()?.to_string(),
                root: a["files"].as_str().map(PathBuf::from)?,
                running: a["running"].as_bool(),
                account: None,
            })
        })
        .collect();
    owned.extend(listing::hosted_here()?.iter().filter_map(|a| {
        Some(bucket::Owned {
            owner: owners::agent(&listing::workload_of(a)?).ok()?,
            name: a["name"].as_str()?.to_string(),
            root: a["home"].as_str().map(PathBuf::from)?,
            running: None,
            account: Some(a["account"].as_str()?.to_string()),
        })
    }));
    owned.extend(crate::teams::hosted_teams());
    Ok(bucket::Folders { owned })
}

/// `lnk storage`'s [`bucket::OWNER`]: the ID of the agent, or of the
/// hosted team's group, named `asked.name` or whose ID's id it is.
fn owner(asked: &bucket::OwnerName) -> anyhow::Result<bucket::Found> {
    let named = |id: &str, name: &str| asked.name == id || asked.name == name;
    match asked.kind.as_str() {
        owners::AGENT => {
            for name in link_agent::settings::names()? {
                let Ok(settings) = link_agent::settings::Settings::load_without_keys(&name) else {
                    continue;
                };
                let id = settings.agent_id().map(str::to_string).unwrap_or_default();
                if !named(&id, &name) {
                    continue;
                }
                return Ok(match settings.agent_id() {
                    Some(id) => bucket::Found::Here {
                        owner: owners::agent(id)?,
                        name,
                    },
                    None => bucket::Found::Unstarted,
                });
            }
            for hosted in listing::hosted_here()? {
                let (Some(id), Some(name)) =
                    (listing::workload_of(&hosted), hosted["name"].as_str())
                else {
                    continue;
                };
                if named(&id, name) {
                    return Ok(bucket::Found::Here {
                        owner: owners::agent(&id)?,
                        name: name.to_string(),
                    });
                }
            }
            Ok(bucket::Found::None)
        }
        owners::GROUP => Ok(crate::teams::hosted_teams()
            .into_iter()
            .find(|t| named(t.owner.id(), &t.name))
            .map_or(bucket::Found::None, |t| bucket::Found::Here {
                owner: t.owner,
                name: t.name,
            })),
        _ => Ok(bucket::Found::Unkept),
    }
}

/// The agents of `lnk agent list --json`'s `json` that run here, as
/// measure takes them.
fn measured(json: &serde_json::Value) -> Vec<Agent> {
    let mut agents = Vec::new();
    for agent in json["agents"].as_array().into_iter().flatten() {
        let Some(name) = agent["name"].as_str() else {
            continue;
        };
        let Some(id) = listing::workload_of(agent) else {
            continue;
        };
        let key = id.clone();
        if !agent["location"].is_null() || !link_measure_contract::plain(&id) {
            continue;
        }
        let own = agent["folder"].as_str().map(PathBuf::from);
        let mut folders: Vec<PathBuf> = own.iter().cloned().collect();
        if let Some(files) = agent["files"].as_str().map(PathBuf::from)
            && own.as_ref().is_none_or(|own| !files.starts_with(own))
        {
            folders.push(files);
        }
        let processes = &agent["processes"];
        // Each of its services' groups.
        let groups: Vec<PathBuf> = processes["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_str().map(PathBuf::from))
            .collect();
        agents.push(Agent {
            id,
            key,
            name: name.to_string(),
            owner: serde_json::from_value::<link_machine::owner::Owner>(agent["owner"].clone())
                .ok()
                .and_then(|o| o.checked().ok())
                .map(|o| o.text())
                .filter(|o| link_measure_contract::plain(o)),
            teams: agent["teams"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| {
                    t["resource"]
                        .as_str()
                        .and_then(|t| link_id::candidate_of(link_agent_team::KIND, t))
                        .filter(|t| link_measure_contract::plain(t))
                        .map(str::to_string)
                })
                .collect(),
            running: agent["running"] == true,
            pids: processes["pids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_u64())
                .filter_map(|p| u32::try_from(p).ok())
                .collect(),
            groups,
            measures: agent["measures"].as_str().map(PathBuf::from),
            times: agent["times"].as_str().map(PathBuf::from),
            folders,
        });
    }
    agents
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step's sandbox (`claude-run`) is of its agent's own harness.
    #[test]
    fn a_steps_sandbox_is_of_its_harness() {
        assert_eq!(harness_of("claude", "claude").as_deref(), Some("claude"));
        assert_eq!(
            harness_of("claude-run", "claude").as_deref(),
            Some("claude")
        );
        assert_eq!(
            harness_of("my-harness-setup", "my-harness").as_deref(),
            Some("my-harness")
        );
    }

    /// A hosted copy is gone only where the account answered with its
    /// agents, without the agent there.
    #[test]
    fn a_hosted_copy_is_gone_only_where_the_account_answered_without_it() {
        let agent = |id: &str, on: &str| serde_json::json!({"workload": "agent:a1b2c3d4e5f6", "principal": format!("service:{id}"), "machine": {"name": on, "cloud": true}});
        let answered = serde_json::json!([{"name": "cloud", "cloud": true}]);
        let listed = |agents: Vec<serde_json::Value>, unreached: Vec<serde_json::Value>| serde_json::json!({"agents": agents, "unreached": unreached, "answered": answered});
        let id = "abcdefabcdef";
        assert!(hosted_listing_without(&listed(vec![], vec![]), "cloud", id));
        assert!(!hosted_listing_without(
            &listed(vec![agent(id, "cloud")], vec![]),
            "cloud",
            id
        ));
        assert!(hosted_listing_without(
            &listed(vec![agent(id, "other")], vec![]),
            "cloud",
            id
        ));
        let silent = serde_json::json!({"name": "cloud", "cloud": true, "error": "down"});
        assert!(!hosted_listing_without(
            &listed(vec![], vec![silent]),
            "cloud",
            id
        ));
        // An account the list never asked (one it doesn't know hosts
        // agents): nothing said of it.
        // An account listing an agent without its service says nothing.
        let unnamed = serde_json::json!({"workload": "agent:a1b2c3d4e5f6", "machine": {"name": "cloud", "cloud": true}});
        assert!(!hosted_listing_without(
            &listed(vec![unnamed], vec![]),
            "cloud",
            id
        ));
        let unasked = serde_json::json!({"agents": [], "unreached": [], "answered": []});
        assert!(!hosted_listing_without(&unasked, "cloud", id));
        assert!(!hosted_listing_without(&serde_json::json!({}), "cloud", id));
    }

    #[test]
    fn agents_are_read_from_the_list() {
        let json = serde_json::json!({"agents": [
            {"workload": "agent:a1a1a1a1a1a1", "name": "main",
             "owner": {"kind": "machine-user", "user": "me", "machine": "laptop"}, "running": true,
             "teams": [{"resource": "group:t1t1t1t1t1t1"}, {"resource": "not plain"}, 3,
                       {"resource": "group:t2t2t2t2t2t2"}],
             "folder": "/m", "files": "/elsewhere/Main",
             "processes": {"pids": [12, 13], "group": "/sys/fs/cgroup/x",
                           "groups": ["/sys/fs/cgroup/x", "/sys/fs/cgroup/y"]},
             "measures": "/home/me/m.jsonl", "location": null},
            {"workload": "agent:a2a2a2a2a2a2", "name": "away", "running": null, "location": "aws-1"},
            {"workload": "agent:no ids with spaces", "name": "x", "location": null},
            {"workload": "a4a4a4a4a4a4", "name": "bare", "location": null},
            {"workload": "agent:a3a3a3a3a3a3", "name": "plain",
             "owner": {"kind": "machine-user", "user": "me@x", "machine": "laptop"},
             "machine": {"resource": "installation:m1m1m1m1m1m1"},
             "processes": {"pids": [], "groups": []}, "location": null},
        ], "unreached": []});
        let agents = measured(&json);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[1].id, "a3a3a3a3a3a3");
        // Its route's count is by the keyring's name for it: its ID.
        assert_eq!(
            (agents[0].key.as_str(), agents[1].key.as_str()),
            ("a1a1a1a1a1a1", "a3a3a3a3a3a3")
        );
        assert_eq!(agents[0].pids, [12, 13]);
        assert_eq!(agents[0].owner.as_deref(), Some("me@laptop"));
        assert_eq!(agents[1].owner, None);
        assert_eq!(agents[0].teams, ["t1t1t1t1t1t1", "t2t2t2t2t2t2"]);
        assert!(agents[1].teams.is_empty());
        assert_eq!(agents[0].groups.len(), 2);
        assert!(agents[1].groups.is_empty());
        assert_eq!((agents[0].folders.len(), agents[1].folders.len()), (2, 0));
    }
}
