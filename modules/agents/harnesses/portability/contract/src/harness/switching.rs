//! What a switch between two harnesses leaves behind
//! ([`super::Contract::left_behind`]): what the harness it leaves keeps
//! that the one it goes to won't take, from their two contracts.

use super::features::{Feature, Honor};
use crate::asp;

/// One thing a switch leaves behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behind {
    /// Its conversations: the harness it leaves exports none over ASP,
    /// or the one it goes to takes none in. They stay in Link's folder
    /// as it left them, and come back with a switch back.
    Conversations,
    /// The memory files the harness it leaves keeps of its own.
    NativeMemory,
    /// A feature the harness it leaves has, which the other doesn't read
    /// the same way: its skills or its persona.
    Feature(Feature),
}

impl Behind {
    /// It, as a switch says it: "its conversations".
    pub fn said(self, from: &str) -> String {
        match self {
            Self::Conversations => format!("{from}'s conversations"),
            Self::NativeMemory => format!("{from}'s own memory files"),
            Self::Feature(Feature::Skills) => format!("{from}'s skills"),
            Self::Feature(Feature::Persona) => format!("{from}'s persona"),
            Self::Feature(other) => other.words().to_string(),
        }
    }
}

/// The features a switch carries only when both harnesses read them
/// from the agent's spec.
const CARRIED: [Feature; 2] = [Feature::Skills, Feature::Persona];

impl super::Contract {
    /// What a switch from it to `to` leaves behind: its conversations
    /// unless it exports them over ASP and `to` takes them in, its own
    /// memory files, and its skills and persona unless both read them
    /// from the agent's spec. Empty: the switch carries everything
    /// either keeps.
    pub fn left_behind(&self, to: &super::Contract) -> Vec<Behind> {
        let mut behind = Vec::new();
        let sessions = asp::Kind::Sessions;
        if !(self.state.export.contains(&sessions) && to.state.import.contains(&sessions)) {
            behind.push(Behind::Conversations);
        }
        // Carried where it says who the agent is both ways and the other
        // reads it: a harness writing what's remembered elsewhere into its
        // own files, or one reading the agent's notes.
        let carried = self.identity && (to.identity || to.notes);
        if self.native_memory && !carried {
            behind.push(Behind::NativeMemory);
        }
        let (from_honors, to_honors) = (self.honors(), to.honors());
        for feature in CARRIED {
            let both_spec = from_honors.get(&feature) == Some(&Honor::Spec)
                && to_honors.get(&feature) == Some(&Honor::Spec);
            if from_honors.contains_key(&feature) && !both_spec {
                behind.push(Behind::Feature(feature));
            }
        }
        behind
    }

    /// The features `on` (those the agent's spec turns on) it honors
    /// that `to` doesn't, each as left behind: what the agent can do now
    /// that it couldn't after a switch. Skills and persona are
    /// [`Self::left_behind`]'s.
    pub fn not_honored(
        &self,
        to: &super::Contract,
        on: &std::collections::BTreeSet<Feature>,
    ) -> Vec<Behind> {
        let (from_honors, to_honors) = (self.honors(), to.honors());
        on.iter()
            .filter(|f| !CARRIED.contains(f))
            .filter(|f| from_honors.contains_key(f) && !to_honors.contains_key(f))
            .map(|f| Behind::Feature(*f))
            .collect()
    }
}

/// Non-text content a destination can represent in a continued session.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum SessionContent {
    Tools,
    Thinking,
    Images,
    Audio,
    Resources,
    Other,
}

impl SessionContent {
    pub fn words(self) -> &'static str {
        match self {
            Self::Tools => "tool events",
            Self::Thinking => "thinking",
            Self::Images => "images",
            Self::Audio => "audio",
            Self::Resources => "embedded resources",
            Self::Other => "session metadata and resource links",
        }
    }
}

