//! One bridge for all of this machine's agents, where the machine says so
//! (`agents.toml`'s `shared_bridge`, off by default): `lnk agent bridges
//! <dir>` ([`serve`]) carries each agent's connections and messages from
//! others ([`crate::bridge::carry`]), as each agent's own bridge service
//! would, and runs one scheduler runner for all of them in a time zone
//! (`lnk-scheduler-tool run --agents`), instead of a service and a runner
//! an agent.
//!
//! Each agent's start keeps in `~/.lnk/safe/agents/bridges/agents/<agent
//! id>.json` ([`Record`]) that it's carried, and its stop removes it.
//! What it's carried with is its own, read where no harness reaches as
//! ever: its plan (`bridge.json`, its endpoint and key) and its settings.
//! Each agent's way in is its own socket, answered with its own
//! endpoint and key, so no agent's message reaches another's harness. An
//! agent started again, or changed, is carried again on its own; the
//! others are left as they are. With none left, it ends.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bridge::{self, Carrying, Plan};
use crate::idle::Door;
use crate::kept::{Kept, Restart, Watch};
use crate::settings::{self, Settings};

/// That an agent is carried by this machine's bridge, while it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The agent's name, by which its settings and plan are found.
    pub name: String,
    /// Its scheduler's runner, as it would run alone (`lnk-scheduler-tool
    /// run <flags>`): none when it has none, or the calendar wakes it.
    pub runner: Option<link_scheduler_tool_contract::cli::Runner>,
    /// Whether it runs in a terminal (`--foreground`): then carried only
    /// while it does, so one closed without a word leaves it.
    pub foreground: bool,
    /// How it's stopped when idle and woken at its next message, if its
    /// spec lets it ([`crate::idle`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep: Option<crate::idle::Sleep>,
}

link_files::specification!(Record, include_str!("bridges/record.schema.toml"));

/// Whether agents here share one bridge: the machine says so.
pub fn sharing() -> bool {
    crate::machine_settings::shared_bridge().unwrap_or(false)
}

/// Where this machine's bridge keeps whom it carries
/// (`~/.lnk/safe/agents/bridges`): in the agents' folder, which every sandbox
/// hides.
pub fn dir() -> PathBuf {
    link_files::layout::Hidden::SharedBridges.path()
}

/// The record of the agent keyed `key` ([`Settings::key`]).
pub fn record_of(key: &str) -> PathBuf {
    dir().join("agents").join(format!("{key}.json"))
}

/// Keeps that agent `agent` is carried, with its scheduler's `runner`,
/// then starts this machine's bridge if it isn't running: it carries the
/// agent at its next look.
pub fn join(
    agent: &Settings,
    runner: Option<link_scheduler_tool_contract::cli::Runner>,
    foreground: bool,
    sleep: Option<crate::idle::Sleep>,
) -> anyhow::Result<()> {
    let record = Record {
        name: agent.name.clone(),
        runner,
        foreground,
        sleep,
    };
    let path = record_of(&agent.key());
    let records = path.parent().within("no folder")?;
    link_files::create_private_dir(records).within(format!("creating {}", records.display()))?;
    link_files::write_private(&path, &link_files::saved::print_json(&record)?)
        .within(format!("writing {}", path.display()))?;
    start(&dir(), foreground)
}

/// While this process runs (an agent in a terminal), starts this
/// machine's bridge again whenever none runs: one killed, or ended by a
/// fault, is back within a few seconds, and asks each harness for what it
/// finished meanwhile. A service's is started again by the machine's
/// service manager.
#[expect(clippy::disallowed_methods, reason = "the watch's own period")]
pub fn watch() {
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(WATCH);
            let dir = dir();
            if !running(&dir)
                && let Err(err) = start(&dir, true)
            {
                link_format::daemon::warn!("starting the bridge again: {err:#}");
            }
        }
    });
}

/// How often an agent in a terminal looks that the bridge runs.
const WATCH: Duration = Duration::from_secs(2);

/// Whether a bridge holds `dir`'s lock: it runs.
fn running(dir: &Path) -> bool {
    let Ok(lock) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("serve.lock"))
    else {
        return true;
    };
    lock.try_lock().is_err()
}

/// Stops carrying agent `agent` (its name): its record goes, and the
/// bridge lets it go at its next look.
pub fn leave(agent: &str) {
    let key = settings::key_of(agent);
    let _ = std::fs::remove_file(record_of(&key));
    crate::idle::forget(&key);
}

