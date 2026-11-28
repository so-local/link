//! What `lnk measure` shows: each unit now and over the last day. With
//! the watcher running, that's its last look and the day it keeps up
//! ([`NOW`]); without, a look as the command runs, and the day as the
//! watcher kept it. The machine is a tree: its agents, and each agent's
//! processes.

use std::collections::BTreeMap;
use std::path::Path;

use link_format::{
    Done, Exit, Foreign, Out, Report, Section, Table, Text, Value as Cell, WithinExt, cmd, phrase,
    prose, sentence,
};
use link_measure_contract::client::{Kind, MEASURES, Unit};
use serde_json::{Value, json};

use crate::sample::{Measured, Sampler};
use crate::settings::{self, Settings};
use crate::store::{Days, Store};

const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;

/// The watcher's last look, as `lnk measure --json` prints it, in the
/// measure module's folder: `lnk measure` reads it rather than look
/// again and read the day back, and `lnk measure --json --last` prints
/// it.
pub const NOW: &str = "now.json";

/// [`NOW`]'s schema: the last look as `lnk measure --json` prints it,
/// naming its schema in the file alone.
pub static NOW_FORMAT: std::sync::LazyLock<link_files::schema::Format> =
    std::sync::LazyLock::new(|| {
        link_files::schema::Format::shipped(include_str!("view/now.schema.toml"))
    });

/// The watcher's last look ([`NOW_FORMAT`]), which an upgrade converts.
pub fn saved_now() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::shipped::<Value>(&NOW_FORMAT)
}

/// The lock a running watcher holds, in the same folder.
pub const WATCH_LOCK: &str = "watch.lock";

/// The watcher's last look ([`NOW`]) in `dir`, when a watcher runs and,
/// given `fresh`, looked within that many seconds of `now`.
pub fn last_look(dir: &Path, fresh: Option<u64>, now: u64) -> Option<Value> {
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join(WATCH_LOCK))
        .ok()?;
    if lock.try_lock().is_ok() {
        return None;
    }
    let last: Value = NOW_FORMAT
        .read_json(&std::fs::read(dir.join(NOW)).ok()?)
        .ok()?
        .spec;
    let at = last["at"].as_u64()?;
    fresh
        .is_none_or(|fresh| now.saturating_sub(at) <= fresh)
        .then_some(last)
}

/// `lnk measure --json --last`: the watcher's last look, however old,
/// or `null` when no watcher runs; never a look taken now, so it's
/// quick. Read by `lnk agent list`.
pub fn last(out: &mut Out) -> Result<Done, Report> {
    let now = link_measure_contract::client::now() / 1_000_000_000;
    let last = last_look(&settings::dir().map_err(settings::unusable)?, None, now);
    out.reply(&last.unwrap_or(Value::Null));
    Ok(Done::shown())
}

/// Where the last day starts, at `now`: the start of the hour after the
/// one a day ago, so it's 24 of them.
pub fn day_start(now: u64) -> u64 {
    (now.saturating_sub(DAY) / HOUR + 1) * HOUR
}

/// One unit as `--json` prints it.
fn unit_json(
    unit: &Unit,
    now_of: Option<&BTreeMap<&'static str, f64>>,
    day: &BTreeMap<&'static str, f64>,
) -> Value {
    let mut measures = serde_json::Map::new();
    for m in MEASURES {
        let mut now = now_of.and_then(|n| n.get(m.name)).copied();
        let mut day_value = day.get(m.name).copied();
        // Counted only per turn, by its harness: nothing "now".
        if matches!(m.kind, Kind::Delta | Kind::Each) {
            now = None;
        }
        // The most in use: of the last day, and now.
        if m.name == "link.memory.peak.max" {
            let used = |n: Option<f64>, d: Option<f64>| match (n, d) {
                (Some(n), Some(d)) => Some(n.max(d)),
                (n, d) => n.or(d),
            };
            let (used_now, used_day) = (
                now_of.and_then(|n| n.get("link.memory.used.sum")).copied(),
                day.get("link.memory.used.sum").copied(),
            );
            day_value = used(used_now, used_day);
            now = day_value;
        }
        if now.is_some() || day_value.is_some() {
            measures.insert(m.name.into(), json!({"now": now, "day": day_value}));
        }
    }
    json!({
        "id": unit.id,
        "kind": unit.kind,
        "parent": unit.parent,
        "name": unit.name,
        "owner": unit.owner,
        "teams": unit.teams,
        "measures": measures,
    })
}

