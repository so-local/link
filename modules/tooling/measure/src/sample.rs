//! One look at this machine: the machine itself, each agent running here
//! and each of its processes, measured from outside them, as units with
//! their values. An agent is what the answer to `measure.agents` says it
//! runs as: on Linux its services' control groups, which hold every
//! process they start, and the tree under any of its processes outside
//! them; else the tree under its processes.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use link_measure_contract::client::{
    self as measure, Carried, Tokens, Unit, group, plain, program,
};

use crate::gpu::Gpus;
#[cfg(not(target_os = "macos"))]
use crate::linux as os;
#[cfg(target_os = "macos")]
use crate::mac as os;
use crate::settings::Settings;

mod listing;
mod looking;
mod space;
pub use listing::{Agent, changed};
use looking::{Counters, Look, MEMORY_EVERY};
use space::walk;
pub use space::{filesystem_used, vm_stat_used};

pub use link_measure_contract::client::process::Proc;

/// The machine now; what a system doesn't say is none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub memory: Option<u64>,
    pub cpu: Option<f64>,
    pub processes: Option<u64>,
    pub ports: Option<u64>,
    pub disk_used: Option<u64>,
    /// When it started, in nanoseconds since 1970.
    pub booted: Option<u64>,
    /// Bytes read and written, by disk.
    pub disks: Option<BTreeMap<String, (u64, u64)>>,
    /// Bytes in and out, by network interface.
    pub networks: Option<BTreeMap<String, (u64, u64)>>,
    /// Its GPUs, where it has one that says.
    pub gpu: Option<Gpus>,
}

/// A unit and its values now: each measure's by its name, and each
/// [`link_measure_contract::client::Kind::Each`] measure's values since the last
/// look.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measured {
    pub unit: Unit,
    pub values: BTreeMap<&'static str, f64>,
    pub each: BTreeMap<&'static str, Vec<f64>>,
    /// When its running totals began, in nanoseconds since 1970: the
    /// machine's start, an agent's main process's, a process's own. A
    /// new one is a restart, its totals counted from nothing again.
    pub started: Option<u64>,
    /// An agent whose model's answers its model route counts
    /// ([`measure::tokens`]): its tokens are the route's, never its
    /// harness's too.
    pub metered: bool,
}

impl Measured {
    fn new(unit: Unit) -> Self {
        Self {
            unit,
            ..Self::default()
        }
    }

    fn set(&mut self, name: &'static str, value: impl Into<Option<f64>>) {
        if let Some(value) = value.into() {
            self.values.insert(name, value);
        }
    }

    /// Adds `value` to measure `name` (a harness's tokens, a turn).
    pub fn add(&mut self, name: &'static str, value: f64) {
        let sum = self.values.entry(name).or_default();
        *sum = measure::add(*sum, value);
    }
}

/// What `unit`'s model route counted since the last look, as its tokens
/// and model requests: from now on, its tokens are the route's alone.
fn add_tokens(unit: &mut Measured, t: &Tokens) {
    unit.metered = true;
    for (name, value) in [
        ("link.tokens.input.sum", t.input),
        ("link.tokens.output.sum", t.output),
        ("link.tokens.cached.sum", t.cached),
        ("link.tokens.cache_written.sum", t.written),
        ("link.requests.model.sum", t.requests),
        ("link.requests.unmetered.sum", t.unmetered),
    ] {
        unit.add(name, value as f64);
    }
}

/// Looks at the machine, remembering between looks what's slow to learn
/// (an agent's folders' size, a process's memory), the machine's
/// counters by device, and what each proxy had counted of its model's
/// requests.
#[derive(Default)]
pub struct Sampler {
    /// Provider commands from the same discovery as the current agent
    /// listing. Published sources take precedence without a launch.
    traffic: Vec<link_core::answers::Answerer>,
    sources: Vec<measure::traffic::Source>,
    disk: HashMap<String, (Instant, u64, u64)>,
    memory: HashMap<u32, (Option<u64>, Instant, u64)>,
    disks: Counters,
    networks: Counters,
    /// When the machine's GPUs were last looked at, and how busy they
    /// were: what they used since is that share of the time.
    gpus: Option<(Instant, f64)>,
    /// What agents' model routes had counted, and processes' GPU time,
    /// at the last look. The watcher keeps it across its restarts.
    pub counted: Counted,
}

