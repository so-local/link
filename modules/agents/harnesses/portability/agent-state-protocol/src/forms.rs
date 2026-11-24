//! State in portable form: each kind in a form that already exists,
//! never one of Link's. A session is ACP's own `session/update`
//! stream, as `session/load` replays it; instructions are `AGENTS.md`;
//! skills are Agent Skills folders; tools are ACP's MCP server
//! descriptors; memory is Markdown notes; settings are session config
//! values. Files stay in the workspace, named by path, and never cross.

use std::collections::BTreeMap;

use crate::acp::schema::v1::{
    ContentBlock, ContentChunk, McpServer, SessionId, SessionUpdate, TextContent,
};
use serde::{Deserialize, Serialize};

/// A kind of state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Sessions,
    Instructions,
    Skills,
    Tools,
    Memory,
    Settings,
    Schedules,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Sessions,
        Kind::Instructions,
        Kind::Skills,
        Kind::Tools,
        Kind::Memory,
        Kind::Settings,
        Kind::Schedules,
    ];
}

/// An agent's state, each kind that was asked for: a kind left out is
/// one not given, never one given empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// Native state retained in its source archive but not reconstructed as
    /// usable portable context. An omission never permits erasing that archive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omissions: Vec<crate::methods::NotTaken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules: Option<crate::schedules::Schedules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Vec<Session>>,
    /// The agent's `AGENTS.md`, whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<Skill>>,
    /// As `session/new` takes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<McpServer>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Vec<Note>>,
    /// Each session config option's value, by the option's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<BTreeMap<String, serde_json::Value>>,
}

impl State {
    /// The kinds it gives.
    pub fn kinds(&self) -> Vec<Kind> {
        let given = [
            self.sessions.is_some(),
            self.instructions.is_some(),
            self.skills.is_some(),
            self.tools.is_some(),
            self.memory.is_some(),
            self.settings.is_some(),
            self.schedules.is_some(),
        ];
        Kind::ALL
            .into_iter()
            .zip(given)
            .filter_map(|(kind, given)| given.then_some(kind))
            .collect()
    }
}

/// A conversation, as the updates `session/load` would replay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Its id where it was exported: an import answers with its new one.
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub updates: Vec<SessionUpdate>,
    /// Observations supplied by this local exporter, outside imported
    /// updates. An importer must never promote them to local execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<link_harness_wire::evidence::export::SessionEvidence>,
    /// The harness that exported it, where it's carried from another:
    /// a destination that keeps it beside a version of its own names the
    /// copy for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
}

impl Session {
    /// Evidence whose native sequence and portable update classification
    /// are valid. Absence does not establish execution or completeness.
    pub fn native_evidence(
        &self,
    ) -> anyhow::Result<Option<&link_harness_wire::evidence::export::SessionEvidence>> {
        let Some(evidence) = &self.evidence else {
            return Ok(None);
        };
        anyhow::ensure!(
            evidence.conversation.thread == self.session_id.0.as_ref(),
            "evidence belongs to another conversation"
        );
        evidence.validate(self.updates.len())?;
        Ok(Some(evidence))
    }

    /// Its messages, for a harness that keeps a conversation as who said
    /// what: each run of the user's or the agent's text joined into one,
    /// its calls and thinking left out.
    pub fn messages(&self) -> Vec<Message> {
        let mut messages: Vec<Message> = Vec::new();
        for update in &self.updates {
            let (by, chunk) = match update {
                SessionUpdate::UserMessageChunk(chunk) => (By::User, chunk),
                SessionUpdate::AgentMessageChunk(chunk) => (By::Agent, chunk),
                _ => continue,
            };
            let ContentBlock::Text(text) = &chunk.content else {
                continue;
            };
            match messages.last_mut() {
                Some(last) if last.by == by && chunk.message_id.is_none() => {
                    last.text.push_str(&text.text)
                }
                _ => messages.push(Message {
                    by,
                    text: text.text.clone(),
                }),
            }
        }
        messages.retain(|message| !message.text.trim().is_empty());
        messages
    }

    /// A session of `messages`, as `session/load` replays them: a chunk
    /// each.
    pub fn of_messages(session_id: SessionId, title: Option<String>, messages: &[Message]) -> Self {
        let updates = messages
            .iter()
            .map(|message| {
                let chunk =
                    ContentChunk::new(ContentBlock::Text(TextContent::new(message.text.clone())));
                match message.by {
                    By::User => SessionUpdate::UserMessageChunk(chunk),
                    By::Agent => SessionUpdate::AgentMessageChunk(chunk),
                }
            })
            .collect();
        Session {
            evidence: None,
            harness: None,
            session_id,
            title,
            updates,
        }
    }
}

/// A message of a conversation, by who said it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub by: By,
    pub text: String,
}

/// Who said a message: the agent's owner, or someone speaking to it, or
/// the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    User,
    Agent,
}

/// An Agent Skills folder: its files by their paths in it,
/// `SKILL.md` among them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub files: BTreeMap<String, String>,
}

/// A Markdown note of the agent's memory, by its path, with where it
/// came from; a deleted note says so, so an import forgets it too.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub path: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_gives_only_the_kinds_it_holds() {
        let state = State {
            instructions: Some("# Agent\n".into()),
            memory: Some(Vec::new()),
            ..State::default()
        };
        assert_eq!(state.kinds(), [Kind::Instructions, Kind::Memory]);
        let wire = serde_json::to_value(&state).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"instructions": "# Agent\n", "memory": []})
        );
        assert_eq!(serde_json::from_value::<State>(wire).unwrap(), state);
        assert!(serde_json::from_value::<Kind>(serde_json::json!("teleport")).is_err());
    }

    #[test]
    fn a_session_is_acps_own_updates() {
        let said = SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("hello"),
        )));
        let session = Session {
            evidence: None,
            harness: None,
            session_id: SessionId::new("sess_1"),
            title: None,
            updates: vec![said],
        };
        let wire = serde_json::to_value(&session).unwrap();
        assert_eq!(wire["sessionId"], "sess_1");
        assert_eq!(wire["updates"][0]["sessionUpdate"], "user_message_chunk");
        assert_eq!(serde_json::from_value::<Session>(wire).unwrap(), session);
    }

    #[test]
    fn a_session_reads_as_its_messages_and_back() {
        let chunk = |text: &str| ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
        let session = Session {
            evidence: None,
            harness: None,
            session_id: SessionId::new("sess_1"),
            title: Some("Plans".into()),
            updates: vec![
                SessionUpdate::UserMessageChunk(chunk("hello")),
                SessionUpdate::AgentMessageChunk(chunk("hi, ")),
                SessionUpdate::AgentThoughtChunk(chunk("they said hello")),
                SessionUpdate::AgentMessageChunk(chunk("there")),
                SessionUpdate::UserMessageChunk(chunk("  ")),
            ],
        };
        let messages = session.messages();
        assert_eq!(
            messages,
            [
                Message {
                    by: By::User,
                    text: "hello".into()
                },
                Message {
                    by: By::Agent,
                    text: "hi, there".into()
                },
            ]
        );
        let back = Session::of_messages(SessionId::new("sess_1"), None, &messages);
        assert_eq!(back.messages(), messages);
        assert_eq!(back.updates.len(), 2);
    }
}
