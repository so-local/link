//! `lnk agent migrate`: an agent someone already has, from a harness
//! installed outside Link (OpenClaw, Hermes, DeepSeek), brought under Link
//! with its harness. What the install keeps is the adapter's typed list
//! (`lnk-<name> traces`); the plan is said in full and journaled
//! ([`crate::journal`]) before anything is done; each step is then done
//! and said, the original left working until `lnk agent migrate --clean`.
//! An install of several agents (OpenClaw's agent folders, Hermes's
//! profiles) becomes as many agents of Link's, each with its own journal.

use link_format::WithinExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure};
use link_files::Relative;
use link_harness_contract::harness::traces::{Kind, Trace};

use crate::journal::{Journal, Step};
use crate::settings::Settings;

mod copying;
pub mod variables;
use copying::remove;
pub use copying::{copy_checked, rewrite};
pub use variables::connection_of;
use variables::{Made, Why};

/// The harnesses `migrate` looks for, in the order it offers them.
pub const HARNESSES: &[&str] = &["openclaw", "hermes", "deepseek"];

/// An install found outside Link: its harness, and its agents, the
/// install's own first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub harness: String,
    pub agents: Vec<Agent>,
}

/// One agent of an install, as a migration carries it: its name there
/// (none: the install's own), each place of the install's that's its, and
/// what of them is other agents', left out of its copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub name: Option<String>,
    pub places: Vec<Place>,
    pub leaving: Vec<Relative>,
}

/// One place of an install that's there, relative to the home folder,
/// and where it lands in its agent's harness home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub kind: Kind,
    pub at: Relative,
    pub lands: Relative,
}

impl Found {
    /// The install's own agent.
    pub fn own(&self) -> &Agent {
        &self.agents[0]
    }
}

impl Agent {
    /// Its home's place.
    pub fn home(&self) -> Option<&Place> {
        self.places.iter().find(|p| p.kind == Kind::Home)
    }
}

/// The install of harness `harness` in home folder `home`, from its
/// adapter's `traces`: found when its home is there, with each agent it
/// holds. Only places in the home folder count: what's elsewhere (`/tmp`)
/// is the running harness's and stays.
pub fn find(harness: &str, traces: &[Trace], home: &Path, uid: &str) -> Option<Found> {
    let there = |path: PathBuf| {
        path.symlink_metadata().ok()?;
        Relative::within(home, &path)
    };
    let shared: Vec<Place> = traces
        .iter()
        .filter(|t| !matches!(t.kind, Kind::Agent | Kind::Profile))
        .filter_map(|t| {
            let at = there(t.at(home, uid))?;
            Some(Place {
                kind: t.kind,
                lands: at.clone(),
                at,
            })
        })
        .collect();
    let own_home = shared.iter().find(|p| p.kind == Kind::Home)?.at.clone();
    let parts = |name: Option<&str>| -> Vec<Place> {
        traces
            .iter()
            .filter(|t| matches!(t.kind, Kind::Agent | Kind::Profile))
            .filter_map(|t| {
                let at = there(t.of(home, uid, name))?;
                let lands = match t.kind {
                    Kind::Profile => own_home.clone(),
                    _ => Relative::within(home, &t.of(home, uid, None))?,
                };
                Some(Place {
                    kind: t.kind,
                    at,
                    lands,
                })
            })
            .collect()
    };
    let mut names: Vec<String> = traces
        .iter()
        .filter(|t| matches!(t.kind, Kind::Agent | Kind::Profile))
        .flat_map(|t| names_of(t, home, uid))
        .collect();
    names.sort();
    names.dedup();
    let others: Vec<(String, Vec<Place>)> = names
        .into_iter()
        .map(|n| {
            let p = parts(Some(&n));
            (n, p)
        })
        .collect();
    let all_parts: Vec<Relative> = others
        .iter()
        .flat_map(|(_, p)| p.iter().map(|p| p.at.clone()))
        .collect();
    let own_parts: Vec<Relative> = parts(None).into_iter().map(|p| p.at).collect();
    let mut agents = vec![Agent {
        name: None,
        places: shared.clone(),
        leaving: all_parts.clone(),
    }];
    for (name, parts) in others {
        let profile = parts.iter().find(|p| p.kind == Kind::Profile);
        agents.push(match profile {
            // A whole home of its own: its settings, secrets and login
            // where the install's own are in theirs.
            Some(Place { at, .. }) => Agent {
                name: Some(name),
                places: traces
                    .iter()
                    .filter(|t| !matches!(t.kind, Kind::Agent | Kind::Profile))
                    .filter_map(|t| {
                        let own = Relative::within(home, &t.at(home, uid))?;
                        let Ok(rest) = Path::new(own.as_str()).strip_prefix(own_home.as_str())
                        else {
                            return shared.iter().find(|p| p.at == own).cloned();
                        };
                        let mine = Relative::within(home, &at.under(home).join(rest))?;
                        mine.under(home).symlink_metadata().ok()?;
                        Some(Place {
                            kind: t.kind,
                            at: mine,
                            lands: own,
                        })
                    })
                    .collect(),
                leaving: Vec::new(),
            },
            // Its part of the shared home, where the install's own agent's
            // is: the rest of the home with it, no other agent's.
            None => Agent {
                name: Some(name),
                places: shared.iter().cloned().chain(parts).collect(),
                leaving: all_parts.iter().chain(&own_parts).cloned().collect(),
            },
        });
    }
    Some(Found {
        harness: harness.to_string(),
        agents,
    })
}

