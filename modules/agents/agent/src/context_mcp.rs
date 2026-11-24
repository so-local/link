//! The calling agent's public context and messaging; no credentials in answers.
use anyhow::ensure;
use link_agent::{
    identity,
    settings::{self, Settings},
};
use link_agent_contract::namespace as forms;
use link_format::WithinExt;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub(crate) fn shown(agent: &Settings, instructions: bool) -> anyhow::Result<forms::Shown> {
    let spec = agent.spec()?;
    let model = agent.model.as_ref();
    let opinions = spec.model_for(model.map_or("", |model| model.provider.as_str()));
    Ok(forms::Shown {
        name: agent.name.clone(),
        harness: agent.harness().into(),
        model: model.and_then(|model| Settings::model_name(model, &spec)),
        window: spec.spec.window.as_ref().and_then(|w| w.max),
        answer: opinions.answer,
        instructions: if instructions {
            Some(identity::of(&spec.spec, &agent.folder().agent)?)
        } else {
            None
        },
    })
}
pub(crate) fn context(agent: &Settings, action: &str, args: Value) -> anyhow::Result<Value> {
    let args = args.as_object().within("arguments must be an object")?;
    if action == "show" {
        ensure!(
            args.keys().all(|k| k == "instructions"),
            "unknown show argument"
        );
        let instructions = args
            .get("instructions")
            .map(|v| v.as_bool().within("instructions must be boolean"))
            .transpose()?
            .unwrap_or(false);
        return Ok(serde_json::to_value(shown(agent, instructions)?)?);
    }
    ensure!(args.is_empty(), "this read takes no arguments");
    Ok(match action {
        "memory_show" => serde_json::to_value(forms::Remembered {
            notes: identity::remembered(&agent.folder().agent)?
                .into_iter()
                .filter(|note| !note.deleted)
                .map(|note| forms::Note {
                    source: note.source.unwrap_or_else(|| "link".into()),
                    text: note.text,
                })
                .collect(),
        })?,
        "tool_list" => {
            let mut namespaces = Vec::new();
            for name in crate::namespaces::NAMES
                .iter()
                .chain(crate::namespaces::DIRECT)
            {
                let file = settings::config_of(&agent.name).join(format!("namespace-{name}.json"));
                if let Ok(bytes) = std::fs::read(file) {
                    let definition: link_sandbox_client::namespace::Namespace =
                        link_files::saved::parse_json(&bytes)?;
                    namespaces.push(forms::Namespace {
                        name: name.to_string(),
                        actions: definition.actions.keys().cloned().collect(),
                    });
                }
            }
            if agent.memory_tool().is_some() {
                let actions = link_agent_contract::actions()
                    .into_iter()
                    .filter_map(|(path, action)| {
                        path.strip_prefix("memory ")
                            .filter(|_| {
                                action.on(link_agent_contract::action_registry::Surface::Tool)
                            })
                            .map(str::to_owned)
                    })
                    .collect();
                namespaces.push(forms::Namespace {
                    name: "memory".into(),
                    actions,
                });
            }
            // Link MCP by the reads its grant opens; its other calls ask.
            if let Some((name, _)) = crate::mcp_tool::handed(agent) {
                namespaces.push(forms::Namespace {
                    name,
                    actions: crate::mcp_tool::reads(agent)?,
                });
            }
            for name in agent.tools_of(agent.harness()) {
                if !namespaces.iter().any(|namespace| namespace.name == name) {
                    namespaces.push(forms::Namespace {
                        name,
                        actions: Vec::new(),
                    });
                }
            }
            serde_json::to_value(forms::Tools { namespaces })?
        }
        _ => anyhow::bail!("unknown context action"),
    })
}
fn message(args: Value, meta: &Value) -> anyhow::Result<forms::Send> {
    let mut args = args
        .as_object()
        .within("arguments must be an object")?
        .clone();
    ensure!(
        !args.contains_key("thread"),
        "thread belongs in call metadata"
    );
    let thread = meta["link.local/thread"]
        .as_str()
        .within("a conversation is required")?;
    ensure!(
        !thread.is_empty() && thread.len() <= 512 && !thread.chars().any(char::is_control),
        "invalid conversation"
    );
    args.insert("thread".into(), json!(thread));
    let request: forms::Send = serde_json::from_value(Value::Object(args))?;
    ensure!(!request.to.trim().is_empty(), "a recipient is required");
    ensure!(
        !request.message.trim().is_empty() && request.message.len() <= 16 << 10,
        "message must be between 1 and 16384 bytes"
    );
    Ok(request)
}

/// A refusal whose reason the model is told: about its own input, never a
/// secret.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// A teammate send's failure as the model is told it: the scheduler's own
/// reason when it refused with one, any other failure as it was.
fn peer_refusal(err: anyhow::Error) -> anyhow::Error {
    match err.downcast::<crate::action_routes::Refused>() {
        Ok(refused) if !refused.reason.is_empty() => Refused(refused.reason).into(),
        Ok(refused) => refused.into(),
        Err(err) => err,
    }
}

