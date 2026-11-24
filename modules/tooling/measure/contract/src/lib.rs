//! Link's measures: what a unit (the machine, an agent, one of its
//! processes, a relay) uses or counts, as numbers only, and how they
//! cross programs: OTLP, the OpenTelemetry protocol, as JSON. Both
//! sides speak it: the measure module (`link_measure_contract::client`
//! re-exports it) and the relay, which counts what only it sees.
//!
//! The schema is closed. A measure is one of [`MEASURES`], each with its
//! unit and how it rolls up to its parent, which is the end of its name
//! (`link.memory.used.sum`); a unit is named by the attributes of
//! [`Unit`] and nothing else. Anything else in what a program sends (an
//! attribute, a description, a measure Link doesn't know, a value that
//! isn't a number) is dropped as it's read ([`read_otlp`]), so no text
//! of a conversation can ride along: there's no field to put it in.

use serde_json::{Value, json};

mod otlp;
pub use otlp::{metrics_url, otlp, read_otlp};

/// How a measure rolls up from a unit to its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rollup {
    /// The parent's is the sum of its children's (memory, tokens).
    Sum,
    /// The largest of its children's (a peak).
    Max,
    /// Not at all: it's the unit's own (a turn's time).
    Own,
}

impl Rollup {
    /// The end of a measure's name.
    pub fn suffix(self) -> &'static str {
        match self {
            Rollup::Sum => "sum",
            Rollup::Max => "max",
            Rollup::Own => "own",
        }
    }
}

/// How a measure's values come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// What it is now (memory in use).
    Gauge,
    /// A running total since the unit started (CPU used): OTLP's
    /// cumulative sum.
    Total,
    /// What one event added (a turn's tokens): OTLP's delta sum.
    Delta,
    /// One value per event (a turn's time to the model): a gauge with a
    /// point per event.
    Each,
}

/// Who says a measure's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    /// The measure module, from outside what it measures: no program
    /// may say it of itself.
    Outside,
    /// A harness, of itself, in the file `LNK_MEASURES` names: what
    /// only it counts (a turn's tokens, cost and times).
    Harness,
    /// The harness module, of the starts and stops it makes, in a file
    /// of the agent's settings (`times.jsonl`), where no harness writes.
    Module,
    /// A relay, of what it carried, sent to the OTLP endpoint its
    /// `--measures` names: what only it sees (a webhook it couldn't
    /// hand on).
    Relay,
    /// A service a workload is reached or served through, of what passed
    /// through it for that workload: the messages it handed on, what a
    /// model's requests were charged, a ceiling it refused at, the
    /// storage it keeps. No program says it of itself.
    Service,
}

/// One of Link's measures.
#[derive(Debug, PartialEq, Eq)]
pub struct Measure {
    /// `link.<dimension>.<measure>.<rollup>`.
    pub name: &'static str,
    /// UCUM, as OpenTelemetry writes units: `By`, `s`, `{token}`.
    pub unit: &'static str,
    pub rollup: Rollup,
    pub kind: Kind,
    pub by: By,
    /// What it is, in a few words.
    pub about: &'static str,
}

const fn measure(
    name: &'static str,
    unit: &'static str,
    rollup: Rollup,
    kind: Kind,
    by: By,
    about: &'static str,
) -> Measure {
    Measure {
        name,
        unit,
        rollup,
        kind,
        by,
        about,
    }
}

use By::{Harness, Module, Outside, Relay, Service};
use Kind::{Delta, Each, Gauge, Total};
use Rollup::{Max, Own, Sum};