/// This machine's bridge's launchd label and systemd unit.
pub const LABEL: &str = "link.bridges";
pub const UNIT: &str = "lnk-bridges.service";

/// Starts the bridge, whose folder is `dir`, unless it runs: as a
/// service of its own where Link's services run and the agent is one,
/// else (in a terminal, or with no user's systemd) in a session of its
/// own. One runs at a time ([`serve`]'s lock).
fn start(dir: &Path, foreground: bool) -> anyhow::Result<()> {
    let program = std::env::current_exe().within("finding lnk-agent")?;
    let argv = vec![
        program.display().to_string(),
        "bridges".into(),
        dir.display().to_string(),
    ];
    let log = dir.join("bridges.log");
    if link_service::managed() && !foreground {
        if link_service::running(LABEL, UNIT) {
            return Ok(());
        }
        // Never a secret: the service's definition is readable.
        let mut env: BTreeMap<String, String> = link_secretive::process::variables(|key| {
            matches!(key, "HOME" | "LANG" | "XDG_CONFIG_HOME" | "XDG_RUNTIME_DIR")
                || (key.starts_with("LNK_")
                    && !["KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"]
                        .iter()
                        .any(|secret| key.contains(secret)))
        })
        .collect();
        env.insert("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into());
        let job = link_service::Job {
            argv: argv.clone(),
            env,
            workdir: dir.to_path_buf(),
            log: log.clone(),
        };
        match link_service::start(LABEL, UNIT, &job) {
            Ok(()) => return Ok(()),
            // No service runs it: in a session of its own, as below.
            Err(_) => {
                let _ = link_service::stop(LABEL, UNIT);
            }
        }
    }
    spawn_detached(&argv, dir, &log)
}

/// Runs `argv` in `dir`, its output added to `log`, in a session of its
/// own and, forked once more with the middle process gone, no process's
/// child here, so whoever started it ending, or waited for, isn't its end.
pub fn spawn_detached(argv: &[String], dir: &Path, log: &Path) -> anyhow::Result<()> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .within(format!("opening {}", log.display()))?;
    let mut command = link_secretive::process::command(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, || {
            match libc::fork() {
                -1 => return Err(std::io::Error::last_os_error()),
                0 => {}
                _ => libc::_exit(0),
            }
            libc::setsid();
            Ok(())
        });
    }
    let mut middle = command.spawn().within(format!("starting {}", argv[0]))?;
    let _ = middle.wait();
    Ok(())
}

/// How often the bridge looks at whom it carries: an agent started is
/// carried within this.
const LOOK: Duration = Duration::from_millis(250);

/// How long it runs on with none to carry, for an agent starting again.
const NONE_FOR: Duration = Duration::from_secs(20);

/// The variable a debug build's test sets to [`NONE_FOR`] in its place,
/// in milliseconds, to see a bridge end without waiting for it
/// ([`link_secretive::test_var`]).
pub const TEST_NONE_FOR: &str = "LNK_TEST_BRIDGE_NONE_FOR_MS";

/// [`NONE_FOR`], or a debug build's test's ([`TEST_NONE_FOR`]).
fn none_for() -> Duration {
    link_secretive::test_var(TEST_NONE_FOR)
        .and_then(|ms| ms.to_str()?.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(NONE_FOR)
}

/// How long a bridge started for an agent waits for one ending to let
/// go: well past its last look.
const ENDING: Duration = Duration::from_secs(2);

/// Every agent recorded in `dir`, by key, as its record says now, and
/// what each is carried as: its record and plan, so an agent started
/// again (its plan has a new key) is carried anew. One in a
/// terminal that closed is left out.
fn carried(dir: &Path) -> BTreeMap<String, (Record, String)> {
    let mut agents = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir.join("agents")) else {
        return agents;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(key) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|key| settings::is_key(key))
        else {
            continue;
        };
        // One being written is read at the next look.
        let Some((bytes, record)) = std::fs::read(&path).ok().and_then(|bytes| {
            let record = link_files::saved::parse_json::<Record>(&bytes).ok()?;
            Some((bytes, record))
        }) else {
            continue;
        };
        if link_machine::machine::check_name(&record.name).is_err() {
            continue;
        }
        // Its terminal closed without a word: its record goes too.
        if record.foreground && !crate::service::foreground_running(&record.name) {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let plan = std::fs::read(Plan::path(&record.name)).unwrap_or_default();
        let generation = link_digest::sha256(&[bytes, plan].concat());
        agents.insert(key.to_string(), (record, generation));
    }
    agents
}

