//! What Link adds to ACP, the session protocol between a bridge and an
//! agent's harness: where a harness takes ACP connections, and what
//! Link says beside a session's requests, in their `_meta` under
//! [`META`]. Both sides read the one type, [`Said`].

use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::{JsonRpcNotification, JsonRpcResponse};
use serde::{Deserialize, Serialize};

pub(super) mod client;
pub use client::{exported, imported};
#[cfg(any(test, feature = "test-support"))]
pub mod fake;

/// A WebSocket to a harness's ACP, as a bridge opens it.
pub type Socket =
    tokio_tungstenite::WebSocketStream<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>;

/// The session of a message that names no thread.
pub const DEFAULT_THREAD: &str = "default";

/// Told by the harness as soon as it has taken a prompt's message, its
/// answer still to come: a webhook's sender is answered then, not once
/// the turn ends. A message that joins a running turn, or a command,
/// is taken when its prompt answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonRpcNotification)]
#[notification(method = "_link/taken")]
#[serde(rename_all = "camelCase")]
pub struct Taken {
    pub session_id: SessionId,
}

/// The method a bridge asks a harness for the answers of turns it
/// finished after a crash, which nobody waited for: [`crate::Recover`]
/// asked, [`RecoveredAnswers`] answered.
pub const RECOVER: &str = "_link/recover";

/// What [`RECOVER`] answers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonRpcResponse)]
pub struct RecoveredAnswers {
    pub answers: Vec<crate::Recovered>,
}

/// Where a harness takes ACP connections, on its endpoint's port: a
/// WebSocket, one JSON-RPC message to a text frame, opened with the
/// agent's headers and its endpoint key as any request to it is.
pub const ACP_PATH: &str = "/acp";

/// The key of `_meta` Link's words are under.
pub const META: &str = "link";

/// What Link says beside a request: on `session/new`, the thread the
/// session is ([`crate::acp::Said::thread`]'s); on `session/prompt`, who the
/// message is from when it isn't the owner ([`crate::acp::Said::from`]),
/// their name, how many messages led to it ([`crate::acp::Said::hops`]),
/// the tools the conversation narrows to ([`crate::acp::Said::tools`]) and a
/// subagent's parent conversation ([`crate::acp::Said::parent`]). On a
/// prompt's answer, that it joined a turn already running.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Said {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub receipts: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<link_interactions_contract::TransportEventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_receipt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<link_interactions_contract::ScheduledOccurrence>,
    /// A subagent's prompt (`from` `task:<id>`): the conversation that
    /// started it, which a harness keeping its work in a store of its own
    /// records as the subagent's lineage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_name: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hops: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// The most a firing's message stands, as the conversation that made
    /// its schedule or subagent stood: its standing is `from`'s at most
    /// this ([`link_agent_identity::access::Standing::at_most`]). None: as
    /// `from` says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standing: Option<link_agent_identity::access::Standing>,
    /// The model its conversation runs on from now on, a model or an
    /// alias: a subagent's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// On a tool call's update: whether the chat shows it, as its
    /// conversation chose (`/chat:verbose`) in place of the spec's
    /// `[connections] activity`. None: as the spec says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub steered: bool,
}

