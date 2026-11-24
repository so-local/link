//! What the tool boundary observed, before durable capture applies its
//! byte limits and stores media through the conversation's store.

use serde_json::Value;

#[derive(Clone, Copy)]
pub struct Dispatch<'a> {
    pub server: &'a str,
    pub operation: &'a str,
    pub arguments: &'a Value,
}

/// Called after local checks and session startup, just before a request
/// can leave. An error prevents dispatch.
pub trait Observer: Send + Sync {
    fn dispatching(&self, request: Dispatch<'_>) -> anyhow::Result<()>;
    /// Called at receipt/conversion, before optional catalogue refresh
    /// finishes. A recorder may consume media after storing its parts.
    fn received(&self, _answer: &mut super::Answered) -> anyhow::Result<()> {
        Ok(())
    }
}

pub(super) struct Unobserved;

impl Observer for Unobserved {
    fn dispatching(&self, _: Dispatch<'_>) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Observation {
    #[default]
    Refused,
    Reply(Value),
    RemoteError {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    Unknown,
}