impl super::Contract {
    /// Content actually present that this destination cannot use. This
    /// is separate from whether it imports the sessions kind at all.
    pub fn session_omissions(&self, state: &asp::State) -> Vec<SessionContent> {
        use SessionContent as Kind;
        use asp::acp::schema::v1::{ContentBlock, SessionUpdate};
        let mut found = std::collections::BTreeSet::new();
        for session in state.sessions.iter().flatten() {
            let mut calls = std::collections::BTreeMap::new();
            for update in &session.updates {
                let kind = match update {
                    SessionUpdate::ToolCall(call) => {
                        let name = call.name.clone().unwrap_or_else(|| call.title.clone());
                        calls.insert(
                            call.tool_call_id.0.to_string(),
                            (
                                name,
                                call.raw_input
                                    .as_ref()
                                    .is_some_and(serde_json::Value::is_object),
                                call.status,
                            ),
                        );
                        if call.content.iter().any(|content| {
                            !matches!(content, asp::acp::schema::v1::ToolCallContent::Content(_))
                        }) {
                            found.insert(Kind::Other);
                        }
                        Some(Kind::Tools)
                    }
                    SessionUpdate::ToolCallUpdate(update) => {
                        let call = calls
                            .entry(update.tool_call_id.0.to_string())
                            .or_insert_with(|| {
                                (
                                    String::new(),
                                    false,
                                    asp::acp::schema::v1::ToolCallStatus::Pending,
                                )
                            });
                        if let Some(name) = &update.fields.name {
                            call.0 = name.clone();
                        }
                        call.1 |= update
                            .fields
                            .raw_input
                            .as_ref()
                            .is_some_and(serde_json::Value::is_object);
                        if let Some(status) = update.fields.status {
                            call.2 = status;
                        }
                        if update.fields.content.iter().flatten().any(|content| {
                            !matches!(content, asp::acp::schema::v1::ToolCallContent::Content(_))
                        }) {
                            found.insert(Kind::Other);
                        }
                        Some(Kind::Tools)
                    }
                    SessionUpdate::AgentThoughtChunk(chunk) => Some(
                        if chunk
                            .meta
                            .as_ref()
                            .and_then(|meta| meta.get("link"))
                            .is_some_and(|link| link.get("nativeRecord").is_some())
                        {
                            Kind::Other
                        } else {
                            Kind::Thinking
                        },
                    ),
                    SessionUpdate::UserMessageChunk(chunk)
                    | SessionUpdate::AgentMessageChunk(chunk) => {
                        if chunk.meta.is_some() {
                            found.insert(Kind::Other);
                        }
                        match &chunk.content {
                            ContentBlock::Text(_) => None,
                            ContentBlock::Image(image)
                                if ["image/png", "image/jpeg", "image/gif", "image/webp"]
                                    .contains(&image.mime_type.as_str()) =>
                            {
                                Some(Kind::Images)
                            }
                            ContentBlock::Audio(_) => Some(Kind::Audio),
                            ContentBlock::Resource(resource) => {
                                use asp::acp::schema::v1::EmbeddedResourceResource;
                                let mime = match &resource.resource {
                                    EmbeddedResourceResource::TextResourceContents(text) => {
                                        text.mime_type.as_deref().unwrap_or("text/plain")
                                    }
                                    EmbeddedResourceResource::BlobResourceContents(blob) => {
                                        blob.mime_type.as_deref().unwrap_or_default()
                                    }
                                    _ => "",
                                };
                                if ["text/plain", "text/markdown", "text/csv", "application/pdf"]
                                    .contains(&mime)
                                {
                                    Some(Kind::Resources)
                                } else {
                                    Some(Kind::Other)
                                }
                            }
                            _ => Some(Kind::Other),
                        }
                    }
                    _ => Some(Kind::Other),
                };
                if let Some(kind) = kind
                    && !self.session_content.contains(&kind)
                {
                    found.insert(kind);
                }
            }
            // The live projection cannot complete an unfinished tool call
            // or invent a missing programmatic name or input.
            if calls.values().any(|(name, input, status)| {
                !input
                    || name.is_empty()
                    || name.len() > 128
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                    || !matches!(
                        status,
                        asp::acp::schema::v1::ToolCallStatus::Completed
                            | asp::acp::schema::v1::ToolCallStatus::Failed
                    )
            }) {
                found.insert(Kind::Other);
            }
        }
        if self.session_content.contains(&Kind::Other) {
            found.remove(&Kind::Other);
        }
        found.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::Contract;
    use super::super::features::{Feature, Honor, Honors};
    use super::*;

    fn sessions() -> asp::AgentState {
        asp::AgentState {
            export: vec![asp::Kind::Sessions],
            import: vec![asp::Kind::Sessions],
        }
    }

    fn link() -> Contract {
        Contract {
            state: sessions(),
            notes: true,
            features: Some(Feature::ALL.into_iter().map(|f| (f, Honor::Spec)).collect()),
            ..Contract::new(true)
        }
    }

    fn native(state: asp::AgentState, memory: bool, features: &[Feature]) -> Contract {
        let honors: Honors = features.iter().map(|f| (*f, Honor::Native)).collect();
        Contract {
            state,
            native_memory: memory,
            features: Some(honors),
            ..Contract::new(true)
        }
    }

    #[test]
    fn link_to_link_carries_everything() {
        assert_eq!(link().left_behind(&link()), vec![]);
    }

    #[test]
    fn conversations_cross_where_one_exports_them_and_the_other_takes_them() {
        let hermes = native(sessions(), true, &[Feature::Persona, Feature::Skills]);
        assert_eq!(
            link().left_behind(&hermes),
            vec![
                Behind::Feature(Feature::Skills),
                Behind::Feature(Feature::Persona),
            ]
        );
        // Back again: its own memory files stay too.
        assert_eq!(
            hermes.left_behind(&link()),
            vec![
                Behind::NativeMemory,
                Behind::Feature(Feature::Skills),
                Behind::Feature(Feature::Persona),
            ]
        );
        // One that exports its sessions but takes none in.
        let openclaw = native(
            asp::AgentState {
                export: vec![asp::Kind::Sessions],
                import: vec![],
            },
            false,
            &[],
        );
        assert_eq!(hermes.left_behind(&openclaw)[0], Behind::Conversations);
        assert!(
            !openclaw
                .left_behind(&hermes)
                .contains(&Behind::Conversations)
        );
    }

    #[test]
    fn a_harness_carrying_who_the_agent_is_brings_its_memory_to_one_that_reads_it() {
        let identity = |mut c: Contract| {
            c.identity = true;
            c
        };
        let hermes = identity(native(sessions(), true, &[Feature::Persona]));
        let openclaw = identity(native(sessions(), true, &[Feature::Persona]));
        let deepseek = native(asp::AgentState::default(), false, &[]);
        assert!(!hermes.left_behind(&link()).contains(&Behind::NativeMemory));
        assert!(
            !hermes
                .left_behind(&openclaw)
                .contains(&Behind::NativeMemory)
        );
        assert!(
            hermes
                .left_behind(&deepseek)
                .contains(&Behind::NativeMemory)
        );
    }

    #[test]
    fn what_the_spec_turns_on_and_the_other_doesn_t_honor_is_said() {
        let hermes = native(sessions(), true, &[Feature::Persona, Feature::Subagents]);
        let on = [
            Feature::Persona,
            Feature::Schedules,
            Feature::Subagents,
            Feature::Lsp,
        ]
        .into();
        assert_eq!(
            link().not_honored(&hermes, &on),
            vec![
                Behind::Feature(Feature::Schedules),
                Behind::Feature(Feature::Lsp),
            ]
        );
        assert!(hermes.not_honored(&link(), &on).is_empty());
        assert_eq!(
            Behind::Feature(Feature::Schedules).said("link"),
            "schedules"
        );
    }

    #[test]
    fn a_harness_speaking_no_agent_state_leaves_its_conversations() {
        let deepseek = native(asp::AgentState::default(), false, &[Feature::Skills]);
        assert_eq!(
            link().left_behind(&deepseek),
            vec![
                Behind::Conversations,
                Behind::Feature(Feature::Skills),
                Behind::Feature(Feature::Persona),
            ]
        );
        assert_eq!(
            Behind::Conversations.said("deepseek"),
            "deepseek's conversations"
        );
        assert_eq!(
            Behind::NativeMemory.said("hermes"),
            "hermes's own memory files"
        );
        // From an adapter before it: none.
        let old: Contract = serde_json::from_str(r#"{"version": 2}"#).unwrap();
        assert_eq!(old.state, asp::AgentState::default());
        assert!(
            serde_json::to_value(Contract::new(true))
                .unwrap()
                .get("state")
                .is_none()
        );
    }
}

#[cfg(test)]
mod content_tests {
    use super::*;
    use asp::acp::schema::v1::{
        ContentBlock, ContentChunk, ImageContent, SessionId, SessionUpdate, TextContent,
    };

    #[test]
    fn sessions_are_not_a_claim_of_non_text_fidelity() {
        let state = asp::State {
            sessions: Some(vec![asp::forms::Session {
                evidence: None,
                harness: None,
                session_id: SessionId::new("one"),
                title: None,
                updates: vec![
                    SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Image(
                        ImageContent::new("iVBORw0KGgo=", "image/png"),
                    ))),
                    SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                        TextContent::new("A thought"),
                    ))),
                ],
            }]),
            ..asp::State::default()
        };
        let mut destination = super::super::Contract::new(true);
        destination.state.import = vec![asp::Kind::Sessions];
        assert_eq!(
            destination.session_omissions(&state),
            [SessionContent::Thinking, SessionContent::Images]
        );
        destination.session_content = vec![SessionContent::Images];
        assert_eq!(
            destination.session_omissions(&state),
            [SessionContent::Thinking]
        );
    }
}