/// `units` now and over the last day as `days` has it, at `now`, as
/// `lnk measure --json` prints them: `{"at", "units": [...]}`.
pub fn json(units: &[Measured], days: &Days, now: u64) -> Value {
    let since = day_start(now);
    let units: Vec<Value> = units
        .iter()
        .map(|m| unit_json(&m.unit, Some(&m.values), &days.day(&m.unit.id, since)))
        .collect();
    json!({"at": now, "units": units})
}

/// The watcher's last look ([`NOW`]), when one runs and looked lately.
fn watched(dir: &Path, settings: &Settings, now: u64) -> Option<Value> {
    last_look(dir, Some(3 * settings.every.as_secs() + 10), now)
}

/// Whether unit `unit` (as `--json` prints it) is the one `which`
/// names: by its ID, or an agent or the machine by its name.
fn names(unit: &Value, which: &str) -> bool {
    unit["name"] == which && unit["kind"] == "agent"
}

/// An agent by its name, or one of its processes by its ID.
pub struct Pick {
    pub agent: String,
    pub pid: Option<u32>,
}

/// The unit `which` picks of `units` (as `--json` prints them).
fn pick_of<'a>(units: &'a [Value], which: &Pick) -> Option<&'a Value> {
    let agent = units.iter().find(|u| names(u, &which.agent))?;
    let Some(pid) = which.pid else {
        return Some(agent);
    };
    let tail = format!("/{pid}");
    units.iter().find(|u| {
        u["parent"] == agent["id"] && u["id"].as_str().is_some_and(|id| id.ends_with(&tail))
    })
}

/// Everything `lnk measure [--agent <agent>] --json` prints: `{"at", "units":
/// [...]}`, and for one unit, `"hours"`: the last day an hour at a time.
pub fn gather(which: Option<&Pick>) -> Result<Value, Report> {
    let now = link_measure_contract::client::now() / 1_000_000_000;
    let since = day_start(now);
    let dir = settings::dir().map_err(settings::unusable)?;
    let settings = settings::load().map_err(settings::unusable)?;
    let store = Store::new(dir.clone());
    // Each unit now: the watcher's last look, else one taken now.
    let watched = watched(&dir, &settings, now);
    let Some(which) = which else {
        if let Some(last) = watched {
            return Ok(last);
        }
        let live = look(&settings)?;
        let days = Days::read(&store, since, u64::MAX);
        return Ok(json(&live, &days, now));
    };
    let live: Vec<Value> = match watched {
        Some(last) => last["units"].as_array().cloned().unwrap_or_default(),
        None => json(&look(&settings)?, &Days::default(), now)["units"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    };
    let mut picked: Vec<Value> = Vec::new();
    if let Some(unit) = pick_of(&live, which) {
        picked.push(unit.clone());
        picked.extend(
            live.iter()
                .filter(|u| u["parent"] == unit["id"] && u["kind"] != "agent")
                .cloned(),
        );
    }
    let days = Days::read(&store, since, u64::MAX);
    // Not here now: as the watcher last kept it.
    if picked.is_empty() {
        let kept: Vec<Value> = days
            .units()
            .map(|k| unit_json(&k.unit(), None, &BTreeMap::new()))
            .collect();
        let Some(unit) = pick_of(&kept, which) else {
            return Err(Report::error(sentence!(
                "There is no unit {} here.",
                Foreign::new(&which.agent)
            ))
            .exit(Exit::Usage)
            .next(cmd!("lnk measure"), phrase!("list the units")));
        };
        picked.push(unit.clone());
        picked.extend(
            kept.iter()
                .filter(|u| u["parent"] == unit["id"] && u["kind"] != "agent")
                .cloned(),
        );
    }
    // Each one now, and its day as kept.
    let picked: Vec<Value> = picked
        .iter()
        .map(|u| {
            let unit = unit_of(u);
            unit_json(&unit, Some(&now_of(u)), &days.day(&unit.id, since))
        })
        .collect();
    let id = picked[0]["id"].as_str().unwrap_or_default().to_string();
    let hours: Vec<Value> = days
        .hours(&id, since)
        .into_iter()
        .map(|(at, measures)| json!({"at": at, "measures": measures}))
        .collect();
    Ok(json!({"at": now, "units": picked, "hours": hours}))
}

/// The unit `unit` (as `--json` prints it) is.
fn unit_of(unit: &Value) -> Unit {
    let text = |key: &str| unit[key].as_str().map(str::to_string);
    Unit {
        id: text("id").unwrap_or_default(),
        kind: text("kind").unwrap_or_default(),
        parent: text("parent"),
        name: text("name"),
        owner: text("owner"),
        teams: unit["teams"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_string))
            .collect(),
        machine: None,
    }
}

