use anyhow::{Context, bail, ensure};
use link_harness_contract::asp::schedules::{
    Destination, NotCarried, Origin, Schedule, ScheduleId, Schedules, When,
};
use link_harness_contract::asp::schedules::{DestinationThread, WhenAt, WhenCron, WhenEvery};
use link_harness_contract::harness::schedules as source;
use serde_json::Value;
use std::path::Path;

const ADAPTER: &str = "hermes";
const JOBS: &str = "cron/jobs.json";
const ARCHIVE: &str = ".lnk/schedule-imports";
const EMPTY: &[u8] = br#"{"version":1,"jobs":[]}"#;

pub fn clear_root(home: &Path, revision: &str, context: Option<&str>) -> anyhow::Result<()> {
    let still_original = source::read(&home.join(JOBS))?
        .is_some_and(|bytes| link_digest::sha256(&bytes) == revision);
    if let Some(context) = context.filter(|c| !c.is_empty() && still_original) {
        ensure!(
            link_digest::sha256(zone(home)?.as_bytes()) == context,
            "native schedule timezone changed after export; export again before clearing"
        );
    }
    source::clear(&home.join(JOBS), &home.join(ARCHIVE), revision, EMPTY)
}

pub fn export(home: &Path) -> anyhow::Result<Schedules> {
    export_root(&home.join(".hermes"))
}

pub fn export_root(home: &Path) -> anyhow::Result<Schedules> {
    let Some(bytes) = source::read(&home.join(JOBS))? else {
        return Ok(Schedules::default());
    };
    let raw: Value = serde_json::from_slice(&bytes).context("reading native cron/jobs.json")?;
    let jobs = native_jobs(&raw)?;
    let mut out = Schedules {
        source_revision: Some(link_digest::sha256(&bytes)),
        ..Default::default()
    };
    let mut ids = std::collections::BTreeSet::new();
    for (index, job) in jobs.iter().enumerate() {
        let id = job["id"].as_str().filter(|s| !s.is_empty());
        ensure!(
            id.is_none_or(|id| ids.insert(id)),
            "duplicate native schedule identity"
        );
        match convert(home, job).and_then(|job| {
            link_scheduler_tool_contract::timing::check(&job)?;
            Ok(job)
        }) {
            Ok(job) => {
                job.check()?;
                out.carried.push(job);
            }
            Err(err) => out.not_carried.push(NotCarried {
                job: id
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("record:{index}")),
                name: job["name"].as_str().unwrap_or("unnamed job").into(),
                reason: format!("{err:#}"),
            }),
        }
    }
    if let Some(zone) = out.carried.iter().find_map(|job| match &job.when {
        When::Cron(WhenCron { zone, .. }) => Some(zone),
        _ => None,
    }) {
        out.source_context = Some(link_digest::sha256(zone.as_bytes()));
    }
    Ok(out)
}

fn required<'a>(job: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    job[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .with_context(|| format!("missing {key}"))
}

fn instant(text: &str) -> anyhow::Result<u64> {
    let timestamp: jiff::Timestamp = text
        .parse()
        .context("timestamp must include its UTC offset")?;
    ensure!(
        timestamp.subsec_nanosecond() == 0,
        "subsecond timing cannot be represented"
    );
    Ok(u64::try_from(timestamp.as_second())?)
}

fn finish(job: &Value, when: When, message: &str, to: Destination) -> anyhow::Result<Schedule> {
    let id = required(job, "id")?;
    let schedule = Schedule {
        id: ScheduleId(format!("{ADAPTER}:{id}")),
        name: required(job, "name")?.into(),
        when,
        message: message.into(),
        to,
        enabled: job["enabled"].as_bool().context("missing enabled flag")?,
        revision: 1,
        origin: Some(Origin {
            adapter: ADAPTER.into(),
            job: id.into(),
        }),
    };
    schedule.check()?;
    Ok(schedule)
}

