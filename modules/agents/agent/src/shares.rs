//! The chats an agent is shared with: its spec's `[access] message`,
//! which `lnk agent share add` and `remove` write, the only part of a spec
//! Link writes ([`link_agent_spec::with_message_access`]), and
//! `lnk agent share list` reads. A chat's name, for people, is kept beside it
//! (`shares.json`), never who may talk to the agent: that's the spec's.

use link_format::WithinExt;
use std::collections::BTreeMap;
use std::path::PathBuf;

use link_agent_identity::access::Chat;
use serde::{Deserialize, Serialize};

use crate::settings::{self, Settings};

/// A chat the agent is shared with, as `lnk agent shares --json` lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Share {
    pub connection: String,
    /// Its id, as the connection names it.
    pub chat: String,
    /// Its name, where the connection gave one when it was shared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Share {
    /// How people read it: `#team (C0TEAM)`, or the id alone.
    pub fn said(&self) -> String {
        match &self.name {
            Some(name) => format!(
                "{} ({})",
                link_format::clean(name),
                link_format::clean(&self.chat)
            ),
            None => link_format::clean(&self.chat),
        }
    }
}

/// The names of the chats shared, by chat (`chat:slack:C0TEAM`).
fn names_path(agent: &str) -> PathBuf {
    settings::config_of(agent).join("shares.json")
}

/// The names kept of the chats shared, by chat, as `shares.json` keeps
/// them, naming its schema.
#[derive(Default, Serialize, Deserialize)]
#[serde(transparent)]
struct Names(BTreeMap<String, String>);

link_files::specification!(Names, include_str!("shares.schema.toml"));

fn names(agent: &str) -> BTreeMap<String, String> {
    std::fs::read(names_path(agent))
        .ok()
        .and_then(|bytes| link_files::saved::parse_json::<Names>(&bytes).ok())
        .unwrap_or_default()
        .0
}

/// The agent's own spec's text, read only when it's written by this user
/// alone, and never through a link: none when it has none.
fn own_text(agent: &str) -> anyhow::Result<Option<String>> {
    let path = Settings::path_of(agent);
    if !path.exists() {
        return Ok(None);
    }
    // SAFETY: getuid has no preconditions and can't fail.
    let uid = unsafe { libc::getuid() };
    let read = link_files::fs::read_trusted(
        &path,
        link_files::fs::Trust {
            owners: &[uid],
            deny_mode: 0o022,
            ancestors: false,
            no_link: true,
        },
        link_spec::MAX_BYTES,
        "a spec's",
    )?;
    settings::configuration::behavior_text(&read.text)
}

/// The chats agent `agent` is shared with, in its spec's order, each with
/// its name where one was kept.
pub fn list(agent: &str) -> anyhow::Result<Vec<Share>> {
    listed(agent, own_text(agent)?.as_deref())
}

/// The chats spec `text` of agent `agent` shares it with.
fn listed(agent: &str, text: Option<&str>) -> anyhow::Result<Vec<Share>> {
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    let spec = link_agent_spec::parse(text)
        .within(format!("in {}", Settings::path_of(agent).display()))?;
    let names = names(agent);
    Ok(spec
        .message_access()
        .message
        .iter()
        .map(|chat| Share {
            connection: chat.connection().to_string(),
            chat: chat.id().to_string(),
            name: names.get(chat.as_str()).cloned(),
        })
        .collect())
}

/// The chats of a spec's own `[access] message`.
fn chats_of(text: Option<&str>) -> anyhow::Result<Vec<Chat>> {
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    Ok(link_agent_spec::parse(text)?.message_access().message)
}

/// Writes agent `agent`'s spec, `read` when it was read, with `chats` as
/// its `[access] message`, whole or not at all (written aside, then
/// renamed into place, this user's alone), and keeps `named` beside it.
/// Refused when the spec changed since it was read: its owner's edit is
/// never written over.
fn write(
    agent: &str,
    read: Option<&str>,
    chats: &[Chat],
    named: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    // An agent with no spec of its own starts from an empty one, which
    // names its schema as every spec does.
    let empty = link_spec::print_spec(&link_agent_spec::Spec::default())?;
    let written = link_agent_spec::with_message_access(read.unwrap_or(&empty), chats)?;
    anyhow::ensure!(
        own_text(agent)?.as_deref() == read,
        "{} changed meanwhile: run it again",
        Settings::path_of(agent).display()
    );
    link_files::create_private_dir(&settings::config_of(agent))?;
    settings::configuration::write_behavior(&Settings::load_without_keys(agent)?, &written)?;
    let kept = Names(
        named
            .iter()
            .filter(|(chat, _)| chats.iter().any(|c| c.as_str() == chat.as_str()))
            .map(|(chat, name)| (chat.clone(), name.clone()))
            .collect(),
    );
    link_files::write_private(&names_path(agent), &link_files::saved::print_json(&kept)?)?;
    Ok(())
}

