//! Conversation storage: one JSON message per line, plus an optional tool
//! allowlist beside each thread. ACP narrows that list before a prompt;
//! turns read and append messages; ASP exports and imports the same files.

use std::path::PathBuf;

use anyhow::Context;

/// Saved sessions, in the harness home's conversation directory.
pub(crate) fn names() -> Vec<String> {
    crate::runtime::conversation::list_sessions(&root())
}

pub(crate) use crate::runtime::conversation::check_session as check;

fn root() -> PathBuf {
    link_files::home_dir().join("threads")
}

pub(crate) fn thread_file(thread: &str) -> anyhow::Result<PathBuf> {
    crate::runtime::conversation::conversation_path(&root(), thread)
}

/// The allowlist beside the conversation, separate from its messages.
fn tools_file(thread: &str) -> PathBuf {
    thread_file(thread)
        .expect("checked thread name")
        .with_extension("tools.json")
}

/// No saved allowlist means every tool; an empty list means none. Each
/// is named as it is today ([`link_spec::follow::replayed`]), so a tool
/// renamed since stays allowed by its new name and grants no other; the
/// file is never rewritten by reading it.
pub(crate) fn tools(thread: &str) -> anyhow::Result<Option<Vec<String>>> {
    check(thread)?;
    Ok(read_tools(&tools_file(thread))?.map(|tools| named(tools, &link_spec::follow::replayed)))
}

/// `tools`, each named by `named`.
fn named(tools: Vec<String>, named: &dyn Fn(&str) -> String) -> Vec<String> {
    tools.iter().map(|tool| named(tool)).collect()
}

/// The tools a thread is narrowed to, as its `.tools.json` keeps them,
/// naming its schema.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowed {
    tools: Vec<String>,
}

link_files::specification!(Allowed, include_str!("threads.schema.toml"));

/// Each thread's tools, which an upgrade converts.
pub fn saved() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::of::<Allowed>()
}

fn read_tools(path: &std::path::Path) -> anyhow::Result<Option<Vec<String>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(
            link_files::saved::parse_json::<Allowed>(&bytes)
                .context("reading the thread's tool restrictions")?
                .tools,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// Intersect the requested tools with the saved allowlist, never widening it.
pub(crate) fn narrow(thread: &str, requested: Vec<String>) -> anyhow::Result<()> {
    let allowed: Vec<String> = match tools(thread)? {
        Some(previous) => requested
            .into_iter()
            .filter(|tool| previous.contains(tool))
            .collect(),
        None => requested,
    };
    let path = tools_file(thread);
    std::fs::create_dir_all(path.parent().context("no folder")?)?;
    link_files::write_private(
        &path,
        &link_files::saved::print_json(&Allowed { tools: allowed })?,
    )
    .with_context(|| format!("writing {}", path.display()))
}

pub(crate) fn read(thread: &str) -> anyhow::Result<Vec<crate::core::Message>> {
    crate::runtime::conversation::load_conversation(&thread_file(thread)?)
}

/// `messages`, a conversation as recorded, as its model is given it:
/// each call named by `named`, as its tool and command are today
/// ([`link_spec::follow::replayed`]). The record itself is never
/// rewritten, and a call since removed keeps its name.
pub(crate) fn replayed(
    mut messages: Vec<crate::core::Message>,
    named: &dyn Fn(&str) -> String,
) -> Vec<crate::core::Message> {
    for call in messages.iter_mut().flat_map(|m| m.calls.iter_mut()) {
        call.name = named(&call.name);
    }
    messages
}

pub(crate) fn append_message(thread: &str, message: &crate::core::Message) -> anyhow::Result<()> {
    crate::runtime::conversation::append_message(&thread_file(thread)?, message)
}

/// Import portable text through the same complete-message recorder.
pub(crate) fn append(thread: &str, role: &str, text: &str) -> anyhow::Result<()> {
    append_message(
        thread,
        &crate::core::Message {
            role: role.into(),
            text: text.into(),
            ..crate::core::Message::default()
        },
    )
}

#[cfg(test)]
#[path = "../../tests/link/threads.rs"]
mod tests;
