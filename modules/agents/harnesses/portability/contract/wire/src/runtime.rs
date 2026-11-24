//! How an agent's host hands a harness a message and hears its answer,
//! in the same process, through direct Rust calls. ACP and ASP translate
//! it at their own boundaries.

use std::future::Future;

pub use link_interactions_contract::{ScheduleId, ScheduledOccurrence, TransportEventId};
pub use link_model_contract::chat::media::Media;
use serde::{Deserialize, Serialize};

/// An agent, by what never changes: its owner and its ID. Two agents
/// are the same only when both are, since every owner has a `main`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentKey {
    pub owner: String,
    pub id: String,
}

/// One conversation of one agent: a thread, such as `link:main`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conversation {
    pub agent: AgentKey,
    pub thread: String,
}

/// What a message sent while a turn runs does, declared in the wire's
/// `types.toml`.
pub use crate::declared::Busy;

/// Who a message is from, as the host authenticated it. A message's
/// text never establishes this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sender {
    /// The agent's owner.
    Owner,
    /// Anyone or anything else, and the transport event that carried
    /// it, by which a redelivery is recognized.
    Other {
        from: Origin,
        delivery: Option<TransportEventId>,
    },
}

/// A sender other than the owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Someone in a chat the agent is shared with: `principal` on its
    /// connection, and the name they go by there, which is words for
    /// the model and never who they are.
    Person {
        principal: String,
        name: Option<String>,
    },
    /// A schedule, and the occurrence it fired for.
    Schedule {
        schedule: ScheduleId,
        occurrence: Option<ScheduledOccurrence>,
    },
    /// A task's result.
    Task(String),
    /// Another agent.
    Agent(String),
    /// A background run's result.
    Run(String),
    /// A webhook, by its name.
    Webhook(String),
}

/// A message for a harness to answer.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub conversation: Conversation,
    pub text: String,
    /// Its images and documents.
    pub media: Vec<Media>,
    pub sender: Sender,
    /// How many messages, each sent by an agent on its own, led to it.
    pub hops: u32,
    /// The tools its conversation may use from now on, of those the
    /// agent may: none narrows nothing.
    pub tools: Option<Vec<String>>,
    /// The model its conversation answers with from now on, of those
    /// the agent may use.
    pub model: Option<String>,
    /// What it does if a turn is running, in place of the agent's spec.
    pub busy: Option<Busy>,
    /// Whether its answer is kept until the host says it's delivered
    /// ([`Runtime::delivered`]), so a lost connection can't lose it.
    pub receipt: bool,
}

/// What a harness tells its host while it answers, borrowed for the
/// call, so streaming copies nothing it doesn't need to.
#[derive(Clone, Copy, Debug)]
pub enum Streamed<'a> {
    /// The message is sure to be answered in a turn of its own, not
    /// steered into another.
    Taken,
    /// A piece of the answer's text, as the model writes it.
    Text(&'a str),
    /// The model called a tool.
    Called { call: &'a str, tool: &'a str },
    /// A tool answered a call.
    Answered { call: &'a str, failed: bool },
    /// The final answer, recorded: the host may deliver it now, while
    /// the turn still finishes its work.
    Final(&'a Answer),
}

/// Where a harness sends what it [`Streamed`]. Called inline, so an
/// implementation hands each on without blocking.
pub trait Listener: Send + Sync {
    fn heard(&self, streamed: Streamed<'_>);
}

/// A final answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    /// Cut short: the turn was asked to end while it was written.
    pub interrupted: bool,
    /// Where it's kept until delivered, when the prompt asked.
    pub receipt: Option<Receipt>,
}

/// Where an answer kept for its delivery is: its conversation's thread
/// and its place in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub thread: String,
    pub seq: u64,
}

/// What came of a prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Answered in a turn of its own.
    Answered(Answer),
    /// It joined a turn already running, whose answer answers it too.
    Steered,
}

/// An answer kept for delivery that its host never confirmed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovered {
    pub thread: String,
    pub text: String,
    /// The receipt the host confirms it by.
    pub id: String,
}

/// A harness, as its agent's host calls it. A host serves many
/// conversations at once, so every method takes `&self`.
pub trait Runtime: Send + Sync {
    /// Answers `prompt`, telling `listener` as it goes.
    fn prompt<L: Listener>(
        &self,
        prompt: Prompt,
        listener: &L,
    ) -> impl Future<Output = anyhow::Result<Outcome>> + Send;

    /// Asks the turn running in `conversation`, if one is, to end.
    fn cancel(&self, conversation: &Conversation) -> anyhow::Result<()>;

    /// The answers kept for `agent` that were never confirmed delivered.
    fn recover(
        &self,
        agent: &AgentKey,
    ) -> impl Future<Output = anyhow::Result<Vec<Recovered>>> + Send;

    /// Confirms answers delivered, by their [`Recovered::id`].
    fn delivered(&self, agent: &AgentKey, ids: &[String]) -> anyhow::Result<()>;
}