/// The names of the agents at trace `trace` other than the install's own:
/// each folder whose name fits its path's `{agent}` or `{-agent}`.
fn names_of(trace: &Trace, home: &Path, uid: &str) -> Vec<String> {
    const MARK: &str = "\u{1}";
    let marked = trace.of(home, uid, Some(MARK));
    let (Some(parent), Some(pattern)) =
        (marked.parent(), marked.file_name().and_then(|n| n.to_str()))
    else {
        return Vec::new();
    };
    let Some((before, after)) = pattern.split_once(MARK) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let name = name.strip_prefix(before)?.strip_suffix(after)?;
            (!name.is_empty() && name != link_harness_contract::harness::traces::OWN)
                .then(|| name.to_string())
        })
        .collect();
    names.sort();
    names
}

/// What a migration of `agent`, of an install, does, in order: the agent
/// made, the install's own service stopped first when `service` (one
/// agent's plan holds it), its home and settings copied, its hosted
/// model's key made its own, and each other secret taken out of its copy.
pub fn plan(agent: &Agent, home: &Path, service: bool) -> anyhow::Result<Vec<Step>> {
    let mut steps = vec![Step::Agent];
    if service {
        for place in agent.places.iter().filter(|p| p.kind == Kind::Service) {
            steps.push(Step::Service {
                at: place.at.clone(),
            });
        }
    }
    let homes: Vec<&Relative> = agent
        .places
        .iter()
        .filter(|p| p.kind == Kind::Home)
        .map(|p| &p.at)
        .collect();
    let inside_home = |at: &Relative| {
        homes
            .iter()
            .any(|h| *h != at && Path::new(at.as_str()).starts_with(h.as_str()))
    };
    for place in &agent.places {
        let whole = matches!(place.kind, Kind::Home | Kind::Config) && !inside_home(&place.at);
        if whole || place.kind == Kind::Agent {
            steps.push(Step::Copy {
                from: place.at.clone(),
                to: place.lands.clone(),
                leaving: agent
                    .leaving
                    .iter()
                    .filter(|l| whole && Path::new(l.as_str()).starts_with(place.at.as_str()))
                    .cloned()
                    .collect(),
            });
        }
    }
    for place in agent.places.iter().filter(|p| p.kind == Kind::Secrets) {
        for (name, made) in variables::made(&env_file(&place.at.under(home))?) {
            match made {
                Made::Model { .. } => steps.push(Step::Model { name }),
                Made::Sealed(_) => steps.push(Step::Sealed { name }),
                Made::Left(_) => steps.push(Step::Secret { name }),
                Made::Connection { .. } | Made::Setting => {}
            }
        }
    }
    Ok(steps)
}

/// The names and values in a `.env`-style file: `NAME=value` lines, an
/// `export ` before and quotes around a value allowed, comments and blank
/// lines skipped.
pub fn env_file(path: &Path) -> anyhow::Result<Vec<(String, link_secretive::Secret)>> {
    let text = std::fs::read_to_string(path).within(format!("reading {}", path.display()))?;
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.trim_start_matches("export ").split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        found.push((
            name.to_string(),
            link_secretive::Secret::new(value.to_string()),
        ));
    }
    Ok(found)
}