/// Every measure, in the order they're shown.
pub const MEASURES: &[Measure] = &[
    measure(
        "link.time.run.own",
        "s",
        Own,
        Each,
        Outside,
        "elapsed wall time of a bounded execution",
    ),
    measure(
        "link.memory.used.sum",
        "By",
        Sum,
        Gauge,
        Outside,
        "memory in use (PSS on Linux, the footprint on a Mac)",
    ),
    measure(
        "link.memory.peak.max",
        "By",
        Max,
        Gauge,
        Outside,
        "the most memory in use seen",
    ),
    measure(
        "link.cpu.used.sum",
        "s",
        Sum,
        Total,
        Outside,
        "CPU used, in core-seconds",
    ),
    measure(
        "link.gpu.used.sum",
        "s",
        Sum,
        Delta,
        Outside,
        "GPU used, in GPU-seconds, since the last look",
    ),
    measure(
        "link.gpu.memory.sum",
        "By",
        Sum,
        Gauge,
        Outside,
        "GPU memory in use",
    ),
    measure(
        "link.disk.used.sum",
        "By",
        Sum,
        Gauge,
        Outside,
        "disk its folders take",
    ),
    measure(
        "link.disk.files.sum",
        "{file}",
        Sum,
        Gauge,
        Outside,
        "files in its folders",
    ),
    measure(
        "link.disk.read.sum",
        "By",
        Sum,
        Total,
        Outside,
        "read from disk",
    ),
    measure(
        "link.disk.written.sum",
        "By",
        Sum,
        Total,
        Outside,
        "written to disk",
    ),
    measure(
        "link.network.in.sum",
        "By",
        Sum,
        Total,
        Outside,
        "received through its proxy",
    ),
    measure(
        "link.network.out.sum",
        "By",
        Sum,
        Total,
        Outside,
        "sent through its proxy",
    ),
    measure(
        "link.processes.sum",
        "{process}",
        Sum,
        Gauge,
        Outside,
        "processes running",
    ),
    measure(
        "link.ports.sum",
        "{port}",
        Sum,
        Gauge,
        Outside,
        "TCP ports listened on",
    ),
    measure(
        "link.time.to_model.own",
        "s",
        Own,
        Each,
        Harness,
        "from a message in to the model asked",
    ),
    measure(
        "link.time.first_token.own",
        "s",
        Own,
        Each,
        Harness,
        "from a message in to the answer's first piece out",
    ),
    measure(
        "link.time.turn.own",
        "s",
        Own,
        Each,
        Harness,
        "from a message in to the whole answer out",
    ),
    measure(
        "link.time.start_cold.own",
        "s",
        Own,
        Each,
        Module,
        "a start from cold, its first since the machine started, until it answers",
    ),
    measure(
        "link.time.start_warm.own",
        "s",
        Own,
        Each,
        Module,
        "a start again since, until it answers",
    ),
    measure(
        "link.time.stop.own",
        "s",
        Own,
        Each,
        Module,
        "a stop, until none of its processes is left",
    ),
    measure(
        "link.turns.sum",
        "{turn}",
        Sum,
        Delta,
        Harness,
        "turns answered",
    ),
    measure(
        "link.tool_calls.sum",
        "{call}",
        Sum,
        Delta,
        Harness,
        "tools called",
    ),
    measure(
        "link.errors.sum",
        "{error}",
        Sum,
        Delta,
        Harness,
        "turns that failed, and tool calls",
    ),
    measure(
        "link.fallbacks.off.sum",
        "{turn}",
        Sum,
        Delta,
        Harness,
        "turns its fallback model answered: its model was off",
    ),
    measure(
        "link.fallbacks.window.sum",
        "{turn}",
        Sum,
        Delta,
        Harness,
        "turns its fallback model answered: the prompt was past its model's window",
    ),
    measure(
        "link.fallbacks.overloaded.sum",
        "{turn}",
        Sum,
        Delta,
        Harness,
        "turns its fallback model answered: its model's provider was busy or failing",
    ),
    measure(
        "link.tokens.input.sum",
        "{token}",
        Sum,
        Delta,
        Harness,
        "prompt tokens, cached or not",
    ),
    measure(
        "link.tokens.output.sum",
        "{token}",
        Sum,
        Delta,
        Harness,
        "answer tokens",
    ),
    measure(
        "link.tokens.cached.sum",
        "{token}",
        Sum,
        Delta,
        Harness,
        "of the prompt, read from the provider's cache",
    ),
    measure(
        "link.tokens.cache_written.sum",
        "{token}",
        Sum,
        Delta,
        // Its proxy's to count, as no harness reports it: a harness that
        // said it would add to an unmetered agent's spend unchecked.
        Outside,
        "of the prompt, written to the provider's cache",
    ),
    measure(
        "link.requests.model.sum",
        "{request}",
        Sum,
        Delta,
        Outside,
        "requests for a model's answer its proxy carried",
    ),
    measure(
        "link.requests.unmetered.sum",
        "{request}",
        Sum,
        Delta,
        Outside,
        "of them, answers that gave no usage: their tokens aren't counted",
    ),
    measure(
        "link.cost.sum",
        "{microdollar}",
        Sum,
        Delta,
        Harness,
        "what the provider charged, in millionths of a dollar",
    ),
    measure(
        "link.cost.charged.sum",
        "{microdollar}",
        Sum,
        Delta,
        Service,
        "what its owner was charged for its model requests, in millionths of a dollar",
    ),
    measure(
        "link.messages.arrived.sum",
        "{message}",
        Sum,
        Delta,
        Service,
        "messages that arrived for it",
    ),
    measure(
        "link.messages.answered.sum",
        "{message}",
        Sum,
        Delta,
        Service,
        "of them, ones it answered",
    ),
    measure(
        "link.messages.queued.sum",
        "{message}",
        Sum,
        Delta,
        Service,
        "of them, ones held until it could take them",
    ),
    measure(
        "link.messages.refused.sum",
        "{message}",
        Sum,
        Delta,
        Service,
        "of them, ones refused before it saw them",
    ),
    measure(
        "link.notices.sum",
        "{notice}",
        Sum,
        Delta,
        Service,
        "notices its owner saw from Link rather than from it",
    ),
    measure(
        "link.refusals.ceiling_hour.sum",
        "{request}",
        Sum,
        Delta,
        Service,
        "model requests refused at a provider's spending ceiling for the hour",
    ),
    measure(
        "link.refusals.ceiling_day.sum",
        "{request}",
        Sum,
        Delta,
        Service,
        "model requests refused at a provider's spending ceiling for the day",
    ),
    measure(
        "link.refusals.ceiling_new.sum",
        "{request}",
        Sum,
        Delta,
        Service,
        "model requests refused at the day's ceiling new accounts share",
    ),
    measure(
        "link.storage.home.sum",
        "By",
        Sum,
        Gauge,
        Service,
        "its Home's files where they're stored",
    ),
    measure(
        "link.storage.database.sum",
        "By",
        Sum,
        Gauge,
        Service,
        "its rows where its state is stored",
    ),
    measure(
        "link.decrypts.sum",
        "{decrypt}",
        Sum,
        Delta,
        Service,
        "decryptions a key management service made for it",
    ),
    measure(
        "link.hooks.undelivered.sum",
        "{hook}",
        Sum,
        Total,
        Relay,
        "webhooks its relay couldn't hand on through its tunnel",
    ),
    measure(
        "link.relay.connections.sum",
        "{connection}",
        Sum,
        Gauge,
        Relay,
        "agent connections held by the relay, including standbys",
    ),
    measure(
        "link.relay.requests.sum",
        "{request}",
        Sum,
        Total,
        Relay,
        "requests it was asked, of a relay or one of its schemes",
    ),
    measure(
        "link.relay.refusals.sum",
        "{request}",
        Sum,
        Total,
        Relay,
        "of them, ones the relay refused itself, with a 4xx of its own",
    ),
    measure(
        "link.relay.failures.sum",
        "{request}",
        Sum,
        Total,
        Relay,
        "of them, ones the relay failed itself, with a 5xx of its own",
    ),
    measure(
        "link.relay.unadmitted.sum",
        "{request}",
        Sum,
        Total,
        Relay,
        "requests the relay refused at admission, before any routing",
    ),
    measure(
        "link.relay.bytes.sum",
        "By",
        Sum,
        Total,
        Relay,
        "encoded payload bytes carried to and from connected agents",
    ),
];