/// Each measure's value now, of `unit` as `--json` prints it.
fn now_of(unit: &Value) -> BTreeMap<&'static str, f64> {
    MEASURES
        .iter()
        .filter_map(|m| Some((m.name, unit["measures"][m.name]["now"].as_f64()?)))
        .collect()
}

/// The machine and its agents, looked at now.
fn look(settings: &Settings) -> Result<Vec<Measured>, Report> {
    let machine = link_machine::installation::installation().within("reading this machine's ID")?;
    let mut sampler = Sampler::default();
    let agents = sampler.agents();
    Ok(sampler.sample(&machine, &agents, settings))
}

/// `12.3 MB`.
fn bytes(n: Option<f64>) -> String {
    match n {
        None => "-".into(),
        Some(n) if n < 1e3 => format!("{n:.0} B"),
        Some(n) if n < 1e6 => format!("{:.1} kB", n / 1e3),
        Some(n) if n < 1e9 => format!("{:.1} MB", n / 1e6),
        Some(n) => format!("{:.1} GB", n / 1e9),
    }
}

fn seconds(n: Option<f64>) -> String {
    match n {
        None => "-".into(),
        Some(n) if n < 10.0 => format!("{n:.2}s"),
        Some(n) => format!("{n:.0}s"),
    }
}

fn count(n: Option<f64>) -> String {
    n.map_or("-".into(), |n| format!("{n:.0}"))
}

/// Micro-dollars as dollars.
fn money(n: Option<f64>) -> String {
    n.map_or("-".into(), |n| format!("${:.2}", n / 1e6))
}

/// A value as its unit says.
fn shown(unit: &str, n: Option<f64>) -> String {
    match unit {
        "By" => bytes(n),
        "s" => seconds(n),
        "{microdollar}" => money(n),
        _ => count(n),
    }
}

fn get(unit: &Value, name: &str, when: &str) -> Option<f64> {
    unit["measures"][name][when].as_f64()
}

fn label(unit: &Value) -> String {
    let name = unit["name"].as_str().unwrap_or("-");
    let text = match unit["kind"].as_str() {
        Some("process" | "sandbox") => {
            let pid = unit["id"]
                .as_str()
                .and_then(|i| i.rsplit('/').next())
                .unwrap_or("");
            format!("    {name} {pid}")
        }
        Some("agent") => format!("  {name}"),
        _ => name.to_string(),
    };
    link_format::clean(&text)
}

/// `lnk measure [--agent <agent>] [--json]`.
pub fn show(out: &mut Out, which: Option<&Pick>) -> Result<Done, Report> {
    let gathered = gather(which)?;
    out.reply(&gathered);
    if out.is_json() {
        return Ok(Done::shown());
    }
    let units = gathered["units"].as_array().cloned().unwrap_or_default();
    Ok(match which {
        None => tree(out, &units),
        Some(_) => one(
            out,
            &units,
            gathered["hours"].as_array().cloned().unwrap_or_default(),
        ),
    })
}

