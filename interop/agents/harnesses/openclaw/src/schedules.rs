use anyhow::{Context, bail, ensure};
use link_harness_contract::asp::schedules::{
    Destination, NotCarried, Origin, Schedule, ScheduleId, Schedules, When,
};
use link_harness_contract::asp::schedules::{
    DestinationOwner, DestinationThread, WhenAt, WhenCron, WhenEvery,
};
use link_harness_contract::harness::schedules as source;
use serde_json::Value;
use std::path::Path;

const ADAPTER: &str = "openclaw";
pub(crate) const JOBS: &str = "cron/jobs.json";
const ARCHIVE: &str = ".lnk/schedule-imports";
pub(crate) const EMPTY: &[u8] = br#"{"version":1,"jobs":[]}"#;

pub fn clear_root(home: &Path, revision: &str) -> anyhow::Result<()> {
    source::clear(&home.join(JOBS), &home.join(ARCHIVE), revision, EMPTY)
}

pub fn export(home: &Path) -> anyhow::Result<Schedules> {
    export_root(&home.join(".openclaw"))
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
    ensure!(
        raw["version"].as_u64() == Some(1),
        "unsupported OpenClaw cron store version"
    );
    raw["jobs"]
        .as_array()
        .context("OpenClaw jobs must be an array")
}

fn milliseconds(value: &Value) -> anyhow::Result<u64> {
    let ms = value
        .as_u64()
        .context("missing millisecond time or interval")?;
    ensure!(
        ms.is_multiple_of(1000),
        "subsecond timing cannot be represented"
    );
    Ok(ms / 1000)
}

fn convert(_home: &Path, job: &Value) -> anyhow::Result<Schedule> {
    for key in [
        "pacing",
        "failureAlert",
        "scheduledToolPolicy",
        "sessionKey",
    ] {
        ensure!(job[key].is_null(), "{key} has native execution semantics");
    }
    ensure!(
        job["agentId"].as_str().is_none_or(|agent| agent == "main"),
        "job belongs to another native agent"
    );
    let payload = &job["payload"];
    let message = match payload["kind"].as_str() {
        Some("agentTurn") => {
            ensure!(
                payload
                    .as_object()
                    .is_some_and(|p| p.keys().all(|k| ["kind", "message"].contains(&k.as_str()))),
                "agent turn has native execution options"
            );
            required(payload, "message")?
        }
        Some("systemEvent") => {
            ensure!(
                payload
                    .as_object()
                    .is_some_and(|p| p.keys().all(|key| ["kind", "text"].contains(&key.as_str()))),
                "system event has native execution options"
            );
            required(payload, "text")?
        }
        _ => bail!("payload is not a portable message"),
    };
    ensure!(
        job["wakeMode"].as_str() == Some("now"),
        "heartbeat wake policy cannot be represented"
    );
    ensure!(
        !job["state"]["runningAtMs"].is_number(),
        "a native firing is still active"
    );
    let native = &job["schedule"];
    ensure!(
        job["deleteAfterRun"].as_bool() != Some(true) || native["kind"].as_str() == Some("at"),
        "delete-after-run changes a recurring definition"
    );
    ensure!(
        job["sessionTarget"]
            .as_str()
            .is_none_or(|target| ["main", "isolated"].contains(&target)),
        "native session binding cannot be represented"
    );
    let when = match native["kind"].as_str() {
        Some("cron") => {
            ensure!(
                native["staggerMs"].as_u64().unwrap_or(0) == 0,
                "staggered cron is not exact cron"
            );
            When::Cron(WhenCron {
                cron: required(native, "expr")?.into(),
                zone: required(native, "tz")?.into(),
            })
        }
        Some("every") => When::Every(WhenEvery {
            seconds: milliseconds(&native["everyMs"])?,
            anchor: milliseconds(native.get("anchorMs").unwrap_or(&job["createdAtMs"]))?,
        }),
        Some("at") => When::At(WhenAt {
            at: instant(required(native, "at")?)?,
        }),
        _ => bail!("unsupported schedule cadence"),
    };
    let delivery = &job["delivery"];
    ensure!(
        delivery["accountId"].is_null()
            && delivery["completionDestination"].is_null()
            && delivery["failureDestination"].is_null(),
        "delivery needs an unsupported account or secondary destination"
    );
    ensure!(
        delivery["mode"].as_str() == Some("announce"),
        "delivery is not an explicit chat destination"
    );
    let channel = required(delivery, "channel")?;
    ensure!(channel != "last", "last-used channel is ambiguous");
    let Some(chat) = delivery["to"].as_str().filter(|s| !s.is_empty()) else {
        ensure!(
            delivery["threadId"].is_null(),
            "thread delivery needs its chat"
        );
        return finish(
            job,
            when,
            message,
            Destination::Owner(DestinationOwner {
                connection: Some(channel.into()),
            }),
        );
    };
    let mut thread = format!("{channel}:{chat}");
    if let Some(topic) = delivery.get("threadId").filter(|t| !t.is_null()) {
        let topic = topic
            .as_str()
            .map(str::to_owned)
            .or_else(|| topic.as_u64().map(|n| n.to_string()))
            .context("invalid thread id")?;
        ensure!(
            channel == "telegram",
            "native threaded delivery needs a supported routing mapping"
        );
        thread.push_str(&format!(".{topic}"));
    }
    finish(
        job,
        when,
        message,
        Destination::Thread(DestinationThread { thread }),
    )
}

#[cfg(test)]
mod tests;
