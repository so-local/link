//! The scheduler as a tool: an MCP server over stdio offering `task`,
//! `schedule`, `message` (to a retained child), `list`, `read`,
//! `cancel`, `schedules` and `unschedule`, and `send` for an agent on a
//! team, each as its declarations say (`actions.toml`). Each call writes the store; the runner fires
//! what falls due. On a
//! machine that runs the agent only while it answers, each call that
//! changes the jobs tells the machine when they're next due
//! ([`crate::Address`]).
//!
//! A call says which conversation it's made in, and how many messages led
//! to it, in its `_meta` (`link.local/thread`, `link.local/hops`), as
//! Link Harness sends them: never in its arguments, which the model
//! writes. A job made from it is one hop further, refused past the cap.
//! A harness that keeps a conversation's tools says them too
//! (`link.local/tools`): a subagent gets those, or fewer, never more.
//! The owner's command line alone says `link.local/all` on a `list`, for
//! every conversation's jobs (`lnk agent task list --all`).
//! It says the standing of the message the conversation is answering
//! (`link.local/standing`), which every job keeps, so a schedule or a
//! subagent never fires as more than the conversation that made it.
//! A schedule call that names no conversation is made in the agent's
//! main chat, `link:main`.

use std::io::{BufRead, Write};

use anyhow::{Context, bail, ensure};
use link_connection_contract::AnswerTo;
use serde_json::{Value, json};

use crate::constants::{COMMAND_LINE, HERE, LIST_CALL, NOWHERE, SCHEDULES_CALL};
use crate::limits::limits;
use crate::{
    Address, Caps, Job, Kind, Peers, Store, Teams, cron::Cron, local_offset, now, offset_text,
};

mod children;
mod compacting;
mod making;
mod offered;
mod sending;
mod timing;
use making::{message, schedule, task};
use sending::send;
use timing::link_time;

/// The protocol versions it speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// The tool: the store it writes, the caps it keeps to, the agent's
/// teams, and on the address driver, where it tells the machine when the
/// jobs are next due.
pub struct Tool {
    pub store: Store,
    pub caps: Caps,
    pub teams: Teams,
    pub address: Option<Address>,
    /// The agent's floor (`--floor`): the least standing anyone reaches it
    /// with, which a call that says none keeps, and above which a call's
    /// own never goes.
    pub floor: link_connection_contract::access::Standing,
    /// The connections the agent is connected to (`--connection`): where
    /// a schedule's answer may go, besides here, nowhere and the command
    /// line.
    pub connections: Vec<String>,
}

impl Tool {
    /// Where a schedule's answer may go, by the names `answer` takes:
    /// here, nowhere, the command line, then each connection.
    pub(crate) fn answers(&self) -> Vec<String> {
        let mut answers: Vec<String> = [HERE, NOWHERE, COMMAND_LINE].map(str::to_string).into();
        for connection in &self.connections {
            if !answers.contains(connection) {
                answers.push(connection.clone());
            }
        }
        answers
    }
}