fn native_jobs(raw: &Value) -> anyhow::Result<&Vec<Value>> {
    if let Some(jobs) = raw.as_array() {
        return Ok(jobs);
    }
    raw["jobs"]
        .as_array()
        .context("Hermes jobs must be an array")
}

fn zone(home: &Path) -> anyhow::Result<String> {
    // Read the profile's configured value, never this adapter process's TZ.
    if let Some(bytes) = source::read(&home.join(".env"))? {
        let text = String::from_utf8(bytes)?;
        if let Some(value) = text
            .lines()
            .filter_map(|l| {
                l.trim()
                    .trim_start_matches("export ")
                    .strip_prefix("HERMES_TIMEZONE=")
            })
            .next()
        {
            return Ok(value.trim().trim_matches(['\'', '"']).into());
        }
    }
    let bytes = source::read(&home.join("config.yaml"))?
        .context("Hermes cron needs its configured timezone")?;
    let config: serde_norway::Value = serde_norway::from_slice(&bytes)?;
    Ok(config["timezone"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Hermes cron has no explicit timezone")?
        .into())
}

fn convert(home: &Path, job: &Value) -> anyhow::Result<Schedule> {
    for key in [
        "script",
        "monitor_script",
        "monitor_url",
        "model",
        "provider",
        "base_url",
        "context_from",
        "enabled_toolsets",
        "workdir",
        "reasoning_effort",
        "failure_deliver",
    ] {
        ensure!(job[key].is_null(), "{key} has native execution semantics");
    }
    ensure!(
        job["skills"].as_array().is_none_or(Vec::is_empty)
            && job["skill"].is_null()
            && job["no_agent"].as_bool() != Some(true),
        "native skill/script payload cannot be represented"
    );
    let one_shot = job["schedule"]["kind"].as_str() == Some("once");
    ensure!(
        job["repeat"]["times"].is_null()
            || (one_shot
                && job["repeat"]["times"].as_u64() == Some(1)
                && job["repeat"]["completed"].as_u64().unwrap_or(0) == 0),
        "finite repeat count or completed one-shot cannot be represented"
    );
    ensure!(
        job["fire_claim"].is_null(),
        "a native firing is still active"
    );
    let native = &job["schedule"];
    let when = match native["kind"].as_str() {
        Some("cron") => When::Cron(WhenCron {
            cron: required(native, "expr")?.into(),
            zone: zone(home)?,
        }),
        Some("interval") => When::Every(WhenEvery {
            seconds: native["minutes"]
                .as_u64()
                .context("interval minutes must be a positive integer")?
                .checked_mul(60)
                .context("interval overflow")?,
            anchor: instant(required(job, "next_run_at")?)?,
        }),
        Some("once") => When::At(WhenAt {
            at: instant(required(native, "run_at")?)?,
        }),
        _ => bail!("unsupported schedule cadence"),
    };
    let deliver = required(job, "deliver")?;
    let to = match deliver {
        "origin" => {
            let origin = &job["origin"];
            ensure!(
                origin["account_id"].is_null(),
                "native delivery account cannot be represented"
            );
            let platform = required(origin, "platform")?;
            let chat = origin["chat_id"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| origin["chat_id"].as_i64().map(|n| n.to_string()))
                .context("missing origin chat id")?;
            let mut thread = format!("{platform}:{chat}");
            if let Some(topic) = origin.get("thread_id").filter(|t| !t.is_null()) {
                ensure!(
                    platform == "telegram",
                    "native threaded delivery needs a supported routing mapping"
                );
                let topic = topic
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| topic.as_i64().map(|n| n.to_string()))
                    .context("invalid origin thread id")?;
                thread.push_str(&format!(".{topic}"));
            }
            Destination::Thread(DestinationThread { thread })
        }
        "local" => bail!("local file output is not an owner or chat destination"),
        target if target.contains(':') => Destination::Thread(DestinationThread {
            thread: target.into(),
        }),
        _ => bail!("delivery does not identify an exact chat"),
    };
    finish(job, when, required(job, "prompt")?, to)
}

#[cfg(test)]
mod tests;
