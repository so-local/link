//! Model actions other than chat: closed requests and answers.
use crate::chat::{Media, Tokens};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub ready: bool,
    pub loaded: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Embed {
    #[serde(default)]
    pub name: Option<String>,
    pub texts: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Embedded {
    pub vectors: Vec<Vec<f64>>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transcribe {
    pub name: String,
    pub audio: Media,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heard {
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Count {
    #[serde(default)]
    pub name: Option<String>,
    pub text: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counted {
    pub tokens: u64,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub requests: u64,
    pub unmetered: u64,
    pub tokens: Tokens,
}
/// Which chat's calls to report: the latest without one.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOf {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
}
/// What the calling agent's model calls are spent on
/// ([`crate::context`]): the chats kept, the latest first, and the
/// report of the chat asked, else of the latest.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub chats: Vec<crate::context::Kept>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<crate::context::Report>,
}