/// Where the owner can be messaged now: their terminal and each connection
/// the agent's bridge carries.
fn reachable(agent: &Settings) -> Vec<String> {
    let carried = crate::bridge::Plan::load(&agent.name)
        .map(|plan| plan.connections)
        .unwrap_or_default();
    owner_places(carried)
}

/// The owner's terminal first, then each of `carried` but Link's own.
fn owner_places(carried: Vec<String>) -> Vec<String> {
    std::iter::once(link_agent::chat::TERMINAL.to_string())
        .chain(
            carried
                .into_iter()
                .filter(|connection| connection != link_agent::chat::CONNECTION),
        )
        .collect()
}

/// What the `send` tool says of `connection`: where the owner can be
/// messaged as it's listed. A connection connected later is taken too, as
/// each send checks [`reachable`] again.
fn places(agent: &Settings) -> String {
    format!(
        "For to=owner: where to message them, one of {}. cli is their terminal on this computer (lnk agent chat), where it waits until they open it. Omit to answer here.",
        reachable(agent).join(", ")
    )
}

async fn send(agent: &Settings, args: Value, meta: Value) -> anyhow::Result<Value> {
    let request = message(args.clone(), &meta)?;
    if request.to == "owner" {
        // Load keys only for this mutation, never for context reads.
        let current = Settings::load(&agent.name)?;
        ensure!(current.key() == agent.key(), "agent identity changed");
        let id = link_id::candidate()?;
        let held = match &request.connection {
            Some(connection) => {
                let places = reachable(&current);
                if !places.contains(connection) {
                    return Err(Refused(format!(
                        "{connection} is not connected: the owner connects it with lnk agent connect {connection}. Send to one of: {}",
                        places.join(", ")
                    ))
                    .into());
                }
                crate::bridge::send_connection(&current, connection, &request.message)?;
                connection == link_agent::chat::TERMINAL
            }
            None => {
                crate::bridge::send_owner(&current, &request.thread, &request.message)?;
                link_agent::chat::is_link(&request.thread)
            }
        };
        if let Some(from) = meta["link.local/from"]
            .as_str()
            .filter(|from| from.starts_with("schedule:"))
        {
            let connection = request
                .connection
                .as_deref()
                .unwrap_or_else(|| request.thread.split(':').next().unwrap_or_default());
            crate::bridge::sent_in_schedule(&current, &request.thread, from, connection)?;
        }
        return Ok(serde_json::to_value(forms::Sent {
            sent: vec![forms::Recipient {
                to: "owner".into(),
                id,
                delegation: None,
                held,
            }],
        })?);
    }
    ensure!(
        request.connection.is_none(),
        "connection selects where to message the owner, not a teammate"
    );
    // A teammate is reached through its scheduler, whose caps stop two
    // that keep answering each other: with none, say so and how to add it.
    let peer = crate::namespaces::file(agent, "agent-peer");
    if !peer.is_file() {
        return Err(Refused(format!(
            "Not sent: you message teammates through your scheduler, which is off. Your owner turns it on with lnk agent tool add scheduler --agent {}, and it's on for an agent put on a team.",
            agent.name
        ))
        .into());
    }
    // The scheduler's refusal (past the cap, not a teammate) is about this
    // send: the model is told its words, so it can say why it stopped.
    let result = crate::action_routes::invoke(&peer, "send", args, meta)
        .await
        .map_err(peer_refusal)?;
    Ok(serde_json::to_value(
        serde_json::from_value::<forms::Sent>(
            result
                .get("structuredContent")
                .cloned()
                .within("missing send receipt")?,
        )?,
    )?)
}