/// Answers the messages on `input` on `output`, until `input` ends.
pub fn serve(tool: &Tool, input: impl BufRead, mut output: impl Write) -> anyhow::Result<()> {
    for line in input.lines() {
        let line = line.context("reading a message")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = answer(tool, &line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The reply to message `line`: none for a notification.
pub fn answer(tool: &Tool, line: &str) -> Option<Value> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "not JSON".into()));
    };
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = VERSIONS
                .iter()
                .find(|v| Some(**v) == asked)
                .unwrap_or(&VERSIONS[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "link-scheduler", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!(
                        "{} Your time zone is {}: a schedule's cron fires on that clock.",
                        match tool.teams.teammates.is_empty() {
                            true => "Start subagents for work that can run beside you, and \
                                schedule messages to yourself for later. A subagent's answer, \
                                and a schedule's, come to you as messages from them, never from \
                                your owner.",
                            false => "Start subagents for work that can run beside you, schedule \
                                messages to yourself for later, and send your teammates \
                                messages. A subagent's answer, a schedule's and a teammate's \
                                come to you as messages from them, never from your owner.",
                        },
                        crate::zone_said(now())
                    ),
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => match offered::offered(tool) {
            Ok(tools) => result(id, json!({"tools": tools})),
            Err(why) => error(id, -32603, format!("{why:#}")),
        },
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let meta = params.get("_meta").cloned().unwrap_or(Value::Null);
            let declared = link_scheduler_tool_contract::calls()
                .is_ok_and(|calls| calls.iter().any(|call| call == name));
            if !declared || (name == "send" && tool.teams.teammates.is_empty()) {
                return Some(error(id, -32602, format!("no tool {name:?}")));
            }
            // Its arguments, refused by their declaration before its handler.
            if let Err(refused) = crate::cli::check_call(name, &arguments) {
                return Some(result(
                    id,
                    json!({"content": [{"type": "text", "text": refused.to_string()}], "isError": true}),
                ));
            }
            let called = call(tool, name, &arguments, &meta).map(|mut value| {
                // Kept, but the machine not told: a success that says so,
                // never an error, which would invite making it again.
                let reads = link_scheduler_tool_contract::reads()
                    .is_ok_and(|reads| reads.iter().any(|read| read == name));
                if let (Some(address), false) = (&tool.address, reads)
                    && let Err(err) = address.post(&tool.store)
                    && let Some(said) = value.as_object_mut()
                {
                    said.insert(
                        "warning".into(),
                        json!(format!(
                            "Done, but the machine wasn't told when the jobs are next due, so \
                             it may not wake for them: {err:#}. Don't make it again."
                        )),
                    );
                }
                value
            });
            match called {
                Ok(value) => result(
                    id,
                    json!({"content": [{"type": "text", "text": value.to_string()}],
                        "structuredContent": value}),
                ),
                Err(err) => result(
                    id,
                    json!({"content": [{"type": "text", "text": format!("{err:#}")}],
                        "isError": true}),
                ),
            }
        }
        other => error(id, -32601, format!("no method {other:?}")),
    })
}

/// Where a call was made: its conversation, and how many messages led
/// to it.
struct Made {
    meta: Value,
    thread: Option<String>,
    hops: u32,
    /// The conversation's tools, from a harness that keeps them: none
    /// from one that doesn't.
    tools: Option<Vec<String>>,
    /// The model the conversation runs on, from a harness that says.
    model: Option<String>,
    /// The standing a job made from it keeps: its harness's claim
    /// (`link.local/standing`), at most the agent's floor, or the floor
    /// when it says none. None: a claim that isn't a standing.
    standing: Option<link_connection_contract::access::Standing>,
}

fn made(meta: &Value, floor: link_connection_contract::access::Standing) -> Made {
    Made {
        standing: match meta.get("link.local/standing") {
            None | Some(Value::Null) => Some(floor),
            Some(claim) => {
                serde_json::from_value::<link_connection_contract::access::Standing>(claim.clone())
                    .ok()
                    .map(|claim| claim.at_most(floor))
            }
        },
        meta: meta.clone(),
        thread: meta["link.local/thread"]
            .as_str()
            .filter(|t| plain_thread(t))
            .map(str::to_string),
        hops: meta["link.local/hops"]
            .as_u64()
            .map_or(0, |h| h.min(u32::MAX as u64) as u32),
        // Said, but not as a list of tools' names: none of them, never
        // all.
        tools: meta.get("link.local/tools").map(|tools| {
            tools
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|t| link_connection_contract::is_tool_name(t))
                .take(link_connection_contract::TOOLS_MAX)
                .map(str::to_string)
                .collect()
        }),
        model: meta["link.local/model"]
            .as_str()
            .filter(|m| link_connection_contract::is_model_name(m))
            .map(str::to_string),
    }
}