/// What [`Sampler`] remembers of counts between looks.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Counted {
    /// What each agent's model route had counted at the last look, by
    /// its ID ([`measure::tokens`]): its tokens are what that grew by
    /// since. An agent whose route ever counted is metered: its
    /// harness's own count is never taken.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, Tokens>,
    /// Each process's GPU time, in seconds, at the last look it was
    /// seen, by [`proxy_key`]: what it used is what it grew by since.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gpus: BTreeMap<String, f64>,
}

link_files::specification!(Counted, include_str!("sample/counted.schema.toml"));

/// A process's key in [`Counted::gpus`]: its pid, and when it started,
/// so another process given the same pid isn't taken for it.
fn proxy_key(proc: &Proc) -> String {
    format!("{}@{}", proc.pid, proc.started.unwrap_or(0))
}

/// Whether the process [`proxy_key`] named still runs.
fn still_runs(key: &str) -> bool {
    let Some((pid, started)) = key.split_once('@') else {
        return false;
    };
    let (Ok(pid), Ok(started)) = (pid.parse::<u32>(), started.parse::<u64>()) else {
        return false;
    };
    os::process(pid, false).is_some_and(|p| p.started.unwrap_or(0) == started)
}

impl Sampler {
    /// The machine, then each agent of `agents` that runs, then its
    /// processes.
    pub fn sample(
        &mut self,
        machine: &link_machine::installation::Installation,
        agents: &[Agent],
        settings: &Settings,
    ) -> Vec<Measured> {
        let home = link_files::home_dir();
        let mut look = Look::new(&self.sources, &self.traffic);
        let mut out = Vec::new();
        let now = os::machine(&home, &mut look.nets);
        let mut unit = Measured::new(Unit {
            id: machine.id.clone(),
            kind: "machine".into(),
            name: Some(machine.name.clone()).filter(|n| plain(n)),
            ..Unit::default()
        });
        unit.started = now.booted;
        unit.set("link.memory.used.sum", now.memory.map(|v| v as f64));
        unit.set("link.cpu.used.sum", now.cpu);
        unit.set("link.processes.sum", now.processes.map(|v| v as f64));
        unit.set("link.ports.sum", now.ports.map(|v| v as f64));
        unit.set("link.disk.used.sum", now.disk_used.map(|v| v as f64));
        if let Some(disks) = &now.disks {
            let (read, written) = self.disks.add(disks);
            unit.set("link.disk.read.sum", read as f64);
            unit.set("link.disk.written.sum", written as f64);
        }
        if let Some(networks) = &now.networks {
            let (received, sent) = self.networks.add(networks);
            unit.set("link.network.in.sum", received as f64);
            unit.set("link.network.out.sum", sent as f64);
        }
        if let Some(gpus) = now.gpu {
            unit.set("link.gpu.memory.sum", gpus.memory as f64);
            // As busy since the last look as it was then: none at the
            // first.
            if let Some((at, busy)) = self.gpus {
                unit.set("link.gpu.used.sum", busy * at.elapsed().as_secs_f64());
            }
            self.gpus = Some((Instant::now(), gpus.busy));
        }
        out.push(unit);
        let mut gpus = BTreeSet::new();
        let link = link_files::installation_dir();
        for agent in agents {
            out.extend(self.agent(machine, agent, settings, &link, &mut look, &mut gpus));
        }
        // Nothing kept of processes gone; one this look missed (a listing
        // that failed) is kept, or its whole count would come again.
        self.counted
            .gpus
            .retain(|key, _| gpus.contains(key) || still_runs(key));
        // What's kept of processes gone.
        let running: BTreeSet<u32> = out
            .iter()
            .filter_map(|m| m.unit.id.rsplit_once('/')?.1.parse().ok())
            .collect();
        self.memory.retain(|pid, _| running.contains(pid));
        out
    }