/// The words of agent `from`'s plan, of an install of harness
/// `harness`, a line each, before anything is done.
pub fn words(
    harness: &str,
    from: &Agent,
    steps: &[Step],
    agent: &str,
    harness_home: &Path,
    home: &Path,
) -> Vec<String> {
    let tilde = link_settings::folders::tilde;
    let made = made_of(from, home);
    let mut lines = Vec::new();
    for step in steps {
        lines.push(match step {
            Step::Schedules { names, not_carried, .. } => format!("Carrying schedules into Link: {}. Not carried: {}. Only the copied native source is cleared; the original install keeps its jobs.", names.join(", "), not_carried.join(", ")),
            Step::Agent => format!("Making the agent {agent}, run by {harness} under Link."),
            Step::Copy { from, to, leaving } => format!(
                "Copying {} to {}{}, each file checked where it lands, its paths naming where it \
                 is now; each database copied whole by SQLite.",
                tilde(&from.under(home)),
                tilde(&to.under(harness_home)),
                match leaving.len() {
                    0 => String::new(),
                    1 => " but a folder of another of its agents".to_string(),
                    n => format!(" but {n} folders of its other agents"),
                }
            ),
            Step::Secret { name } => format!(
                "Leaving {name} in the original's settings{}: lnk secret add {name} --to \
                 agent:{agent} --host <its host> gives it sealed.",
                match made.iter().find(|(n, _)| n == name).map(|(_, m)| m) {
                    Some(Made::Left(Why::Elsewhere { by, to })) => {
                        format!(", as {by} sends it to {to}")
                    }
                    Some(Made::Left(Why::NotItsKey)) => {
                        ", as it doesn't hold its provider's key".into()
                    }
                    Some(Made::Left(Why::SecondModel { first })) => {
                        format!(", as {agent}'s model key is {first}")
                    }
                    _ => ", as Link doesn't know where it goes".into(),
                }
            ),
            Step::Model { name } => format!(
                "Making {name} {agent}'s model key, kept in Link's secrets: its proxy adds it \
                 for its provider, and the harness never holds it."
            ),
            Step::Sealed { name } => format!(
                "Giving {name} to {agent} sealed for {}: its proxy adds it to calls there, and \
                 the harness never holds it.",
                match made.iter().find(|(n, _)| n == name).map(|(_, m)| m) {
                    Some(Made::Sealed(sealing)) => sealing.host.clone(),
                    _ => "its host".into(),
                }
            ),
            Step::Service { at } => format!(
                "Stopping its own service, {}, and keeping it from starting: two would answer one bot.",
                tilde(&at.under(home))
            ),
        });
    }
    lines.push(format!(
        "Under Link, {} runs in its sandbox: it writes its home and the agent's files \
         folder, and reaches the internet through Link's proxy, where before it reached your \
         whole computer. lnk agent show --permissions shows what it may do; lnk agent permission add \
         gives it more.",
        harness
    ));
    if steps.iter().any(|s| {
        matches!(
            s,
            Step::Secret { .. } | Step::Model { .. } | Step::Sealed { .. }
        )
    }) {
        lines.push(
            "Each secret is taken out of the copy's settings, so the harness never holds its \
             value."
                .to_string(),
        );
    }
    for connection in connections(from, home) {
        lines.push(format!(
            "Its {connection} bot goes to Link's connection once it's in, the token handed over: \
             you confirm the bot is yours there."
        ));
    }
    for place in &from.places {
        let at = tilde(&place.at.under(home));
        match place.kind {
            Kind::Login => lines.push(format!(
                "Its own login, {at}, can't move: it's copied, and the original keeps working."
            )),
            Kind::Program
            | Kind::Package
            | Kind::Cache
            | Kind::Log
            | Kind::Lock
            | Kind::Browser => lines.push(format!(
                "{at} stays where it is until lnk agent migrate --clean."
            )),
            _ => {}
        }
    }
    lines
}

/// What Link makes of each variable of agent `from`'s secrets files.
pub fn made_of(from: &Agent, home: &Path) -> Vec<(String, Made)> {
    from.places
        .iter()
        .filter(|p| p.kind == Kind::Secrets)
        .flat_map(|p| variables::made(&env_file(&p.at.under(home)).unwrap_or_default()))
        .collect()
}