/// Whether any record in `dir` was written since `since`: an agent
/// starting as the bridge would end.
fn recorded_since(dir: &Path, since: Instant) -> bool {
    let Ok(entries) = std::fs::read_dir(dir.join("agents")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|at| at.elapsed().unwrap_or_default() < since.elapsed())
    })
}

/// The scheduler runners, one a time zone: each zone's file of agents,
/// and its runner, kept while the file lists any.
#[derive(Default)]
struct Runners {
    running: BTreeMap<String, Kept>,
}

/// When a zone's runner that ended is started again: no more than every
/// few seconds, so one that stops at once doesn't spin.
const RUNNER_RESTART: Restart = Restart {
    after: Duration::from_secs(5),
    at_most: Duration::from_secs(5),
};

/// A zone's runner is started again only while its file lists an agent:
/// one with none left ends by itself, and stays ended.
struct Listed(PathBuf);

impl Watch for Listed {
    fn ended(&mut self, how: &std::io::Result<std::process::ExitStatus>) -> bool {
        if let Err(err) = how {
            link_format::daemon::warn!("not running the scheduler: {err}");
        }
        std::fs::read(&self.0)
            .ok()
            .and_then(|bytes| {
                link_files::saved::parse_json::<link_scheduler_tool_contract::listed::Listed>(
                    &bytes,
                )
                .ok()
            })
            .is_some_and(|listed| !listed.agents.is_empty())
    }
}

impl Runners {
    /// Gives each zone's runner the agents in it now: their flags in its
    /// file, which it reads again when it changes; one kept where none
    /// is, and one with no agent left ends by itself.
    fn tend(&mut self, dir: &Path, agents: &BTreeMap<String, (Record, String)>) {
        let mut zones: BTreeMap<String, (String, Vec<link_scheduler_tool_contract::cli::Flags>)> =
            BTreeMap::new();
        for (record, _) in agents.values() {
            // Each agent's runner by its time zone, its flags as they are.
            let Some(runner) = &record.runner else {
                continue;
            };
            let Some(zone) = runner.flags.time_zone.clone() else {
                continue;
            };
            zones
                .entry(zone)
                .or_insert((runner.program.display().to_string(), Vec::new()))
                .1
                .push(runner.flags.clone());
        }
        // A zone with a runner but none left: told so, and it ends.
        for zone in self.running.keys() {
            zones
                .entry(zone.clone())
                .or_insert_with(|| (String::new(), Vec::new()));
        }
        for (zone, (program, listed)) in &zones {
            let file = dir.join(format!(
                "runners/{}.json",
                &link_digest::sha256(zone.as_bytes())[..12]
            ));
            let bytes =
                link_files::saved::print_json(&link_scheduler_tool_contract::listed::Listed {
                    agents: listed.clone(),
                })
                .unwrap_or_default();
            // Its folder gone (its home deleted as it ends): nothing made.
            if dir.is_dir()
                && std::fs::read(&file).ok().as_ref() != Some(&bytes)
                && let Err(err) = link_files::write_private(&file, &bytes)
            {
                link_format::daemon::warn!("the scheduler's agents in {zone}: {err:#}");
            }
            // One that ended with none listed, gone.
            if self.running.get(zone).is_some_and(Kept::ended) {
                self.running.remove(zone);
            }
            if listed.is_empty() || program.is_empty() || self.running.contains_key(zone) {
                continue;
            }
            let (program, zone_of, listed_in) = (program.clone(), zone.clone(), file.clone());
            let run = link_scheduler_tool_contract::cli::SchedulerToolRunArgs {
                agents: Some(listed_in),
                flags: link_scheduler_tool_contract::cli::Flags {
                    time_zone: Some(zone_of),
                    ..Default::default()
                },
            }
            .argv();
            let kept = Kept::start(
                move || {
                    let mut command = link_secretive::process::ours(&program);
                    command.args(&run);
                    command
                },
                RUNNER_RESTART,
                Listed(file),
            );
            self.running.insert(zone.clone(), kept);
        }
    }

    /// Tells every runner it has no agent left, and ends each
    /// ([`Kept::stop`]: asked, its firings ended, then waited for).
    fn end(mut self, dir: &Path) {
        self.tend(dir, &BTreeMap::new());
        for (_, kept) in self.running {
            kept.stop();
        }
    }
}