pub(crate) async fn serve(name: Option<&str>, identity: &str) -> anyhow::Result<()> {
    let agent = Settings::resolve_without_keys(name)?;
    ensure!(agent.key() == identity, "agent context identity changed");
    let out = std::sync::Arc::new(std::sync::Mutex::new(link_format::streams::protocol()));
    let mut questions = crate::question_mcp::Waits::default();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        ensure!(line.len() <= 1 << 20, "context message too large");
        let message: Value = serde_json::from_str(&line)?;
        if message["method"] == "notifications/cancelled" {
            questions.cancel(&message["params"]["requestId"]);
            continue;
        }
        let Some(id) = message.get("id") else {
            continue;
        };
        let answer = match message["method"].as_str() {
            Some("initialize") => Ok(
                json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"agent-context","version":env!("CARGO_PKG_VERSION")}}),
            ),
            Some("tools/list") => {
                let mut tools = json!({"tools":[{"name":"show","description":"What this agent runs","inputSchema":{"type":"object","properties":{"instructions":{"type":"boolean"}},"additionalProperties":false}},{"name":"memory_show","description":"The notes this agent keeps","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},{"name":"tool_list","description":"The action namespaces granted to this agent","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},{"name":"send","description":"Send a message to the owner here or on another connected service, or to a teammate by name","inputSchema":{"type":"object","properties":{"to":{"type":"string"},"connection":{"type":"string","description":places(&agent)},"message":{"type":"string","maxLength":16384}},"required":["to","message"],"additionalProperties":false}}]});
                tools["tools"]
                    .as_array_mut()
                    .expect("tools array")
                    .push(crate::question_mcp::offered());
                tools["tools"]
                    .as_array_mut()
                    .expect("tools array")
                    .extend(crate::interaction_mcp::offered());
                Ok(tools)
            }
            Some("tools/call") => {
                let action = message["params"]["name"].as_str().unwrap_or_default();
                let args = message["params"]
                    .get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                let answer = if action == "question_create" {
                    match questions.start(
                        &agent,
                        args,
                        &message["params"]["_meta"],
                        id.clone(),
                        out.clone(),
                    ) {
                        Ok(()) => continue,
                        Err(err) => Err(err),
                    }
                } else if matches!(
                    action,
                    "question_read" | "question_cancel" | "task_held" | "task_revalidate"
                ) {
                    crate::interaction_mcp::execute(
                        &agent,
                        action,
                        args,
                        &message["params"]["_meta"],
                    )
                } else if action == "send" {
                    send(
                        &agent,
                        args,
                        message["params"].get("_meta").cloned().unwrap_or(json!({})),
                    )
                    .await
                } else {
                    context(&agent, action, args)
                };
                answer.map(|answer|json!({"content":[{"type":"text","text":answer.to_string()}],"structuredContent":answer}))
            }
            _ => anyhow::bail!("unknown context method"),
        };
        let reply = match answer {
            Ok(answer) => json!({"jsonrpc":"2.0","id":id,"result":answer}),
            Err(err) => {
                let message = err
                    .downcast_ref::<Refused>()
                    .map_or("context input was refused", |refused| refused.0.as_str());
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":message}})
            }
        };
        let bytes = serde_json::to_vec(&reply)?;
        ensure!(
            bytes.len() <= link_sandbox_client::routes::TOOL_MESSAGE_MAX,
            "context answer too large"
        );
        let mut out = out
            .lock()
            .map_err(|_| anyhow::anyhow!("context output lock failed"))?;
        out.write_all(&bytes)?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_is_reached_on_their_terminal_and_each_connection_carried() {
        assert_eq!(owner_places(Vec::new()), ["cli"]);
        assert_eq!(
            owner_places(vec!["link".into(), "slack".into(), "telegram".into()]),
            ["cli", "slack", "telegram"]
        );
        let refused: anyhow::Error = Refused("slack is not connected".into()).into();
        assert_eq!(
            refused.downcast_ref::<Refused>().unwrap().0,
            "slack is not connected"
        );
    }

    #[test]
    fn the_schedulers_refusal_of_a_send_reaches_the_model_in_its_words() {
        let cap = "Not sent: 9 messages in a chain, past the cap of 8";
        let told = peer_refusal(
            crate::action_routes::Refused {
                by: "provider",
                reason: cap.into(),
            }
            .into(),
        );
        assert_eq!(told.downcast_ref::<Refused>().expect("told").0, cap);
        let silent = peer_refusal(
            crate::action_routes::Refused {
                by: "namespace",
                reason: String::new(),
            }
            .into(),
        );
        assert!(silent.downcast_ref::<Refused>().is_none());
        let other = peer_refusal(anyhow::anyhow!("namespace failed"));
        assert!(other.downcast_ref::<Refused>().is_none());
    }

    #[test]
    fn messages_require_the_calling_conversation_and_closed_bounded_arguments() {
        let meta = json!({"link.local/thread":"telegram:42"});
        let args = json!({"to":"owner","message":"Ready"});
        assert_eq!(message(args.clone(), &meta).unwrap().thread, "telegram:42");
        let selected = message(
            json!({"to":"owner","message":"Ready","connection":"discord"}),
            &meta,
        )
        .unwrap();
        assert_eq!(selected.connection.as_deref(), Some("discord"));
        assert_eq!(selected.thread, "telegram:42");
        assert!(message(args.clone(), &json!({})).is_err());
        assert!(
            message(
                json!({"to":"owner","message":"Ready","thread":"telegram:99"}),
                &meta
            )
            .is_err()
        );
        assert!(message(json!({"to":"owner","message":"Ready","extra":true}), &meta).is_err());
        assert!(message(json!({"to":"owner","message":" "}), &meta).is_err());
        assert!(
            message(
                json!({"to":"owner","message":"x".repeat((16<<10)+1)}),
                &meta
            )
            .is_err()
        );
        assert!(message(args, &json!({"link.local/thread":"telegram:42\n"})).is_err());
    }
}