/// Each variable of agent `from`'s secrets, what Link makes of it for
/// `agent`, and where it's sent: said whole, as one may be used other
/// than its name says.
pub fn handed(from: &Agent, home: &Path, agent: &str) -> Vec<[String; 3]> {
    made_of(from, home)
        .into_iter()
        .map(|(name, made)| {
            let (makes, to) = match made {
                Made::Model { provider } => (
                    format!("{agent}'s model key"),
                    format!("{provider}'s API, added by its proxy"),
                ),
                Made::Connection { connection } => (
                    format!("its {connection} bot"),
                    format!("{connection}, by Link's connection"),
                ),
                Made::Sealed(sealing) => (
                    format!("a secret sealed for {agent}"),
                    format!("{}, in {}", sealing.host, sealing.header),
                ),
                Made::Setting => ("a setting, copied".into(), "nowhere: not a secret".into()),
                Made::Left(why) => (
                    "left in the original".into(),
                    match why {
                        Why::Unknown => "unknown to Link".into(),
                        Why::Elsewhere { by, to } => format!("{to}, by {by}"),
                        Why::NotItsKey => "not its provider's key".into(),
                        Why::SecondModel { first } => format!("{first} is the model's key"),
                    },
                ),
            };
            [name, makes, to]
        })
        .collect()
}

/// The connections whose tokens agent `from`'s secrets hold, each once.
pub fn connections(from: &Agent, home: &Path) -> Vec<&'static str> {
    let mut connections = Vec::new();
    for place in from.places.iter().filter(|p| p.kind == Kind::Secrets) {
        for (name, _) in env_file(&place.at.under(home)).unwrap_or_default() {
            if let Some(connection) = connection_of(&name)
                && !connections.contains(&connection)
            {
                connections.push(connection);
            }
        }
    }
    connections
}

/// The variables that hand connection `connection`'s tokens, from agent
/// `from`'s secrets, to its `connect`.
pub fn tokens_for(
    connection: &str,
    from: &Agent,
    home: &Path,
) -> Vec<(&'static str, link_secretive::Secret)> {
    let mut tokens = Vec::new();
    for place in from.places.iter().filter(|p| p.kind == Kind::Secrets) {
        for (name, value) in env_file(&place.at.under(home)).unwrap_or_default() {
            if let Some(var) = variables::handed_as(&name, connection) {
                tokens.push((var, value));
            }
        }
    }
    tokens
}

