//! One-way native import, with a durable firing-owner handoff.
use crate::{
    adapter::{Adapter, Boxed},
    settings::{self, Settings},
};
use anyhow::{Context, ensure};
use link_harness_contract::asp::schedules::{Schedule, Schedules};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const PENDING: &str = "schedule-handoff.json";
const LOCK: &str = "schedule-handoff.lock";
pub const DEFINITIONS: &str = "portable.json";
/// The agent's saved record of its schedules' identities, beside its
/// settings: what its spec never says.
const RECORD: &str = "schedules.json";

/// A schedule's identity in the agent's saved record: its id, the
/// revision its runner last had, the native job it came from, and the
/// definition that revision was of, so a changed one is a newer revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    pub id: link_harness_contract::asp::schedules::ScheduleId,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<link_harness_contract::asp::schedules::Origin>,
    pub definition: link_agent_spec::Schedule,
}

/// The agent's saved record ([`RECORD`]): each schedule's identity,
/// naming its schema.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schedules: Vec<Kept>,
}

link_files::specification!(Record, include_str!("schedules/record.schema.toml"));

/// Each agent's schedule identities, which an upgrade converts.
pub(crate) fn saved_record() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Record>()
}

/// The agent's record, by its schedules' names: none before its first.
pub fn record(agent: &Settings) -> anyhow::Result<Vec<Kept>> {
    match std::fs::read(settings::config_of(&agent.name).join(RECORD)) {
        Ok(bytes) => Ok(link_files::saved::parse_json::<Record>(&bytes)
            .context("reading the schedules' record")?
            .schedules),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

/// Writes agent `name`'s record whole: as a move lands it, the identities
/// its schedules had where it came from.
pub fn put_record(name: &str, kept: &[Kept]) -> anyhow::Result<()> {
    let record = Record {
        schedules: kept.to_vec(),
    };
    Ok(link_files::write_private(
        &settings::config_of(name).join(RECORD),
        &link_files::saved::print_json(&record)?,
    )?)
}

fn keep_record(agent: &Settings, schedules: &[Schedule]) -> anyhow::Result<()> {
    let kept: Vec<Kept> = schedules
        .iter()
        .map(|schedule| Kept {
            id: schedule.id.clone(),
            revision: schedule.revision,
            origin: schedule.origin.clone(),
            definition: link_agent_spec::Schedule::of(schedule),
        })
        .collect();
    put_record(&agent.name, &kept)
}

/// The schedules a runner fires: each of the spec's with its identity
/// from the record, a newer revision where its definition changed, and
/// one the record lacks named for its spec (`spec:<name>`) at revision 1.
pub fn identified(definitions: &[link_agent_spec::Schedule], record: &[Kept]) -> Vec<Schedule> {
    use link_harness_contract::asp::schedules::ScheduleId;
    definitions
        .iter()
        .map(
            |definition| match record.iter().find(|k| k.definition.name == definition.name) {
                Some(kept) => definition.with(
                    kept.id.clone(),
                    kept.revision + u64::from(kept.definition != *definition),
                    kept.origin.clone(),
                ),
                None => definition.with(ScheduleId(format!("spec:{}", definition.name)), 1, None),
            },
        )
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Stage {
    Prepared,
    Recorded,
    NativeCleared,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handoff {
    harness: String,
    root: PathBuf,
    schedules: Schedules,
    stage: Stage,
    #[serde(default)]
    superseded: Vec<Schedule>,
}

link_files::specification!(Handoff, include_str!("schedules/handoff.schema.toml"));

/// Each schedule handoff, which an upgrade converts.
pub(crate) fn saved_handoff() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Handoff>()
}

fn pending(agent: &Settings) -> PathBuf {
    settings::config_of(&agent.name).join(PENDING)
}

/// The schedule named in a harness change's explanation.
pub fn words(schedule: &Schedule) -> String {
    use link_harness_contract::asp::schedules::{Destination, When};
    use link_harness_contract::asp::schedules::{
        DestinationOwner, DestinationThread, WhenAt, WhenCron, WhenEvery,
    };
    let when = match &schedule.when {
        When::Cron(WhenCron { cron, zone }) => format!("{cron} in {zone}"),
        When::Every(WhenEvery { seconds, anchor }) => {
            format!("every {seconds} seconds, anchored at Unix time {anchor}")
        }
        When::At(WhenAt { at }) => format!("once at Unix time {at}"),
    };
    let to = match &schedule.to {
        Destination::Owner(DestinationOwner {
            connection: Some(connection),
        }) => format!("your direct chat on {connection}"),
        Destination::Owner(DestinationOwner { connection: None }) => "your CLI inbox".into(),
        Destination::Thread(DestinationThread { thread }) => format!("chat {thread}"),
    };
    format!("{} ({when}, to {to})", schedule.name)
}

/// The immutable source id binds to the same definition on every retry.
/// Owner-edited imported definitions take precedence over a repeated export.
pub fn merge(existing: &[Schedule], incoming: &[Schedule]) -> anyhow::Result<Vec<Schedule>> {
    let mut merged = existing.to_vec();
    for schedule in incoming {
        schedule.check()?;
        if let Some(known) = merged.iter().find(|s| {
            s.id == schedule.id
                || (s.origin.is_some() && s.origin == schedule.origin)
                || s.name == schedule.name
        }) {
            ensure!(
                known.id == schedule.id && known.origin == schedule.origin,
                "schedule identity conflicts with an existing definition"
            );
            continue;
        }
        merged.push(schedule.clone());
    }
    Ok(merged)
}

/// Called with the native runner stopped. An existing handoff is always
/// reconciled first, even when its native file was already cleared.
pub fn import(
    agent: &Settings,
    harness: &str,
    root: &Path,
    schedules: &Schedules,
) -> anyhow::Result<()> {
    let _lock = link_files::lock_file(&settings::config_of(&agent.name).join(LOCK))?;
    recover_locked(agent)?;
    if schedules.source_revision.is_none()
        || (schedules.carried.is_empty() && schedules.not_carried.is_empty())
    {
        return Ok(());
    }
    for definition in &schedules.carried {
        link_scheduler_tool_contract::timing::check(definition)?;
        ensure!(
            definition
                .origin
                .as_ref()
                .is_some_and(|origin| origin.adapter == harness),
            "native definition does not belong to the exporting adapter"
        );
    }
    let handoff = Handoff {
        harness: harness.into(),
        root: root.into(),
        schedules: schedules.clone(),
        stage: Stage::Prepared,
        superseded: Vec::new(),
    };
    save(agent, &handoff)?;
    recover_locked(agent)
}

fn save(agent: &Settings, handoff: &Handoff) -> anyhow::Result<()> {
    Ok(link_files::write_private(
        &pending(agent),
        &link_files::saved::print_json(handoff)?,
    )?)
}

pub fn recover(agent: &Settings) -> anyhow::Result<()> {
    let _lock = link_files::lock_file(&settings::config_of(&agent.name).join(LOCK))?;
    recover_locked(agent)
}

fn recover_locked(agent: &Settings) -> anyhow::Result<()> {
    let mut handoff: Handoff = match std::fs::read(pending(agent)) {
        Ok(bytes) => {
            link_files::saved::parse_json(&bytes).context("reading pending schedule handoff")?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    let advanced = advance(
        &mut handoff,
        |handoff| record_definitions(agent, handoff),
        |handoff| {
            let adapter = Adapter::find(&handoff.harness)?;
            let boxed = Boxed::new(agent, &handoff.harness)?;
            adapter.clear_schedules(
                &boxed,
                &handoff.root,
                handoff
                    .schedules
                    .source_revision
                    .as_deref()
                    .context("handoff has no native revision")?,
                handoff.schedules.source_context.as_deref(),
            )
        },
        |handoff| save(agent, handoff),
    );
    if let Err(error) = advanced {
        if handoff.stage == Stage::Recorded {
            let adapter = Adapter::find(&handoff.harness)?;
            let fresh = adapter.native_schedules(&handoff.root)?;
            if (fresh.source_revision != handoff.schedules.source_revision
                || fresh.source_context != handoff.schedules.source_context)
                && (!fresh.carried.is_empty() || !fresh.not_carried.is_empty())
            {
                for definition in &fresh.carried {
                    link_scheduler_tool_contract::timing::check(definition)?;
                }
                let names = fresh
                    .carried
                    .iter()
                    .map(words)
                    .chain(
                        fresh
                            .not_carried
                            .iter()
                            .map(|s| format!("{} is not carried: {}", s.name, s.reason)),
                    )
                    .collect::<Vec<_>>()
                    .join(", ");
                handoff.superseded = handoff.schedules.carried;
                handoff.schedules = fresh;
                handoff.stage = Stage::Prepared;
                save(agent, &handoff)?;
                return Err(error.context(format!("Native schedules changed. The pending import now names {names}. Retry to continue; managed Link firing has not been activated by this attempt")));
            }
        }
        return Err(error);
    }
    // Only after the copied native source is durably clear may Link fire.
    publish(agent)?;
    let revision = handoff
        .schedules
        .source_revision
        .as_deref()
        .context("handoff has no native revision")?;
    let report = settings::config_of(&agent.name)
        .join("schedule-imports")
        .join(format!("{revision}.json"));
    link_files::write_private(&report, &link_files::saved::print_json(&handoff)?)?;
    std::fs::remove_file(pending(agent))?;
    Ok(())
}

fn record_definitions(agent: &Settings, handoff: &Handoff) -> anyhow::Result<()> {
    let path = Settings::path_of(&agent.name);
    let before = std::fs::read_to_string(&path)?;
    let behavior = settings::configuration::behavior_text(&before)?.unwrap_or_default();
    let own = link_agent_spec::parse(&behavior)?;
    let existing: Vec<_> = identified(&own.schedules.unwrap_or_default(), &record(agent)?)
        .into_iter()
        .filter(|definition| !handoff.superseded.iter().any(|s| s.id == definition.id))
        .collect();
    let definitions = merge(&existing, &handoff.schedules.carried)?;
    let written: Vec<_> = definitions
        .iter()
        .map(link_agent_spec::Schedule::of)
        .collect();
    let changed = link_agent_spec::with_schedules(&behavior, &written)?;
    let written = settings::configuration::with_behavior(&before, &changed)?;
    ensure!(
        std::fs::read_to_string(&path)? == before,
        "agent settings changed during schedule import; retry"
    );
    // The identities first: a spec schedule the record lacks is the
    // spec's own, so an import stopped between them keeps its native ones.
    keep_record(agent, &definitions)?;
    link_files::write_private(&path, written.as_bytes())?;
    Ok(())
}

/// Persist a stage only after its effect succeeds. Both effects are
/// idempotent, including interruption between the effect and this save.
fn advance(
    handoff: &mut Handoff,
    record: impl FnOnce(&Handoff) -> anyhow::Result<()>,
    clear: impl FnOnce(&Handoff) -> anyhow::Result<()>,
    mut persist: impl FnMut(&Handoff) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if handoff.stage == Stage::Prepared {
        record(handoff)?;
        handoff.stage = Stage::Recorded;
        persist(handoff)?;
    }
    if handoff.stage == Stage::Recorded {
        clear(handoff)?;
        handoff.stage = Stage::NativeCleared;
        persist(handoff)?;
    }
    Ok(())
}

/// The scheduler's runner consumes this projection; it contains the same
/// canonical values as the spec. The tool cannot activate native sources.
pub fn publish(agent: &Settings) -> anyhow::Result<PathBuf> {
    match std::fs::read(pending(agent)) {
        Ok(bytes) => {
            let handoff: Handoff = link_files::saved::parse_json(&bytes)?;
            ensure!(
                handoff.stage == Stage::NativeCleared,
                "native schedule handoff is not cleared yet"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("reading schedule handoff before publication"),
    }
    let definitions = identified(
        &agent.spec()?.spec.schedules.unwrap_or_default(),
        &record(agent)?,
    );
    keep_record(agent, &definitions)?;
    let state = agent
        .folder()
        .of(link_store_contract::storage::Collection::Schedule);
    let path = state.join(DEFINITIONS);
    let definitions = link_scheduler_tool_contract::definitions::Definitions {
        schedules: definitions,
    };
    link_files::write_private(&path, &link_files::saved::print_json(&definitions)?)?;
    Ok(path)
}

#[cfg(test)]
mod tests;
