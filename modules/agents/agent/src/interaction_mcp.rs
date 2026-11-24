//! Authenticated question and retained-proposal reads never expose owner response codes or bindings.
use crate::settings::Settings;
use anyhow::{Context, ensure};
use link_interactions::*;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn offered() -> Vec<Value> {
    vec![
        json!({"name":"question_read","description":"Read a question created by this exact work. Preserve supplied option wording and user qualifications.",
            "inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}}),
        json!({"name":"question_cancel","description":"Cancel a question created by this exact work; this never supplies an owner answer.",
            "inputSchema":{"type":"object","properties":{"id":{"type":"string"},"reason":{"type":"string"}},"required":["id","reason"],"additionalProperties":false}}),
        json!({"name":"task_held","description":"List this work's retained delegation proposals. Read each exact job with task.read, review it against the latest plan, then revalidate or abandon it. Owner holds can be released only by the owner.",
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"task_revalidate","description":"After reviewing the exact retained job and the latest plan, keep or abandon it. This cannot release an owner hold or authorize an operation.",
            "inputSchema":{"type":"object","properties":{"id":{"type":"string"},"keep":{"type":"boolean"}},"required":["id","keep"],"additionalProperties":false}}),
    ]
}

pub fn execute(agent: &Settings, action: &str, args: Value, meta: &Value) -> anyhow::Result<Value> {
    execute_at(&Store::machine()?, &agent.key(), action, args, meta)
}

fn execute_at(
    store: &Store,
    agent: &str,
    action: &str,
    args: Value,
    meta: &Value,
) -> anyhow::Result<Value> {
    let token = meta["link.local/work"]
        .as_str()
        .context("this harness has no authenticated work binding")?;
    let work = store.authenticate_work(
        token,
        agent,
        meta["link.local/thread"].as_str().unwrap_or_default(),
        meta["link.local/attempt"].as_str().unwrap_or_default(),
    )?;
    ensure!(
        meta["link.local/control_revision"].as_u64() == Some(work.control_revision),
        "work changed before this interaction action"
    );
    if action == "task_held" {
        ensure!(
            args.as_object().is_some_and(|a| a.is_empty()),
            "held takes no arguments"
        );
        return Ok(serde_json::to_value(store.delegation_proposals(token)?)?);
    }
    if action == "task_revalidate" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Revalidate {
            id: String,
            keep: bool,
        }
        let request: Revalidate = serde_json::from_value(args)?;
        return Ok(serde_json::to_value(store.revalidate_delegation(
            token,
            work.control_revision,
            &request.id,
            request.keep,
        )?)?);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Question {
        id: RequestId,
        reason: Option<String>,
    }
    let request: Question = serde_json::from_value(args)?;
    let snapshot = store.snapshot(&request.id, now())?;
    ensure!(
        snapshot
            .question
            .requester
            .work
            .as_ref()
            .is_some_and(|w| w.agent == work.agent
                && w.session == work.session
                && w.attempt == work.attempt
                && w.generation == work.generation),
        "question belongs to another continuation"
    );
    match action {
        "question_read" => {
            ensure!(request.reason.is_none(), "read takes no reason");
            Ok(serde_json::to_value(snapshot)?)
        }
        "question_cancel" => {
            let reason = request.reason.context("cancellation needs a reason")?;
            ensure!(
                !reason.trim().is_empty() && reason.len() <= 1024,
                "invalid cancellation reason"
            );
            store.cancel(&request.id, &snapshot.question.requester, &reason, now())?;
            Ok(json!({"cancelled":request.id}))
        }
        _ => anyhow::bail!("unknown interaction action"),
    }
}

#[cfg(test)]
mod tests;