/// The measure named `name`, if Link has it.
pub fn find(name: &str) -> Option<&'static Measure> {
    MEASURES.iter().find(|m| m.name == name)
}

/// The kinds of unit.
pub const KINDS: &[&str] = &[
    "machine", "agent", "sandbox", "process", "relay", "scheme", "job", "stage", "run", "task",
    "provider",
];

/// The attributes that name a unit, the only ones a measure carries;
/// an agent's owner and teams are ones of their own.
pub const ID: &str = "link.unit.id";
pub const KIND: &str = "link.unit.kind";
pub const PARENT: &str = "link.unit.parent";
pub const NAME: &str = "link.unit.name";
pub const MACHINE: &str = "link.machine.id";
pub const OWNER: &str = "link.owner.id";
/// An agent's teams, by their IDs: an array of strings.
pub const TEAMS: &str = "link.team.ids";
/// OpenTelemetry's own: the program that reported it.
pub const SERVICE: &str = "service.name";

/// Whether `value` may be an attribute's value: an ID or a name, never
/// text (`[A-Za-z0-9._:/@-]`, at most 64).
pub fn plain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/@-".contains(&b))
}

/// The programs a process or sandbox unit may be named by: Link's own,
/// its sandboxes', and what harnesses run on. Any other's name is
/// whatever it chose (a script's file name), so it has none.
pub const PROGRAMS: &[&str] = &[
    // `lnk` and every module Link releases.
    "lnk",
    "lnk-agent",
    "lnk-archive",
    "lnk-aws",
    "lnk-box",
    "lnk-bundle",
    "lnk-storage",
    "lnk-bucket-store",
    "lnk-cloud",
    "lnk-deepseek",
    "lnk-discord",
    "lnk-env",
    "lnk-file-store",
    "lnk-file-tool",
    "lnk-gcp",
    "lnk-hermes",
    "lnk-browser-tool",
    "lnk-code-tool",
    "lnk-desktop-tool",
    "lnk-document-converter",
    "lnk-harness",
    "lnk-mini-harness",
    "lnk-memory-tool",
    "lnk-run-tool",
    "lnk-scheduler-tool",
    "lnk-web-tool",
    "lnk-manual",
    "lnk-mcp",
    "lnk-measure",
    "lnk-model",
    "lnk-openclaw",
    "lnk-sandbox",
    "lnk-secret",
    "lnk-signal",
    "lnk-slack",
    "lnk-sqlite-store",
    "lnk-spec",
    "lnk-telegram",
    "lnk-webhook-connection",
    // Its sandboxes', and what harnesses run on.
    "bwrap",
    "sandbox-exec",
    "node",
    "bun",
    "deno",
    "python",
    "python3",
    "uv",
];