/// Runs step `step` of agent `agent`'s migration from home folder `home`.
pub fn run(step: &Step, journal: &Journal, home: &Path) -> anyhow::Result<()> {
    match step {
        Step::Schedules { at, .. } => {
            let agent = Settings::load_without_keys(&journal.agent)?;
            crate::schedules::recover(&agent)?;
            let adapter = crate::adapter::Adapter::find(&journal.harness)?;
            let root = at.under(&agent.box_of(&journal.harness));
            let schedules = adapter.native_schedules(&root)?;
            crate::schedules::import(&agent, &journal.harness, &root, &schedules)
        }
        Step::Agent => {
            let path = Settings::path_of(&journal.agent);
            if !path.exists() {
                let mut agent = Settings::new(&journal.agent);
                agent.id = Some(crate::settings::ids::workload(
                    &crate::settings::ids::reserve()?,
                )?);
                agent.harness = Some(journal.harness.clone());
                agent.save()?;
            }
            Ok(())
        }
        Step::Copy { from, to, leaving } => {
            let agent = Settings::load_without_keys(&journal.agent)?;
            let harness_home = agent.box_of(&journal.harness);
            let target = to.under(&harness_home);
            let leaving: Vec<PathBuf> = leaving.iter().map(|l| l.under(home)).collect();
            copy_checked(&from.under(home), &target, &leaving)?;
            // Its settings and sessions name where it was: where it is
            // now, under Link, each copy of the plan where it landed.
            rewrite(&target, &moves(journal, home, &harness_home))?;
            Ok(())
        }
        // Never given plain: an agent is given a secret sealed only, for a
        // host only its owner knows, and the original still holds it.
        Step::Secret { name } => take_out(journal, home, name),
        Step::Sealed { name } => {
            let value = secret_of(journal, home, name)?;
            let Some(variables::Variable::Secret { sealed, .. }) = variables::known().get(name)
            else {
                bail!("{name} isn't a secret Link seals anymore");
            };
            let agent = Settings::load_without_keys(&journal.agent)?;
            // Every agent granted it already keeps it: adding replaces a
            // secret whole, its grants with it.
            let mut to = link_secret_contract::list()?
                .into_iter()
                .find(|l| l.kept && l.name.as_deref() == Some(name.as_str()))
                .map(|l| l.to)
                .unwrap_or_default()
                .into_iter()
                .filter(|g| g.r#as == link_secret_contract::Use::Variable)
                .map(|g| link_secret_contract::Grantee::of(g.workload, &g.name, g.principal))
                .collect::<anyhow::Result<Vec<_>>>()?;
            to.push(agent.secret_agent()?);
            link_secret_contract::add(name, &value, Some(sealed.clone()), to)?;
            take_out(journal, home, name)
        }
        Step::Model { name } => {
            let value = secret_of(journal, home, name)?;
            let mut agent = Settings::load(&journal.agent)?;
            if agent.model.is_none() {
                let provider = crate::settings::provider_of(value.expose())
                    .within(format!("{name} isn't a hosted model's key anymore"))?;
                // Its provider's model by Link's default, as a key alone gives.
                agent.model = Some(link_harness_contract::harness::Model {
                    actions_url: None,
                    provider: provider.into(),
                    api_key: value.expose_map(|k| k.trim().to_string()),
                    name: None,
                    base_url: None,
                });
                agent.save()?;
            }
            take_out(journal, home, name)
        }
        Step::Service { at } => {
            let path = at.under(home);
            let label = service_label(&path);
            link_service::unload_other(&label);
            if path.exists() {
                let kept = stopped_service(at)?;
                if let Some(parent) = kept.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&path, &kept).within(format!("turning off {}", path.display()))?;
            }
            // Kept with the agent, so removing it turns the service back on.
            let mut agent = Settings::load(&journal.agent)?;
            if !agent.stopped_services.iter().any(|k| k == at.as_str()) {
                agent.stopped_services.push(at.as_str().to_string());
                agent.save()?;
            }
            Ok(())
        }
    }
}

/// Each copy of `journal`'s plan, where it was in home folder `home` and
/// where it lands in harness home `harness_home`, the deepest first, so
/// an agent's own folder is met before the home it was in.
pub fn moves(journal: &Journal, home: &Path, harness_home: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut moves: Vec<(PathBuf, PathBuf)> = journal
        .entries
        .iter()
        .filter_map(|e| match &e.step {
            Step::Copy { from, to, .. } => Some((from.under(home), to.under(harness_home))),
            _ => None,
        })
        .collect();
    moves.sort_by_key(|(from, _)| std::cmp::Reverse(from.components().count()));
    moves.dedup();
    moves
}

/// Undoes step `step`, done, of a migration from home folder `home`.
pub fn undo(step: &Step, journal: &Journal, home: &Path) -> anyhow::Result<()> {
    match step {
        Step::Schedules { .. } => Ok(()),
        Step::Agent => {
            for path in [
                Settings::path_of(&journal.agent),
                crate::settings::config_of(&journal.agent),
                crate::settings::agent_dir(&journal.agent),
            ] {
                remove(&path)?;
            }
            Ok(())
        }
        Step::Copy { to, .. } => match Settings::load_without_keys(&journal.agent) {
            Ok(agent) => remove(&to.under(&agent.box_of(&journal.harness))),
            Err(_) => Ok(()),
        },
        Step::Sealed { name } => {
            let named = link_secret_contract::Called::Name(name.clone());
            link_secret_contract::remove(&link_secret_contract::Removal::Secret(named), None)?;
            Ok(())
        }
        // Its copy goes with the agent; the original holds the secret,
        // and the agent's model key goes with the agent its first step made.
        Step::Secret { .. } | Step::Model { .. } => Ok(()),
        Step::Service { at } => turn_back_on(home, at).map(|_| ()),
    }
}