    /// What the process [`proxy_key`] `key` names used of GPUs since
    /// the last look, given its busy time since it started, `now`: all
    /// of it the first time it's seen, or when its count started over.
    /// Its key goes in `seen`.
    fn gpu_since(&mut self, key: String, now: f64, seen: &mut BTreeSet<String>) -> f64 {
        let before = self.counted.gpus.insert(key.clone(), now);
        seen.insert(key);
        match before {
            Some(before) if before <= now => now - before,
            _ => now,
        }
    }

    /// An agent and its processes; its folders, and what its model route
    /// counted in Link's settings `link`, whether it runs or not. Its
    /// processes whose GPU time is read go in `gpus`, by [`proxy_key`].
    fn agent(
        &mut self,
        machine: &link_machine::installation::Installation,
        agent: &Agent,
        settings: &Settings,
        link: &Path,
        look: &mut Look,
        gpus: &mut BTreeSet<String>,
    ) -> Vec<Measured> {
        let mut unit = Measured::new(Unit {
            id: agent.id.clone(),
            kind: "agent".into(),
            parent: Some(machine.id.clone()),
            name: Some(agent.name.clone()).filter(|n| plain(n)),
            machine: Some(machine.id.clone()),
            owner: agent.owner.clone(),
            teams: agent.teams.clone(),
        });
        // What its model route counted since the last look, all of it
        // the first time; whether it runs or not, as a count may come
        // after its turn. Counted there, its harness's tokens aren't taken.
        if let Some(now) = measure::tokens::read(link, &agent.key) {
            let before = self.counted.tokens.insert(agent.id.clone(), now);
            add_tokens(&mut unit, &now.since(&before.unwrap_or_default()));
        }
        let (used, files) = self.folders(agent, settings, look);
        unit.set("link.disk.used.sum", used as f64);
        unit.set("link.disk.files.sum", files as f64);
        if !agent.running {
            return vec![unit];
        }
        let groups: Vec<(&Path, Vec<u32>)> = agent
            .groups
            .iter()
            .map(|g| (g.as_path(), group::group_pids(g)))
            .collect();
        let procs = self.processes(&pids(agent, &groups));
        unit.started = agent
            .pids
            .first()
            .and_then(|main| procs.iter().find(|p| p.pid == *main))
            .and_then(|p| p.started);
        unit.set(
            "link.memory.used.sum",
            procs.iter().filter_map(|p| p.memory).sum::<u64>() as f64,
        );
        // Each group's, every process it ever held; the rest, those
        // running.
        let (mut cpu, mut io) = ((0.0, Vec::new()), ((0, 0), Vec::new()));
        for (group, pids) in &groups {
            if let Some(used) = group::group_cpu(group) {
                cpu.0 += used;
                cpu.1.extend(pids);
            }
            if let Some((read, written)) = group::group_io(group) {
                io.0 = (io.0.0 + read, io.0.1 + written);
                io.1.extend(pids);
            }
        }
        for proc in &procs {
            if !cpu.1.contains(&proc.pid) {
                cpu.0 += proc.cpu;
            }
            if !io.1.contains(&proc.pid) {
                io.0 = (io.0.0 + proc.read, io.0.1 + proc.written);
            }
        }
        unit.set("link.cpu.used.sum", cpu.0);
        unit.set("link.disk.read.sum", io.0.0 as f64);
        unit.set("link.disk.written.sum", io.0.1 as f64);
        unit.set("link.processes.sum", procs.len() as f64);
        let mut ports = BTreeSet::new();
        let mut carried = None;
        let mut gpu: Option<(f64, u64)> = None;
        let mut out = Vec::new();
        for proc in &procs {
            let name = program(&proc.name);
            let sandbox = matches!(name, Some("bwrap" | "sandbox-exec"));
            let mut unit = Measured::new(Unit {
                id: format!("{}/{}", agent.id, proc.pid),
                kind: if sandbox { "sandbox" } else { "process" }.into(),
                parent: Some(agent.id.clone()),
                name: name.map(str::to_string),
                machine: Some(machine.id.clone()),
                ..Unit::default()
            });
            unit.started = proc.started;
            unit.set("link.memory.used.sum", proc.memory.map(|m| m as f64));
            unit.set("link.cpu.used.sum", proc.cpu);
            unit.set("link.disk.read.sum", proc.read as f64);
            unit.set("link.disk.written.sum", proc.written as f64);
            unit.set("link.processes.sum", 1.0);
            let own = os::ports(proc.pid, &mut look.nets);
            unit.set("link.ports.sum", own.len() as f64);
            ports.extend(own);
            if let Some((seconds, memory)) = look.gpu(proc.pid) {
                unit.set("link.gpu.memory.sum", memory as f64);
                // What it used since the last look: all of it, the
                // first time it's seen.
                let used = seconds.map(|now| self.gpu_since(proxy_key(proc), now, gpus));
                unit.set("link.gpu.used.sum", used);
                let (u, m) = gpu.unwrap_or((0.0, 0));
                gpu = Some((u + used.unwrap_or(0.0), m.saturating_add(memory)));
            }
            if let Some(Carried { up, down }) = look.carried(proc.pid) {
                unit.set("link.network.out.sum", up as f64);
                unit.set("link.network.in.sum", down as f64);
                let (u, d) = carried.unwrap_or((0, 0));
                carried = Some((u + up, d + down));
            }
            out.push(unit);
        }
        unit.set("link.ports.sum", ports.len() as f64);
        if let Some((used, memory)) = gpu {
            unit.set("link.gpu.used.sum", used);
            unit.set("link.gpu.memory.sum", memory as f64);
        }
        if let Some((up, down)) = carried {
            unit.set("link.network.out.sum", up as f64);
            unit.set("link.network.in.sum", down as f64);
        }
        out.insert(0, unit);
        out
    }