/// Of [`PROGRAMS`], the one a process's name as its system keeps it
/// is: Linux cuts names to 15 bytes.
pub fn program(name: &str) -> Option<&'static str> {
    PROGRAMS
        .iter()
        .find(|p| **p == name || name.len() == 15 && p.starts_with(name))
        .copied()
}

/// `a + b`, at most the largest number there is: a sum of measures is
/// never infinite, which JSON can't write.
pub fn add(a: f64, b: f64) -> f64 {
    let sum = a + b;
    if sum.is_finite() { sum } else { f64::MAX }
}

/// What's measured: the machine, an agent, one of its processes, a
/// relay.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unit {
    /// The machine's and an agent's are theirs (`agents.toml`, the
    /// agent's settings); a process's is `<agent id>/<pid>`; a relay's, its
    /// domain; a relay's scheme's, `<relay id>/<scheme>`.
    pub id: String,
    /// One of [`KINDS`].
    pub kind: String,
    /// Where it runs: the machine's ID for an agent, the agent's for a
    /// process, the relay's for a scheme; none for the machine or a relay.
    pub parent: Option<String>,
    /// What people call it: the agent's name, a process's program (one
    /// of [`PROGRAMS`]), a relay's role (`hooks`), a scheme's name
    /// (`webhooks`). A provider's ID is its name, as Link's model facts
    /// write it (`anthropic`).
    pub name: Option<String>,
    /// The machine it's on.
    pub machine: Option<String>,
    /// An agent's owner, as its settings say.
    pub owner: Option<String>,
    /// The teams an agent is in, by their IDs, as their files say.
    pub teams: Vec<String>,
}

impl Unit {
    /// Whether it's one the schema takes: a known kind, every value
    /// [`plain`], and a process or sandbox named only by one of
    /// [`PROGRAMS`].
    pub fn valid(&self) -> bool {
        let program = match self.kind.as_str() {
            "process" | "sandbox" => self.name.as_deref().is_none_or(|n| PROGRAMS.contains(&n)),
            _ => true,
        };
        KINDS.contains(&self.kind.as_str())
            && program
            && plain(&self.id)
            && [&self.parent, &self.name, &self.machine, &self.owner]
                .into_iter()
                .flatten()
                .chain(&self.teams)
                .all(|v| plain(v))
    }

    fn attributes(&self, service: &str) -> Vec<Value> {
        let mut pairs = vec![(SERVICE, service), (ID, &self.id), (KIND, &self.kind)];
        for (key, value) in [
            (PARENT, &self.parent),
            (NAME, &self.name),
            (MACHINE, &self.machine),
            (OWNER, &self.owner),
        ] {
            if let Some(value) = value {
                pairs.push((key, value));
            }
        }
        let mut attributes: Vec<Value> = pairs
            .into_iter()
            .map(|(key, value)| json!({"key": key, "value": {"stringValue": value}}))
            .collect();
        if !self.teams.is_empty() {
            let values: Vec<Value> = self
                .teams
                .iter()
                .map(|team| json!({"stringValue": team}))
                .collect();
            attributes.push(json!({"key": TEAMS, "value": {"arrayValue": {"values": values}}}));
        }
        attributes
    }
}

/// One value of a measure.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub measure: &'static Measure,
    pub value: f64,
    /// When, in nanoseconds since 1970.
    pub at: u64,
    /// For a [`Kind::Total`], when its count began; for a
    /// [`Kind::Delta`], the start of what it counts.
    pub start: Option<u64>,
}

impl Point {
    /// `value` of the measure `name`, at `at`. Panics on a name Link
    /// doesn't have: the names are Link's own.
    pub fn new(name: &str, value: f64, at: u64) -> Self {
        Self {
            measure: find(name).unwrap_or_else(|| panic!("no measure {name}")),
            value,
            at,
            start: None,
        }
    }

    pub fn since(mut self, start: u64) -> Self {
        self.start = Some(start);
        self
    }
}

/// One unit's points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub unit: Unit,
    pub points: Vec<Point>,
}

/// Now, in nanoseconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;

/// Measurement files and process helpers.
pub mod client;

#[cfg(test)]
link_test_kit::program!(system);