fn tree(out: &mut Out, units: &[Value]) -> Done {
    let mut table = Table::new(
        [
            "Unit",
            "Kind",
            "Memory",
            "CPU",
            "Processes",
            "Network in/out",
            "Tokens in/out (day)",
            "Turns (day)",
            "Spend (day)",
        ],
        sentence!("Nothing is measured here yet."),
    )
    .right(2)
    .right(3)
    .right(4)
    .right(7)
    .right(8);
    // The machine, then each agent with its processes under it.
    let mut ordered: Vec<&Value> = units.iter().filter(|u| u["kind"] == "machine").collect();
    for agent in units.iter().filter(|u| u["kind"] == "agent") {
        ordered.push(agent);
        ordered.extend(units.iter().filter(|u| u["parent"] == agent["id"]));
    }
    for unit in ordered {
        let network = match (
            get(unit, "link.network.in.sum", "now"),
            get(unit, "link.network.out.sum", "now"),
        ) {
            (None, None) => "-".into(),
            (i, o) => format!("{} / {}", bytes(i), bytes(o)),
        };
        let tokens = match (
            get(unit, "link.tokens.input.sum", "day"),
            get(unit, "link.tokens.output.sum", "day"),
        ) {
            (None, None) => "-".into(),
            (i, o) => format!("{} / {}", count(i), count(o)),
        };
        table.row([
            Cell::from(Foreign::new(&label(unit))),
            Foreign::new(unit["kind"].as_str().unwrap_or("")).into(),
            bytes(get(unit, "link.memory.used.sum", "now")).into(),
            seconds(get(unit, "link.cpu.used.sum", "now")).into(),
            count(get(unit, "link.processes.sum", "now")).into(),
            network.into(),
            tokens.into(),
            count(get(unit, "link.turns.sum", "day")).into(),
            money(get(unit, "link.cost.sum", "day")).into(),
        ]);
    }
    out.block(table);
    out.block(Text(prose!("CPU is core-seconds since it started.")));
    Done::shown()
        .next(
            cmd!("lnk measure show"),
            phrase!("see this machine's resource capacity"),
        )
        .next(
            cmd!("lnk measure --agent <agent>"),
            phrase!("see one, now and over the last day"),
        )
        .next(
            cmd!("lnk measure start"),
            phrase!("keep watching in the background"),
        )
}

fn one(out: &mut Out, units: &[Value], hours: Vec<Value>) -> Done {
    let Some(unit) = units.first() else {
        return Done::shown();
    };
    let mut table = Table::new(
        ["Measure", "Now", "Last day"],
        sentence!("Nothing is measured for it."),
    )
    .right(1)
    .right(2);
    for m in MEASURES {
        let (now, day) = (get(unit, m.name, "now"), get(unit, m.name, "day"));
        if now.is_none() && day.is_none() {
            continue;
        }
        table.row([m.about.to_string(), shown(m.unit, now), shown(m.unit, day)]);
    }
    out.block(
        Section::new(phrase!(
            "{} ({})",
            Foreign::new(label(unit).trim()),
            Foreign::new(unit["kind"].as_str().unwrap_or(""))
        ))
        .with(table),
    );
    if !hours.is_empty() {
        let mut table = Table::new(
            ["Hour (UTC)", "Memory (most)", "CPU", "Turns", "Spend"],
            sentence!("Nothing was kept for the last day."),
        )
        .right(1)
        .right(2)
        .right(3)
        .right(4);
        for hour in &hours {
            let at = hour["at"].as_u64().unwrap_or(0);
            let value = |name: &str| hour["measures"][name].as_f64();
            table.row([
                format!("{:02}:00", at % DAY / HOUR),
                bytes(value("link.memory.used.sum")),
                seconds(value("link.cpu.used.sum")),
                count(value("link.turns.sum")),
                money(value("link.cost.sum")),
            ]);
        }
        out.block(table);
    }
    let children: Vec<&Value> = units.iter().skip(1).collect();
    if !children.is_empty() {
        let mut table = Table::new(
            ["Process", "Memory", "CPU", "Ports"],
            sentence!("It has no processes."),
        )
        .right(1)
        .right(2)
        .right(3);
        for child in children {
            table.row([
                Cell::from(Foreign::new(label(child).trim())),
                bytes(get(child, "link.memory.used.sum", "now")).into(),
                seconds(get(child, "link.cpu.used.sum", "now")).into(),
                count(get(child, "link.ports.sum", "now")).into(),
            ]);
        }
        out.block(table);
    }
    Done::shown().next(cmd!("lnk measure"), phrase!("see every unit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--json` shows an agent's teams, and they're read back from it.
    #[test]
    fn an_agents_teams_are_shown() {
        let unit = Unit {
            id: "a1".into(),
            kind: "agent".into(),
            teams: vec!["t1".into()],
            ..Unit::default()
        };
        let json = unit_json(&unit, None, &BTreeMap::new());
        assert_eq!(json["teams"], json!(["t1"]));
        assert_eq!(unit_of(&json), unit);
    }
}