    /// Processes `pids` now, each one's memory read at most every
    /// [`MEMORY_EVERY`], and as it was in between.
    fn processes(&mut self, pids: &[u32]) -> Vec<Proc> {
        let mut procs = Vec::new();
        for pid in pids {
            let kept = self.memory.get(pid).copied();
            let due = kept.is_none_or(|(_, at, _)| at.elapsed() >= MEMORY_EVERY);
            let Some(mut proc) = os::process(*pid, due) else {
                continue;
            };
            match (proc.memory, kept) {
                (None, Some((started, _, memory))) if started == proc.started => {
                    proc.memory = Some(memory);
                }
                // Another process of the same pid.
                (None, _) => proc.memory = os::process(*pid, true).and_then(|p| p.memory),
                _ => {}
            }
            if let Some(memory) = proc.memory
                && (due || kept.is_none_or(|(started, ..)| started != proc.started))
            {
                self.memory
                    .insert(*pid, (proc.started, Instant::now(), memory));
            }
            procs.push(proc);
        }
        procs
    }

    /// The bytes and files of `agent`'s folders, walked at most every
    /// `folders` of the settings, and of all agents' due, one a look.
    fn folders(&mut self, agent: &Agent, settings: &Settings, look: &mut Look) -> (u64, u64) {
        if let Some((at, used, files)) = self.disk.get(&agent.id)
            && (at.elapsed() < settings.folders_every || look.walked)
        {
            return (*used, *files);
        }
        look.walked |= self.disk.contains_key(&agent.id);
        let (mut used, mut files) = (0, 0);
        for folder in &agent.folders {
            let (u, f) = walk(folder, settings.folders_max);
            used += u;
            files += f;
        }
        self.disk
            .insert(agent.id.clone(), (Instant::now(), used, files));
        (used, files)
    }
}