/// Whether `thread` can name a conversation: short, and nothing a
/// terminal would read as more than text.
fn is_thread(thread: &str) -> bool {
    !thread.is_empty() && thread.len() <= 128 && link_format::clean(thread) == thread
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A request's `_meta`, as JSON-RPC carries it.
pub type Meta = serde_json::Map<String, serde_json::Value>;

impl Said {
    /// `meta` with it under [`META`], or without it when it says nothing.
    pub fn into_meta(self, mut meta: Meta) -> Meta {
        match serde_json::to_value(&self) {
            Ok(value) if self != Self::default() => {
                meta.insert(META.into(), value);
            }
            _ => {
                meta.remove(META);
            }
        }
        meta
    }

    /// What `meta` says, checked: refused, and why, when it names a
    /// sender that isn't one, a name beside anyone but a person, or
    /// tools that aren't a list of tools' names. Nothing, from no
    /// `_meta` or none of Link's.
    pub fn of(meta: Option<&Meta>) -> anyhow::Result<Self> {
        let Some(said) = meta.and_then(|m| m.get(META)) else {
            return Ok(Self::default());
        };
        let said: Self = serde_json::from_value(said.clone())
            .map_err(|err| anyhow::anyhow!("its _meta.link isn't Link's: {err}"))?;
        anyhow::ensure!(
            said.delivery.is_none() || said.from.is_some(),
            "delivery correlation needs source provenance"
        );
        if let Some(occurrence) = &said.occurrence {
            anyhow::ensure!(
                said.from
                    .as_deref()
                    .is_some_and(|f| f.starts_with("schedule:"))
                    && occurrence.revision > 0
                    && !occurrence.schedule.0.is_empty(),
                "occurrence requires authenticated schedule provenance"
            );
        }
        if let Some(parent) = &said.parent {
            anyhow::ensure!(
                said.from.as_deref().is_some_and(|f| f.starts_with("task:")),
                "a parent conversation comes only with a subagent's message"
            );
            anyhow::ensure!(is_thread(parent), "that parent isn't a thread");
        }
        if let Some(thread) = &said.thread {
            anyhow::ensure!(is_thread(thread), "that thread isn't a thread");
        }
        if let Some(from) = &said.from {
            anyhow::ensure!(crate::is_from(from), "that from isn't a sender");
        }
        if let Some(name) = &said.from_name {
            let person = said
                .from
                .as_deref()
                .is_some_and(|f| f.parse::<crate::access::ChatMember>().is_ok());
            anyhow::ensure!(
                person && crate::is_person_name(name),
                "that fromName isn't a person's name"
            );
        }
        if let Some(tools) = &said.tools {
            anyhow::ensure!(
                crate::tools_of(&tools.join(",")).is_some_and(|t| t.len() == tools.len()),
                "those tools aren't a list of tools' names"
            );
        }
        if let Some(model) = &said.model {
            anyhow::ensure!(
                crate::is_model_name(model),
                "that model isn't a model's name"
            );
        }
        Ok(said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A subagent's message names the conversation that started it; a
    /// parent beside any other sender, or one that isn't a thread, is
    /// refused.
    #[test]
    fn a_parent_comes_only_with_a_subagents_message() {
        let said = Said {
            from: Some("task:a1b2c3d4e5f6".into()),
            parent: Some("link:main".into()),
            ..Said::default()
        };
        let meta = said.clone().into_meta(Meta::new());
        assert_eq!(
            serde_json::Value::Object(meta.clone()),
            serde_json::json!({"link": {"from": "task:a1b2c3d4e5f6", "parent": "link:main"}})
        );
        assert_eq!(Said::of(Some(&meta)).unwrap(), said);
        let refused = |link: serde_json::Value| {
            let meta = serde_json::json!({ "link": link }).as_object().cloned();
            Said::of(meta.as_ref()).is_err()
        };
        assert!(refused(
            serde_json::json!({"from": "member:telegram:42", "parent": "link:main"})
        ));
        assert!(refused(serde_json::json!({"parent": "link:main"})));
        assert!(refused(
            serde_json::json!({"from": "task:a1b2c3d4e5f6", "parent": ""})
        ));
        assert!(refused(
            serde_json::json!({"from": "task:a1b2c3d4e5f6", "parent": "a\u{1b}[2Jb"})
        ));
    }

    #[test]
    fn what_link_says_is_under_meta_and_checked() {
        let said = Said {
            from: Some("member:telegram:42".into()),
            from_name: Some("Ada".into()),
            hops: 2,
            tools: Some(vec!["memory__search".into()]),
            ..Said::default()
        };
        let meta = said.clone().into_meta(Meta::new());
        assert_eq!(
            serde_json::Value::Object(meta.clone()),
            serde_json::json!({"link": {"from": "member:telegram:42", "fromName": "Ada",
                "hops": 2, "tools": ["memory__search"]}})
        );
        assert_eq!(Said::of(Some(&meta)).unwrap(), said);
        assert_eq!(Said::of(None).unwrap(), Said::default());
        assert!(Said::default().into_meta(meta).is_empty());
        let refused = |link: serde_json::Value| {
            let meta = serde_json::json!({ "link": link }).as_object().cloned();
            Said::of(meta.as_ref()).is_err()
        };
        assert!(refused(serde_json::json!({"from": "nobody"})));
        assert!(refused(
            serde_json::json!({"from": "webhook:daily", "fromName": "Ada"})
        ));
        assert!(refused(serde_json::json!({"tools": ["not a tool"]})));
        assert!(refused(serde_json::json!({"teleport": true})));
        assert!(!refused(
            serde_json::json!({"from": "webhook:daily", "hops": 1})
        ));
    }
}
