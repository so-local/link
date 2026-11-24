//! The model namespace's request and response shapes.
use crate::Thinks;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub mod calls;
pub mod lines;
pub mod media;
pub use media::{Call, Kind, Media, Tokens};
#[cfg(feature = "client")]
pub mod client;
/// How hard a model thinks, declared in the model's `types.toml`.
pub use crate::declared::Thinking;

impl Thinking {
    pub fn word(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// A message of the prompt: the owner's, with the tools' answers to the
/// agent's calls before it, or the agent's, with its calls after.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Message {
    pub role: String,
    pub text: String,
    /// The agent's: the tools it called.
    pub calls: Vec<Call>,
    /// The owner's: the answers to the calls of the message before.
    pub answers: Vec<Answer>,
    /// The agent's: its thinking before them, the provider's own blocks.
    pub thinking: Vec<Value>,
    /// The owner's: the images and documents of its messages, sent
    /// before its text.
    pub parts: Vec<Media>,
}

/// A tool's answer to a call, in the prompt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    /// The call's id.
    pub call: String,
    pub text: String,
    pub failed: bool,
    /// Its images and documents.
    pub parts: Vec<Media>,
}

/// The most bytes of parts one prompt holds, the latest first; those
/// before them are named and left out. Sent in base64, they take a third
/// more, within the 32 MiB a request through Link's proxy holds.
pub const PARTS_SENT_MAX: u64 = 20 << 20;

/// The most of an image Anthropic takes, in base64.
#[cfg(feature = "client")]
const ANTHROPIC_IMAGE_MAX: usize = 5 << 20;

/// What a call left without an answer is answered with: its turn ended
/// before the tool's answer was kept.
pub const UNANSWERED: &str = "It wasn't answered: the turn ended first.";

/// What a summary at the window's head is introduced with.
pub const SUMMARY_HEAD: &str =
    "[A summary of this conversation before this point, which you wrote as it left your window:]";

/// What the model is asked.
#[derive(Serialize)]
pub struct Prompt<'a> {
    pub system: &'a str,
    pub messages: &'a [Message],
    /// The tools it's offered; none, and it's offered none.
    pub tools: &'a [Tool],
    /// Offer the tools, but tell it to call none: the turn's last round.
    pub no_calls: bool,
    /// The longest answer the model writes, in tokens: its own
    /// ([`crate::limits`]).
    pub answer: u32,
    /// The window it's sent, in tokens, as the agent's spec and what's
    /// known of the model make it: said when the provider refuses a
    /// prompt as too long.
    pub window: u32,
    /// What the model costs, when it's known: a turn's cost where the
    /// provider doesn't say it, from the tokens the provider counted.
    pub price: Option<crate::Price>,
    /// Where its price jumps for a long prompt (the facts' `above`).
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub above: &'a [crate::Jump],
    /// How it's asked to think: as the model can, as hard as the agent's
    /// spec says.
    pub thinking: Think,
    /// Asked of the provider's faster service, where it offers one
    /// ([`offers_fast`]): sent to no other.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fast: bool,
    /// The chat it's asked in, by its caller's name for it: what the
    /// bridge keeps its record under ([`crate::context`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<&'a str>,
    /// The ranges of `system` its caller composed, by name, which the
    /// bridge counts beneath `instructions` ([`crate::context::split`]).
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub spans: &'a [crate::context::Span],
    /// The chat that asked for this one's work (a subagent's, its
    /// compaction's): what the bridge counts it under too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<&'a str>,
    /// How long the provider's cache is asked to keep the prompt (a
    /// [`crate::Offer`]'s `lasts`), chosen by what it costs
    /// ([`crate::caching`]); none, as long as the provider keeps it
    /// unasked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<&'a str>,
}

/// Whether `provider` offers a faster service for its model `name`:
/// OpenAI's priority processing for each of its models, Anthropic's fast
/// mode for the models that have it.
pub fn offers_fast(provider: &str, name: &str) -> bool {
    match provider {
        "openai" => true,
        "anthropic" => FAST_ANTHROPIC.contains(&name),
        _ => false,
    }
}

/// Anthropic's models with a fast mode.
const FAST_ANTHROPIC: [&str; 3] = ["claude-opus-5-5", "claude-opus-5", "claude-opus-4-8"];

/// The beta Anthropic's fast mode is asked under.
pub const FAST_BETA: &str = "fast-mode-2026-02-01";

/// How the model is asked to think: how it can ([`Thinks`]), how hard
/// the agent's spec says (`[model] thinking`), and within how many
/// tokens for a model that takes a budget (`thinking_budget`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Think {
    pub thinks: Thinks,
    pub level: Thinking,
    pub budget: u32,
}

impl Default for Think {
    fn default() -> Self {
        Self {
            thinks: Thinks::No,
            level: Thinking::Off,
            budget: 0,
        }
    }
}

/// A tool offered to the model, in the namespace's shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}
impl Tool {
    pub fn size(&self) -> usize {
        self.name.len() + self.description.len() + self.schema.to_string().len()
    }
}
/// An owned model/chat request. Unknown arguments are refused.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chat {
    #[serde(default)]
    pub name: Option<String>,
    pub system: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub no_calls: bool,
    pub answer: u32,
    pub window: u32,
    #[serde(default)]
    pub price: Option<crate::Price>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub above: Vec<crate::Jump>,
    #[serde(default)]
    pub thinking: Think,
    #[serde(default)]
    pub fast: bool,
    pub chat: Option<String>,
    #[serde(default)]
    pub spans: Vec<crate::context::Span>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub cache: Option<String>,
}
impl Chat {
    pub fn prompt(&self) -> Prompt<'_> {
        Prompt {
            system: &self.system,
            messages: &self.messages,
            tools: &self.tools,
            no_calls: self.no_calls,
            answer: self.answer,
            window: self.window,
            price: self.price,
            above: &self.above,
            thinking: self.thinking,
            fast: self.fast,
            chat: self.chat.as_deref(),
            spans: &self.spans,
            parent: self.parent.as_deref(),
            cache: self.cache.as_deref(),
        }
    }
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.answer > 0 && self.answer <= self.window,
            "answer must fit the model window"
        );
        for tool in &self.tools {
            anyhow::ensure!(
                (1..=128).contains(&tool.name.len())
                    && tool
                        .name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                    && tool.schema.is_object(),
                "a tool needs a plain name and an object schema"
            );
        }
        for message in &self.messages {
            anyhow::ensure!(
                matches!(message.role.as_str(), "user" | "assistant"),
                "unknown message role"
            );
            for part in message
                .parts
                .iter()
                .chain(message.answers.iter().flat_map(|answer| &answer.parts))
            {
                let checked = Media::new(&part.media, part.name.as_deref(), part.bytes.clone())
                    .map_err(anyhow::Error::msg)?;
                anyhow::ensure!(
                    checked.kind == part.kind,
                    "media kind differs from its MIME type"
                );
            }
        }
        Ok(())
    }
}
/// A completed model response, in every provider's same shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answered {
    pub text: String,
    pub calls: Vec<Call>,
    pub thinking: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub tokens: Tokens,
    pub latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    pub interrupted: bool,
}
/// A model/chat stream: text followed by one completed response, or an error.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Stream {
    Text { text: String },
    Answer { answer: Answered },
    Error { kind: Failure, message: String },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Unreached,
    TooLong,
    Overloaded,
    Refused,
}