/// Service file `at`, in the home folder `home`, kept from starting as
/// its install was brought in, turned back on: whether it was off.
pub fn turn_back_on(home: &Path, at: &Relative) -> anyhow::Result<bool> {
    let path = at.under(home);
    if path.exists() {
        return Ok(false);
    }
    let Some(kept) = stopped_places(home, at)?.into_iter().find(|k| k.exists()) else {
        return Ok(false);
    };
    std::fs::rename(&kept, &path).within(format!("turning {} back on", path.display()))?;
    link_service::load_other(&path);
    Ok(true)
}

/// Where service file `at`, in the home folder, is kept while it's off:
/// in `~/.lnk/safe/agents/stopped-services`, by its path in the home folder.
pub fn stopped_service(at: &Relative) -> anyhow::Result<PathBuf> {
    Ok(link_files::layout::Hidden::StoppedServices
        .try_path()?
        .join(at.as_str()))
}

/// Where Link keeps a disabled service file, under Hidden.
pub fn stopped_places(_home: &Path, at: &Relative) -> anyhow::Result<Vec<PathBuf>> {
    Ok(vec![stopped_service(at)?])
}

/// Clears agent `agent`'s partial migration, if there's one: each step
/// done undone, last first. Whether there was one.
pub fn clear_partial(agent: &str) -> anyhow::Result<bool> {
    let Some(mut journal) = Journal::load(agent)? else {
        return Ok(false);
    };
    let home = link_files::try_home_dir()?;
    let undone = journal.clone();
    journal.clear(|step| undo(step, &undone, &home))?;
    Ok(true)
}

/// Secret `name` of the install's, read where it was.
fn secret_of(journal: &Journal, home: &Path, name: &str) -> anyhow::Result<link_secretive::Secret> {
    secrets_of(journal, home)?
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .within(format!("{name} isn't in the install's secrets anymore"))
}

/// Secret `name` taken out of every secrets file of the agent's copy: its
/// harness reads it from Link, never from a file of its home.
fn take_out(journal: &Journal, home: &Path, name: &str) -> anyhow::Result<()> {
    let adapter = crate::adapter::Adapter::find(&journal.harness)?;
    let forget = adapter.contract()?.forget;
    let harness_home = Settings::load_without_keys(&journal.agent)?.box_of(&journal.harness);
    let uid = link_service::user_id().unwrap_or_default();
    for trace in adapter.traces().iter().filter(|t| t.kind == Kind::Secrets) {
        let own = trace.at(home, &uid);
        let copy = journal.entries.iter().find_map(|e| match &e.step {
            Step::Copy { to, .. } => {
                let rest = own.strip_prefix(to.under(home)).ok()?;
                Some(to.under(&harness_home).join(rest))
            }
            _ => None,
        });
        if let Some(copy) = copy.filter(|c| c.exists()) {
            link_harness_contract::harness::merge_env_file(&copy, &[(name, None)])
                .within(format!("taking {name} out of {}", copy.display()))?;
        }
    }
    // And out of the files of the harness's own it may keep it in too.
    if forget {
        let value = secret_of(journal, home, name)?;
        adapter
            .forget(&harness_home, &[value.expose().as_str()])
            .within(format!("taking {name} out of {}", harness_home.display()))?;
    }
    Ok(())
}

/// The secrets in every secrets file of the agent's, read where it was.
fn secrets_of(
    journal: &Journal,
    home: &Path,
) -> anyhow::Result<Vec<(String, link_secretive::Secret)>> {
    let adapter = crate::adapter::Adapter::find(&journal.harness)?;
    let uid = link_service::user_id().unwrap_or_default();
    let mut all = Vec::new();
    for trace in adapter.traces().iter().filter(|t| t.kind == Kind::Secrets) {
        // Where this agent's are: in the folder its home was copied from
        // (a profile's), else where the install's own are.
        let own = trace.at(home, &uid);
        let path = journal
            .entries
            .iter()
            .filter_map(|e| match &e.step {
                Step::Copy { from, to, .. } => {
                    let rest = own.strip_prefix(to.under(home)).ok()?;
                    Some(from.under(home).join(rest))
                }
                _ => None,
            })
            .next()
            .unwrap_or(own);
        if path.exists() {
            all.extend(env_file(&path)?);
        }
    }
    Ok(all)
}

/// A service file's label: its name without `.plist` or `.service`.
fn service_label(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name.trim_end_matches(".plist").to_string()
}

#[cfg(test)]
mod tests;