/// A conversation's name as Link Harness takes one: letters, digits,
/// `-`, `_`, `.`, one `:`, at most 128.
fn plain_thread(thread: &str) -> bool {
    (1..=128).contains(&thread.len())
        && thread.starts_with(|c: char| c.is_ascii_alphanumeric())
        && thread.matches(':').count() <= 1
        && thread
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
}

fn call(tool: &Tool, name: &str, arguments: &Value, meta: &Value) -> anyhow::Result<Value> {
    let mut made = made(meta, tool.floor);
    // A schedule call that names no conversation, from a harness that
    // can't or the owner's terminal, is made in the agent's main chat:
    // its schedules fire there, and are listed and deleted there.
    if made.thread.is_none() && matches!(name, "schedule" | "schedules" | "unschedule") {
        made.thread = Some(link_connection_contract::link_thread(
            link_connection_contract::LINK_MAIN,
        )?);
    }
    let text = |key: &str| arguments.get(key).and_then(Value::as_str);
    if name == link_scheduler_tool_contract::compaction::compaction().call {
        return compacting::keep(tool, &made, arguments, now());
    }
    match name {
        "read" => {
            let id = text("id").context("read needs a job ID")?;
            let job = mine(&tool.store, &made)?
                .into_iter()
                .find(|job| job.id == id)
                .context("no job of this conversation has that ID")?;
            Ok(serde_json::to_value(job)?)
        }
        "list" | "schedules" => {
            // The owner's view from their command line (`lnk agent task
            // list --all`): every conversation's, which no model's call says.
            let mut mine = match meta["link.local/all"] == true {
                true => every(&tool.store),
                false => mine(&tool.store, &made)?,
            };
            // `schedules` is the `cron` namespace's view: schedules alone.
            let schedules = name == "schedules";
            mine.retain(|job| !schedules || job.kind == Kind::Schedule);
            mine.sort_by_key(|job| job.due);
            let jobs: Vec<Value> = mine
                .iter()
                .take(limits().list_max)
                .map(|job| {
                    let mut listed = json!({"id": job.id, "kind": job.kind,
                        "thread": job.thread,
                        "text": job.text.chars().take(limits().listed_text_max).collect::<String>(),
                        "next": link_time(job.due), "cron": job.cron});
                    if !job.after.is_empty() {
                        listed["after"] = json!(job.after);
                    }
                    if let Some(description) = &job.description {
                        listed["description"] = json!(description);
                    }
                    // How it ended; its answer is 'read''s.
                    if let Some(outcome) = &job.outcome {
                        listed["ended"] = match &outcome.how {
                            crate::How::Done { .. } => json!("done"),
                            crate::How::Failed { why } => json!({"failed": why}),
                            crate::How::Cancelled { why } => json!({"cancelled": why}),
                        };
                    }
                    listed
                })
                .collect();
            let mut listed = json!({"jobs": jobs});
            if let (false, Some(store), Some(token)) = (
                schedules,
                &tool.store.interactions,
                meta["link.local/work"].as_str(),
            ) {
                listed["children"] = serde_json::to_value(store.retained_children(token)?)?;
            }
            if mine.len() > limits().list_max {
                // How many more there are, not shown.
                listed["cut"] = json!(mine.len() - limits().list_max);
            }
            Ok(listed)
        }
        "cancel" | "unschedule" => {
            // `unschedule` is the `cron` namespace's: schedules alone.
            let (list, only) = match name {
                "unschedule" => (SCHEDULES_CALL, Some(Kind::Schedule)),
                _ => (LIST_CALL, None),
            };
            let id = text("id")
                .with_context(|| format!("cancelling needs the job's 'id', as {list} gives it"))?;
            let no_job = || format!("no job {id} of this conversation's: {list} lists them");
            ensure!(
                mine(&tool.store, &made)?
                    .iter()
                    .any(|job| job.id == id && only.is_none_or(|kind| job.kind == kind)),
                no_job()
            );
            // `unschedule` is a delete: the schedule is gone from every
            // list; `cancel` keeps a job ended, as `list` shows what ended.
            match only {
                Some(_) => match tool.store.delete(id)? {
                    true => Ok(json!({"deleted": id})),
                    false => bail!(no_job()),
                },
                None => match tool.store.cancel(id)? {
                    true => Ok(json!({"cancelled": id})),
                    false => bail!(no_job()),
                },
            }
        }
        "task" | "schedule" | "send" | "message" => {
            let hops = made.hops.saturating_add(1);
            ensure!(
                hops <= tool.caps.hops,
                "Not sent: {hops} messages in a chain, past the cap of {} ([tool.scheduler] \
                 hops). Answer from what you have.",
                tool.caps.hops
            );
            // A subagent's work is outside the day's budget ([`Kind::counts`]).
            ensure!(
                !matches!(name, "schedule" | "send")
                    || tool.store.sent_on(now()) < tool.caps.per_day,
                "Not sent: {} messages today, the cap ([tool.scheduler] per_day).",
                tool.caps.per_day
            );
            ensure!(
                tool.store.waiting() < limits().jobs_max,
                "Not sent: {} jobs waiting.",
                limits().jobs_max
            );
            // A subagent cancelled, or failed, starts nothing more.
            if let Some(coordinator) = making::coordinator(&made) {
                ensure!(
                    tool.store.at_work(&coordinator),
                    "Not sent: this subagent was cancelled or ended. Answer from what you have."
                );
            }
            if name == "send" {
                return send(tool, &made, arguments, hops);
            }
            let id = link_id::candidate()?;
            let job = match name {
                "task" => task(tool, &made, arguments, &id, hops)?,
                "message" => children::message(tool, &made, arguments, &id, hops)?,
                _ => schedule(tool, &made, arguments, &id, hops)?,
            };
            let disposition = crate::interactions::prepare(&tool.store, &job, meta)?;
            tool.store.put(&job)?;
            Ok(json!({"id": job.id, "kind": job.kind, "thread": job.thread,
                "next": link_time(job.due), "cron": job.cron, "delegation": disposition}))
        }
        _ => bail!("no tool {name:?}"),
    }
}