/// Adds chat `shared` of `connection` to agent `settings`'s `[access]
/// message`, its name kept: its spec checked whole after, as a start
/// would, or nothing written.
pub fn add(
    settings: &Settings,
    connection: &str,
    shared: &link_connection_contract::Shared,
) -> anyhow::Result<Share> {
    let shared_chat = shared.chat.clone();
    anyhow::ensure!(
        shared_chat.connection() == connection,
        "lnk-{connection} shared {}, not a chat on {connection}",
        link_format::clean(shared_chat.as_str())
    );
    let (on, chat) = (shared_chat.connection(), shared_chat.id());
    let read = own_text(&settings.name)?;
    let mut chats = chats_of(read.as_deref())?;
    if !chats.contains(&shared_chat) {
        chats.push(shared_chat.clone());
    }
    let mut named = names(&settings.name);
    if let Some(name) = shared.name.as_ref().filter(|n| !n.trim().is_empty()) {
        named.insert(shared_chat.to_string(), name.clone());
    }
    checked(settings, || {
        write(&settings.name, read.as_deref(), &chats, &named)
    })?;
    Ok(Share {
        connection: on.to_string(),
        chat: chat.to_string(),
        name: named.get(shared_chat.as_str()).cloned(),
    })
}

/// Takes the chat of `connection` named `chat`, by its id or its name, out
/// of agent `settings`'s `[access] message`: the share it was, or none
/// when it wasn't shared.
pub fn remove(settings: &Settings, connection: &str, chat: &str) -> anyhow::Result<Option<Share>> {
    let read = own_text(&settings.name)?;
    let shares = listed(&settings.name, read.as_deref())?;
    let wanted = chat.trim_start_matches('#');
    let Some(share) = shares.into_iter().find(|s| {
        s.connection == connection
            && (s.chat == chat
                || s.name
                    .as_deref()
                    .is_some_and(|n| n == chat || n.trim_start_matches('#') == wanted))
    }) else {
        return Ok(None);
    };
    let gone = link_agent_identity::access::chat(connection, &share.chat)?;
    let chats: Vec<Chat> = chats_of(read.as_deref())?
        .into_iter()
        .filter(|c| *c != gone)
        .collect();
    checked(settings, || {
        write(
            &settings.name,
            read.as_deref(),
            &chats,
            &names(&settings.name),
        )
    })?;
    Ok(Some(share))
}

/// Runs `change` on agent `settings`'s spec, `read` as it was read, then
/// checks the spec it runs whole: one that no longer resolves gets its
/// file back as it was read.
fn checked(settings: &Settings, change: impl FnOnce() -> anyhow::Result<()>) -> anyhow::Result<()> {
    let spec = Settings::path_of(&settings.name);
    let names = names_path(&settings.name);
    // The spec as it was read, trusted; the names only where they're a
    // file of their own, never through a link.
    let kept = std::fs::symlink_metadata(&names)
        .is_ok_and(|meta| meta.is_file())
        .then(|| std::fs::read(&names).ok())
        .flatten();
    let before = (std::fs::read(&spec).ok(), kept);
    let restore = || -> anyhow::Result<()> {
        for (path, bytes) in [(&spec, &before.0), (&names, &before.1)] {
            match bytes {
                Some(bytes) => link_files::write_private(path, bytes)?,
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        Ok(())
    };
    if let Err(err) = change().and_then(|()| settings.spec().map(|_| ())) {
        restore()?;
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_said_by_its_name_and_id() {
        let share = Share {
            connection: "slack".into(),
            chat: "C0TEAM".into(),
            name: Some("#team".into()),
        };
        assert_eq!(share.said(), "#team (C0TEAM)");
        let bare = Share {
            name: None,
            ..share
        };
        assert_eq!(bare.said(), "C0TEAM");
    }
}