/// The processes `agent` runs as, by pid, sorted: those in its
/// services' `groups`, and the trees under its main processes outside
/// them (its start in a terminal; on a Mac, all of them).
fn pids(agent: &Agent, groups: &[(&Path, Vec<u32>)]) -> Vec<u32> {
    let mut pids: Vec<u32> = groups.iter().flat_map(|(_, p)| p).copied().collect();
    let outside: Vec<u32> = agent
        .pids
        .iter()
        .filter(|p| !pids.contains(p))
        .copied()
        .collect();
    pids.extend(tree(&outside));
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// Every process under `roots`, `roots` among them.
fn tree(roots: &[u32]) -> Vec<u32> {
    measure::process::tree(roots, os::alive, os::children)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        crate::settings::merge(&Default::default()).unwrap()
    }

    fn machine() -> link_machine::installation::Installation {
        link_machine::installation::Installation {
            id: "m1".into(),
            name: "here".into(),
        }
    }

    /// An agent running as this test: its process, measured, named by
    /// no name it chose.
    #[test]
    fn a_running_agent_is_its_processes() {
        let agent = Agent {
            id: "a1".into(),
            name: "main".into(),
            owner: Some("me@laptop".into()),
            teams: vec!["t1".into()],
            running: true,
            pids: vec![std::process::id()],
            ..Agent::default()
        };
        let mut sampler = Sampler::default();
        let units = sampler.sample(&machine(), &[agent], &settings());
        assert_eq!(units[0].unit.kind, "machine");
        assert_eq!(units[1].unit.kind, "agent");
        assert_eq!(units[1].unit.owner.as_deref(), Some("me@laptop"));
        assert_eq!(units[1].unit.teams, ["t1"]);
        assert!(units[1].values["link.memory.used.sum"] > 0.0);
        assert!(units[1].values["link.processes.sum"] >= 1.0);
        assert!(units[1].started.is_some());
        let me = format!("a1/{}", std::process::id());
        let mine = units.iter().find(|u| u.unit.id == me).unwrap();
        assert_eq!(mine.unit.parent.as_deref(), Some("a1"));
        // This test's program is named after it: not one Link knows.
        assert_eq!(mine.unit.name, None);
        assert!(units.iter().all(|u| u.unit.valid()), "{units:?}");
        // Its memory as read a moment ago, not read again.
        let again = sampler.sample(&machine(), &[units_agent()], &settings());
        assert!(again[1].values["link.memory.used.sum"] > 0.0);
    }

    fn units_agent() -> Agent {
        Agent {
            id: "a1".into(),
            name: "main".into(),
            running: true,
            pids: vec![std::process::id()],
            ..Agent::default()
        }
    }

    /// With a control group, an agent is its group's processes and its
    /// group's CPU and disk, and a main process outside the group (its
    /// connections' service's) is counted too, as its own tree.
    #[cfg(not(target_os = "macos"))]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
    fn an_agents_group_and_what_runs_outside_it_are_counted() {
        let dir = link_test_kit::TempDir::new("measure-group");
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        // The group holds the child; this test runs outside it.
        std::fs::write(dir.join("cgroup.procs"), format!("{}\n", child.id())).unwrap();
        std::fs::write(dir.join("cpu.stat"), "usage_usec 5000000\n").unwrap();
        std::fs::write(dir.join("io.stat"), "8:0 rbytes=100 wbytes=200\n").unwrap();
        let agent = Agent {
            id: "a1".into(),
            name: "main".into(),
            running: true,
            pids: vec![child.id(), std::process::id()],
            groups: vec![dir.to_path_buf()],
            ..Agent::default()
        };
        // Before the look: this test's process (other tests' threads)
        // reads on while it runs, and its counts only grow.
        let me = os::process(std::process::id(), false).unwrap();
        let units = Sampler::default().sample(&machine(), &[agent], &settings());
        let _ = child.kill();
        let _ = child.wait();
        let ids: BTreeSet<String> = units.iter().map(|u| u.unit.id.clone()).collect();
        assert!(ids.contains(&format!("a1/{}", child.id())), "{ids:?}");
        assert!(
            ids.contains(&format!("a1/{}", std::process::id())),
            "{ids:?}"
        );
        let cpu = units[1].values["link.cpu.used.sum"];
        assert!(cpu >= 5.0 + me.cpu - 0.5, "{cpu}");
        assert!(units[1].values["link.disk.read.sum"] >= 100.0 + me.read as f64);
    }

    /// An agent's tokens are what its model route counted since the last
    /// look: all of it the first time, never the same twice, running or
    /// stopped; an agent no route counted is its harness's to report.
    #[test]
    fn an_agents_tokens_are_what_its_model_route_counted_since_the_last_look() {
        let link = link_test_kit::TempDir::new("measure-agent-tokens");
        let me = std::process::id();
        let counted = |requests: u64, input: u64| {
            measure::tokens::add(
                &link,
                "a1",
                &Tokens {
                    requests,
                    unmetered: 1,
                    input,
                    output: 40,
                    cached: 100,
                    written: 7,
                },
            )
            .unwrap();
        };
        let agent = Agent {
            id: "a1".into(),
            key: "a1".into(),
            name: "main".into(),
            running: true,
            pids: vec![me],
            ..Agent::default()
        };
        let mut sampler = Sampler::default();
        let look = |sampler: &mut Sampler, agent: &Agent| {
            let mut look = Look::default();
            let units = sampler.agent(
                &machine(),
                agent,
                &settings(),
                &link,
                &mut look,
                &mut BTreeSet::new(),
            );
            units[0].clone()
        };
        // None counted yet: its harness's to report.
        let plain = look(&mut sampler, &agent);
        assert!(!plain.metered);
        assert!(!plain.values.contains_key("link.tokens.input.sum"));
        counted(2, 300);
        let first = look(&mut sampler, &agent);
        assert!(first.metered);
        for (name, value) in [
            ("link.tokens.input.sum", 300.0),
            ("link.tokens.output.sum", 40.0),
            ("link.tokens.cached.sum", 100.0),
            ("link.tokens.cache_written.sum", 7.0),
            ("link.requests.model.sum", 2.0),
            ("link.requests.unmetered.sum", 1.0),
        ] {
            assert_eq!(first.values[name], value, "{name}");
        }
        let unchanged = look(&mut sampler, &agent);
        assert!(unchanged.metered);
        assert_eq!(unchanged.values["link.tokens.input.sum"], 0.0);
        assert_eq!(unchanged.values["link.requests.model.sum"], 0.0);
        // Stopped, what its route counted after its turn is still taken.
        let stopped = Agent {
            running: false,
            ..agent.clone()
        };
        counted(1, 50);
        let next = look(&mut sampler, &stopped);
        assert!(next.metered);
        assert_eq!(next.values["link.tokens.input.sum"], 50.0);
        assert_eq!(next.values["link.requests.model.sum"], 1.0);
    }

    /// A process's GPU time is what it grew by since the last look: all
    /// of it when first seen or when it started over, and none of a
    /// process gone kept.
    #[test]
    fn a_processs_gpu_time_is_what_it_used_since_the_last_look() {
        let mut sampler = Sampler::default();
        let mut seen = BTreeSet::new();
        assert_eq!(sampler.gpu_since("7@1".into(), 2.0, &mut seen), 2.0);
        assert_eq!(sampler.gpu_since("7@1".into(), 2.5, &mut seen), 0.5);
        assert_eq!(sampler.gpu_since("7@1".into(), 2.5, &mut seen), 0.0);
        // Another process given the same pid: its own from its start.
        assert_eq!(sampler.gpu_since("7@9".into(), 1.0, &mut seen), 1.0);
        // A count that fell started over.
        assert_eq!(sampler.gpu_since("7@1".into(), 0.25, &mut seen), 0.25);
        assert_eq!(seen.len(), 2);
        // Neither runs: a look forgets both.
        sampler.sample(&machine(), &[], &settings());
        assert!(sampler.counted.gpus.is_empty());
    }

    /// What a process has on NVIDIA's GPUs is its memory there, and its
    /// agent's; its time there isn't the process's to say.
    #[test]
    fn a_processs_memory_on_nvidia_is_its_agents() {
        let me = std::process::id();
        let mut look = Look {
            nvidia: Some(BTreeMap::from([(me, 512 << 20)])),
            ..Look::default()
        };
        let link = link_test_kit::TempDir::new("measure-nvidia");
        let units = Sampler::default().agent(
            &machine(),
            &units_agent(),
            &settings(),
            &link,
            &mut look,
            &mut BTreeSet::new(),
        );
        let mine = units
            .iter()
            .find(|u| u.unit.id == format!("a1/{me}"))
            .unwrap();
        assert!(mine.values["link.gpu.memory.sum"] >= (512 << 20) as f64);
        assert!(units[0].values["link.gpu.memory.sum"] >= (512 << 20) as f64);
    }
}