/// Every job of the model's, of every conversation: waiting, being fired
/// or ended and kept.
fn every(store: &Store) -> Vec<Job> {
    store
        .jobs()
        .into_iter()
        .chain(store.firing())
        .chain(store.ended_jobs())
        .filter(|job| job.kind.is_models())
        .collect()
}

/// The jobs of the conversation a call was made in: those sent to it,
/// the subagents it started, and theirs, waiting, being fired or ended
/// and kept. A conversation never sees, nor cancels, another's.
fn mine(store: &Store, made: &Made) -> anyhow::Result<Vec<Job>> {
    let thread = made
        .thread
        .clone()
        .context("this needs the conversation it's called in")?;
    let all: Vec<Job> = store
        .jobs()
        .into_iter()
        .chain(store.firing())
        .chain(store.ended_jobs())
        .collect();
    let mut threads = vec![thread];
    loop {
        let started: Vec<String> = all
            .iter()
            .filter(|job| {
                job.kind == Kind::Task
                    && matches!(&job.answer, AnswerTo::Thread(parent) if threads.contains(parent))
                    && !threads.contains(&job.thread)
            })
            .map(|job| job.thread.clone())
            .collect();
        if started.is_empty() {
            break;
        }
        threads.extend(started);
    }
    // A message to a teammate is this conversation's when its answer
    // comes back here.
    Ok(all
        .into_iter()
        .filter(|job| job.kind.is_models())
        .filter(|job| match (job.kind, &job.answer) {
            (Kind::Send, AnswerTo::Thread(back)) => threads.contains(back),
            _ => threads.contains(&job.thread),
        })
        .collect())
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests;
