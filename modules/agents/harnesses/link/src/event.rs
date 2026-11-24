//! One event of a conversation: a message, a tool's answer, and what
//! came with it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A message in a conversation, as it's kept: never changed once
/// written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Its place in the conversation, from 1: first, so a reader finds a
    /// line by its number without reading the rest of it.
    pub seq: u64,
    /// Trusted execution observations, never inferred from model text.
    /// Shared, not copied, when the event is: a model request's lists
    /// every message before it, and a turn's events are copied as they're
    /// read and passed on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<std::sync::Arc<link_harness_wire::evidence::Evidence>>,
    /// The durable canonical model attempt that produced this answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<link_interactions::TransportEventId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<link_interactions::InputId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<link_interactions::ScheduledOccurrence>,
    /// A subagent's first message: the conversation that started it, as
    /// its scheduler named it, its lineage in a store of the harness's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// `user` (the owner), `assistant` (the agent), `tool` (a tool's
    /// answer to one of the agent's calls), `summary` (the agent's
    /// summary of what left its window) or `command` (a command the
    /// owner ran, `/new`: [`crate::commands`]).
    pub role: String,
    pub text: String,
    /// The agent's: the tools it calls, each answered by an event of its
    /// own that follows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<Call>,
    /// A tool's: the id of the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    /// A tool's: the name the call was made by (`memory__search`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// A tool's: it failed, or was refused, and `text` says why.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
    /// When it was written, UTC (`2026-09-29T14:03:07Z`).
    pub at: String,
    /// The model that wrote it, as the provider names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// The provider's id for its response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    /// From sending the request to its last token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// In US dollars: what the provider reported, else the tokens it
    /// counted at the model's price, when that's known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// The agent's: its thinking before it answered, as the provider's
    /// own blocks, sent back with its calls as the provider asks. Never
    /// shown to people.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thinking: Vec<Value>,
    /// The owner's side's: who it's from when it isn't the owner
    /// (`schedule:<id>`, `task:<id>`, `agent:<id>`, `run:<id>`,
    /// `webhook:<name>`, or `member:<connection>:<user id>`, someone in a chat
    /// the agent is shared with), as `Said::from` said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// The owner's side's: a `user:` sender's name on their connection, as
    /// `Said::from_name` said: words for the model, never who they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_name: Option<String>,
    /// How many messages, each sent by an agent on its own, led to it
    /// (`Said::hops`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub hops: u32,
    /// The owner's side's: the most it stands (`Said::standing`), the
    /// standing of the conversation that made its schedule or subagent.
    /// None: as `from` says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standing: Option<link_connection_contract::access::Standing>,
    /// The agent's: cut short, its turn asked to end while it was
    /// written; its calls weren't made.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interrupted: bool,
    /// The agent's: answered by its fallback model, and why its model
    /// didn't (`off`, `window`: [`crate::fallback::Why`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// The agent's: the number of the last event the model was asked
    /// with. A message after it came while the model answered, and is
    /// still to answer.
    #[serde(default, skip_serializing_if = "is_zero_seq")]
    pub seen: u64,
    /// The owner's side's: the date and time it came, its connection and
    /// who's speaking, as the model reads them at its head (the prompt's
    /// last section, `[instructions] now`). Kept, so it reads the same
    /// at every turn after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    /// A summary's: the number of the first event it doesn't cover. It
    /// stands for every event before that one, the summary before it
    /// included; its `calls` are those it saved to notes first.
    #[serde(default, skip_serializing_if = "is_zero_seq")]
    pub covers: u64,
    /// The agent's: where the prompt's window starts as of it, so the
    /// next turn walks on from there, not from the first event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<crate::window::Mark>,
    /// The owner's side's, or a tool's: its images and documents, each a
    /// file beside the events, never in them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    /// The exact imported ACP update, retained for lossless ASP export.
    /// Its native fields are a projection for inference, never its archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portable: Option<Vec<link_agent_state_protocol::acp::schema::v1::SessionUpdate>>,
}

link_files::specification!(Event, include_str!("event.schema.toml"));

pub use link_model_contract::chat::media::{
    Call, Kind, Media, PART_MAX, PARTS_MAX, Part, Tokens, decode,
};

pub const USER: &str = "user";
pub const ASSISTANT: &str = "assistant";
pub const TOOL: &str = "tool";
pub const SUMMARY: &str = "summary";
pub const COMMAND: &str = "command";
pub const EVIDENCE: &str = "evidence";

impl Event {
    pub fn user(text: &str) -> Self {
        Self {
            role: USER.into(),
            text: text.into(),
            at: now(),
            ..Self::default()
        }
    }

    /// Whether it's the agent's answer: its text, and no call still to
    /// make.
    pub fn is_answer(&self) -> bool {
        self.role == ASSISTANT && self.calls.is_empty() && !self.text.trim().is_empty()
    }

    /// Its role as people read it: a command's is the owner's, who typed
    /// it.
    pub fn copied_role(&self) -> &str {
        match self.role.as_str() {
            COMMAND => USER,
            role => role,
        }
    }

    /// Its text, after a line naming each of its parts, as people read
    /// it (`[image: photo.jpg]`).
    pub fn said(&self) -> std::borrow::Cow<'_, str> {
        if self.parts.is_empty() {
            return self.text.as_str().into();
        }
        let mut said: Vec<String> = self.parts.iter().map(Part::named).collect();
        if !self.text.is_empty() {
            said.push(self.text.clone());
        }
        said.join("\n").into()
    }
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

fn is_zero_seq(n: &u64) -> bool {
    *n == 0
}

/// Now, UTC, as `2026-09-29T14:03:07Z`.
pub fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    timestamp(secs)
}

/// `secs` since 1970 as `2026-09-29T14:03:07Z`.
pub fn timestamp(secs: u64) -> String {
    let (year, month, day, rest) = civil(secs);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// `secs` since 1970 as its year, month, day and the seconds into that
/// day.
pub fn civil(secs: u64) -> (i64, i64, i64, u64) {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, rest)
}

/// `at`, as [`timestamp`] writes one, in seconds since 1970: none for
/// anything else.
pub fn seconds(at: &str) -> Option<u64> {
    let b = at.as_bytes();
    let shape = b.len() == 20
        && [4, 7].iter().all(|i| b[*i] == b'-')
        && b[10] == b'T'
        && [13, 16].iter().all(|i| b[*i] == b':')
        && b[19] == b'Z';
    if !shape {
        return None;
    }
    let number = |from: usize, to: usize| at.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    // Howard Hinnant's days-from-civil, the inverse of [`civil`]'s.
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A timestamp reads back as the seconds it was written from.
    #[test]
    fn timestamps_read_back() {
        for secs in [0, 951_782_400, 1_790_690_587, 4_102_444_799] {
            assert_eq!(seconds(&timestamp(secs)), Some(secs));
        }
        assert_eq!(seconds("2026-09-29 14:03:07"), None);
        assert_eq!(seconds("yesterday"), None);
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(timestamp(1_790_690_587), "2026-09-29T14:03:07Z");
    }

    #[test]
    fn it_takes_what_connections_download() {
        let taken = link_model_contract::chat::media::MEDIA;
        assert_eq!(taken, link_connection_contract::MEDIA);
    }

    #[test]
    fn an_event_leaves_out_what_it_lacks() {
        let json = serde_json::to_string(&Event::user("hi")).unwrap();
        for absent in [
            "model",
            "tokens",
            "cost",
            "response_id",
            "calls",
            "call",
            "tool",
            "failed",
        ] {
            assert!(!json.contains(absent), "{json}");
        }
    }
}
