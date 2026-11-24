//! Typed answers of the agent, memory and task namespaces; web's are its
//! contract's (`link_web_tool_contract`), generated from its declarations.
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shown {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub window: Option<u32>,
    pub answer: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<link_harness_contract::harness::identity::Identity>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub source: String,
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remembered {
    pub notes: Vec<Note>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    pub namespaces: Vec<Namespace>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Namespace {
    pub name: String,
    pub actions: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Send {
    pub to: String,
    pub message: String,
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sent {
    pub sent: Vec<Recipient>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipient {
    pub to: String,
    pub id: String,
    /// For a teammate's message sent within tracked work: what became of
    /// it as a delegation of that work. None for one of no tracked work,
    /// and for a message to the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<link_interactions::runtime::ProposalState>,
    /// For a message to the owner's terminal (`cli`): kept on this
    /// computer for an open `lnk agent chat`, or the next one, rather than
    /// delivered to a service.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub held: bool,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub prompt: String,
    pub thread: String,
    pub cron: String,
    #[serde(default)]
    pub recurring: Option<bool>,
    #[serde(default)]
    pub answer: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scheduled {
    pub id: String,
    pub kind: String,
    pub thread: String,
    pub next: String,
    pub cron: Option<String>,
    /// What became of a subagent's start where its harness retains
    /// delegations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation: Option<serde_json::Value>,
    /// Kept, but something after went wrong: not to be made again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[cfg(feature = "action-client")]
pub mod client;
pub mod tasks;
