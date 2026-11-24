//! A conversation is a JSONL file of the model interface's messages, kept
//! whole, each line naming its schema ([`Line`]).

use crate::core::Message;
use anyhow::{Context, ensure};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// A line of a conversation: one message, written with
/// [`link_files::saved::line`], naming its schema, so a conversation
/// written across an upgrade reads each line by its own.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Line(Message);

link_files::specification!(Line, include_str!("conversation/line.schema.toml"));

/// Validate a session name before using it as a filename.
pub fn check_session(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && !matches!(name, "." | "..")
            && !name.contains(['/', '\\', '%'])
            && !name.chars().any(char::is_control),
        "invalid thread name"
    );
    Ok(())
}

/// Locate one conversation without letting its name select another directory.
pub fn conversation_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    check_session(name)?;
    Ok(root.join(format!("{}.jsonl", name.replace(':', "%3A"))))
}

/// List the saved sessions in a runtime's conversation directory.
pub fn list_sessions(root: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()?
                .strip_suffix(".jsonl")
                .map(|name| name.replace("%3A", ":"))
        })
        .collect();
    names.sort();
    names
}

/// Reload the exact messages saved by previous turns. Damaged records are errors.
pub fn load_conversation(path: &Path) -> anyhow::Result<Vec<Message>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    text.lines()
        .enumerate()
        .map(|(line, text)| {
            link_files::saved::parse_line::<Line>(text.as_bytes())
                .map(|Line(message)| message)
                .with_context(|| format!("reading conversation line {}", line + 1))
        })
        .collect()
}

/// Append one complete message privately before executing the next operation.
pub fn append_message(path: &Path, message: &Message) -> anyhow::Result<()> {
    let parent = path.parent().context("conversation has no directory")?;
    link_files::create_private_dir(parent)?;
    link_files::saved::append(path, &Line(message.clone()))
}

/// Refuse an unfinished tool turn instead of silently executing its calls again.
pub fn check_complete(messages: &[Message]) -> anyhow::Result<()> {
    let mut pending = BTreeSet::new();
    for message in messages {
        ensure!(
            matches!(message.role.as_str(), "user" | "assistant"),
            "unknown conversation role"
        );
        for answer in &message.answers {
            ensure!(
                pending.remove(&answer.call),
                "tool answer has no pending call"
            );
        }
        if !message.calls.is_empty() {
            ensure!(
                pending.is_empty(),
                "another model answer preceded its tool results"
            );
            for call in &message.calls {
                ensure!(
                    !call.id.is_empty() && pending.insert(call.id.clone()),
                    "tool call IDs must be unique and nonempty"
                );
            }
        } else if message.answers.is_empty() {
            ensure!(
                pending.is_empty(),
                "a message preceded pending tool results"
            );
        }
    }
    ensure!(
        pending.is_empty(),
        "this conversation has unanswered tool calls; start a new conversation rather than replaying them"
    );
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/runtime/conversation.rs"]
mod tests;