/// `lnk agent bridges <dir>`: carries every agent `dir` records, each
/// carried again on its own when it's started again or changes, until
/// none is left. One at a time: another already carrying is left to it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls a lock and its state, which nothing announces"
)]
pub fn serve(dir: &Path) -> anyhow::Result<()> {
    link_files::create_private_dir(dir).within(format!("creating {}", dir.display()))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("serve.lock"))
        .within(format!("opening {}", dir.display()))?;
    // Another runs it: it carries this one's agent at its next look. One
    // ending a moment ago, between its last look and letting go, is
    // waited for, and this one carries.
    let waited = Instant::now();
    while lock.try_lock().is_err() {
        if waited.elapsed() > ENDING {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut carrying: BTreeMap<String, (String, Arc<Carrying>, Option<Door>)> = BTreeMap::new();
    let mut runners = Runners::default();
    let mut none_since = Instant::now();
    loop {
        // Its folder gone (its home deleted): nothing to carry, nothing
        // made again.
        if !dir.is_dir() {
            for (_, (_, carried, door)) in std::mem::take(&mut carrying) {
                if let Some(door) = door {
                    door.close();
                }
                carried.stop();
            }
            return Ok(());
        }
        let agents = carried(dir);
        // Gone, or to be carried anew.
        let stale: Vec<String> = carrying
            .iter()
            .filter(|(key, (generation, ..))| agents.get(*key).map(|(_, g)| g) != Some(generation))
            .map(|(key, _)| key.clone())
            .collect();
        for key in stale {
            if let Some((_, carried, door)) = carrying.remove(&key) {
                if let Some(door) = door {
                    door.close();
                }
                carried.stop();
            }
        }
        for (key, (record, generation)) in &agents {
            if carrying.contains_key(key) {
                continue;
            }
            let loaded = Settings::load(&record.name)
                .and_then(|agent| Ok((Plan::load(&record.name)?, agent)));
            match loaded {
                // The runner is this bridge's, for every agent at once.
                // One that stops when idle: its front door, and asked for
                // what nobody took only as it wakes and sleeps.
                Ok((plan, agent)) => {
                    let sleeps = record.sleep.clone().filter(|_| cfg!(target_os = "linux"));
                    let carried = Arc::new(bridge::carry(&agent, &plan, None, sleeps.is_none()));
                    let door = sleeps.map(|sleep| {
                        Door::open(
                            key,
                            sleep,
                            carried.clone(),
                            crate::background::sweeper(&agent),
                        )
                    });
                    carrying.insert(key.clone(), (generation.clone(), carried, door));
                }
                // Read again at its next start.
                Err(err) => {
                    link_format::daemon::warn!("not carrying {}: {err:#}", record.name);
                    carrying.insert(
                        key.clone(),
                        (generation.clone(), Arc::new(bridge::Carrying::none()), None),
                    );
                }
            }
        }
        runners.tend(dir, &agents);
        if !agents.is_empty() {
            none_since = Instant::now();
        } else if none_since.elapsed() > none_for() && !recorded_since(dir, none_since) {
            runners.end(dir);
            // None left: its service ends, so no restart starts it again.
            let file = match cfg!(target_os = "macos") {
                true => link_service::plist_path_of(LABEL),
                false => link_service::unit_path_of(UNIT),
            };
            if link_service::managed() && file.exists() {
                link_service::stop_from_within(&file);
            }
            return Ok(());
        }
        std::thread::sleep(LOOK);
    }
}

#[cfg(test)]
mod tests {

    /// A runner's line is its program, `run` and its flags, as the
    /// generated argv writes them; its zone's file lists its flags, typed,
    /// and reads back as they were.
    #[test]
    fn a_runners_line_and_its_zones_file_are_its_flags() {
        use link_scheduler_tool_contract::cli::{Flags, Runner};
        let runner = Runner {
            program: "/bin/lnk-scheduler-tool".into(),
            flags: Flags {
                state: Some("/s".into()),
                time_zone: Some("Europe/Paris".into()),
                ..Flags::default()
            },
        };
        assert_eq!(
            runner.argv(),
            [
                "/bin/lnk-scheduler-tool",
                "run",
                "--state",
                "/s",
                "--time-zone",
                "Europe/Paris"
            ]
        );
        let listed = link_scheduler_tool_contract::listed::Listed {
            agents: vec![runner.flags.clone()],
        };
        let file = link_files::saved::print_json(&listed).unwrap();
        assert_eq!(
            link_files::saved::parse_json::<link_scheduler_tool_contract::listed::Listed>(&file)
                .unwrap()
                .agents,
            [runner.flags]
        );
    }
}
