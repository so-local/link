//! Canonical model attempts and explicit source attribution. Media
//! selection here precedes provider-specific conversion, not proof of
//! provider media delivery.

use super::{Captured, Phase, WorkRef};
use link_files::reading::ContentDigest;
use link_model_contract::chat::media::Part;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Source {
    Native { seq: u64 },
    Imported { seq: u64 },
    Synthetic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Slot {
    Text { start: u64, end: u64 },
    Answer { index: u32, call: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Coverage {
    Whole,
    Omitted { reason: String },
    Unknown { reason: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub source: Source,
    pub message: u32,
    pub slot: Slot,
    pub source_text: ContentDigest,
    pub projected_text: ContentDigest,
    pub coverage: Coverage,
    pub media: Vec<Part>,
    pub omissions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestAnswer {
    pub call: String,
    pub text: ContentDigest,
    pub failed: bool,
    pub media: Vec<ContentDigest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestCall {
    pub id: String,
    pub name: String,
    pub arguments: ContentDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestMessage {
    pub role: String,
    pub text: ContentDigest,
    pub answers: Vec<RequestAnswer>,
    pub calls: Vec<RequestCall>,
    pub media: Vec<ContentDigest>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestTool {
    pub name: String,
    /// Stable digest of the actual name, description, and schema.
    pub declaration: ContentDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaDelivery {
    /// Canonical selection only. A provider may omit or transform a
    /// selected image or document; delivery has not been established.
    CanonicalOnly,
}

/// Recorded after fallback selection and system substitution, before
/// this canonical request may leave. The successful containing append
/// is its identity. A response's `request` links the producing attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub phase: Phase,
    pub attempt: String,
    pub work: Option<WorkRef>,
    pub model: Option<String>,
    pub provider: String,
    pub system: ContentDigest,
    pub tools: Vec<RequestTool>,
    pub no_calls: bool,
    pub projection: Captured<RequestProjection>,
    pub media_delivery: MediaDelivery,
}

/// The messages a request sent and their sources: whole, or as the
/// request before it in the conversation and what changed. Each request
/// lists every message of its window, so listing them whole every time
/// grows a conversation's evidence with the square of its window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RequestProjection {
    Whole(Projected),
    Continued(Continued),
}

/// A request's every message, in order, and their sources, each naming
/// its message by its place among them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projected {
    pub messages: Vec<RequestMessage>,
    pub sources: Vec<Contribution>,
}

/// A request that begins with `kept` of the messages of the request
/// before it (`after`, its sequence), from its `from`th, with their
/// sources, and goes on with `messages`. Its `sources` are those of the
/// messages it adds, numbered as its own, so each is `kept` or more.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continued {
    pub after: u64,
    pub from: u32,
    pub kept: u32,
    pub messages: Vec<RequestMessage>,
    pub sources: Vec<Contribution>,
}

impl RequestProjection {
    /// The sources this record lists itself: a whole one's every source,
    /// a continued one's of the messages it adds.
    pub fn own_sources(&self) -> &[Contribution] {
        match self {
            Self::Whole(whole) => &whole.sources,
            Self::Continued(continued) => &continued.sources,
        }
    }
}

impl Projected {
    /// The request `continued` says, this being the request it goes on
    /// from: an error when it keeps messages this doesn't have.
    pub fn continued(&self, continued: &Continued) -> anyhow::Result<Projected> {
        let (from, kept) = (continued.from as usize, continued.kept as usize);
        let Some(end) = from
            .checked_add(kept)
            .filter(|end| *end <= self.messages.len())
        else {
            anyhow::bail!("a continued model request keeps messages the one before it hasn't");
        };
        let mut messages = self.messages[from..end].to_vec();
        messages.extend(continued.messages.iter().cloned());
        let mut sources: Vec<Contribution> = self
            .sources
            .iter()
            .filter(|source| (from..end).contains(&(source.message as usize)))
            .cloned()
            .map(|mut source| {
                source.message -= continued.from;
                source
            })
            .collect();
        sources.extend(continued.sources.iter().cloned());
        Ok(Projected { messages, sources })
    }

    /// `next`, the request after this one (`after`, its sequence), as
    /// what changed: the longest end of this one's messages that begins
    /// `next`'s, with their sources, and the rest. None when `next`
    /// doesn't begin with one, or isn't made again exactly from it.
    pub fn continue_to(&self, after: u64, next: &Projected) -> Option<Continued> {
        let from = (0..self.messages.len()).find(|&from| {
            let end = &self.messages[from..];
            next.messages.len() >= end.len() && next.messages[..end.len()] == *end
        })?;
        let kept = self.messages.len() - from;
        let continued = Continued {
            after,
            from: u32::try_from(from).ok()?,
            kept: u32::try_from(kept).ok()?,
            messages: next.messages[kept..].to_vec(),
            sources: next
                .sources
                .iter()
                .filter(|source| source.message as usize >= kept)
                .cloned()
                .collect(),
        };
        (self.continued(&continued).ok()? == *next).then_some(continued)
    }
}
