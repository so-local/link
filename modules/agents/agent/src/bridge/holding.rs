//! The messages a cloud that hosts agents held for one: kept, given once
//! the agent runs, or refused.

use super::*;
use link_format::WithinExt;

/// Where the messages a cloud that hosts agents held for agent `agent`
/// while it was paused there wait until they're given (`lnk agent
/// deliver`, and its bridge as it reaches its harness):
/// `~/.lnk/safe/agents/<agent>/held.jsonl`, one JSON line each, only
/// this user's to read, each line naming its schema ([`HELD`]).
pub fn held_file(agent: &str) -> PathBuf {
    settings::config_of(agent).join("held.jsonl")
}

/// A line of `held.jsonl`: a held message, as the cloud's contract
/// writes it, naming its schema.
pub static HELD: std::sync::LazyLock<link_files::schema::Format> = std::sync::LazyLock::new(|| {
    link_files::schema::Format::shipped(include_str!("holding/held.schema.toml"))
});

/// The held messages, which an upgrade converts.
pub fn saved_held() -> link_files::upgrade::Saved {
    link_files::upgrade::Saved::shipped::<link_agent_place::Held>(&HELD)
}

/// Where agent `agent` keeps its generation here: the ID of the move it
/// arrived by (`~/.lnk/safe/agents/<agent>/generation`), none for one
/// made here. Held messages are given only to the generation they name.
pub fn generation_file(agent: &str) -> PathBuf {
    settings::config_of(agent).join("generation")
}

/// Agent `agent`'s generation here, as [`generation_file`] keeps it.
pub fn generation(agent: &str) -> Option<String> {
    std::fs::read_to_string(generation_file(agent))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|id| link_agent_contract::moving::check_move_id(id).is_ok())
}

/// Agent `agent` arrived here by move `move_id`: its generation from now.
pub fn set_generation(agent: &str, move_id: &str) -> anyhow::Result<()> {
    link_agent_contract::moving::check_move_id(move_id)?;
    link_files::write_private(&generation_file(agent), move_id.as_bytes())
        .within("keeping the agent's generation")
}

/// Whether `held` is for the generation `here`: one naming none, from a
/// cloud that names none, is any generation's.
pub fn for_generation(here: Option<&str>, held: &link_agent_place::Held) -> bool {
    held.generation
        .as_deref()
        .is_none_or(|named| here == Some(named))
}

/// The held messages of agent `agent` locked for one reader and writer,
/// and read: each line that's a message.
fn held_locked(agent: &str) -> anyhow::Result<(std::fs::File, Vec<link_agent_place::Held>)> {
    let path = held_file(agent);
    let dir = path.parent().within("no folder for held messages")?;
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700)
        .create(dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("held.lock"))?;
    lock.lock()?;
    let kept = match std::fs::read_to_string(&path) {
        Ok(text) => text
            .lines()
            .filter_map(|line| HELD.read_json(line.as_bytes()).ok())
            .map(|read| read.spec)
            .collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => return Err(err).within("reading the held messages"),
    };
    Ok((lock, kept))
}

/// `held` written as agent `agent`'s held messages, or the file gone when
/// there are none.
fn held_write(agent: &str, held: &[link_agent_place::Held]) -> anyhow::Result<()> {
    let path = held_file(agent);
    if held.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        };
    }
    let mut text = Vec::new();
    for message in held {
        text.extend(HELD.print_json(message)?);
        text.push(b'\n');
    }
    link_files::write_private(&path, &text).within("keeping the held messages")
}

/// Keeps `messages` with agent `agent`'s held messages, before any is
/// given: given or not, none is lost.
pub fn keep_held(agent: &str, messages: &[link_agent_place::Held]) -> anyhow::Result<()> {
    let (_lock, mut kept) = held_locked(agent)?;
    kept.extend_from_slice(messages);
    held_write(agent, &kept)
}

/// Gives agent `agent`, carried as `plan` says, each of its held
/// messages ([`deliver_held`]): those given go, those refused are handed
/// to the caller (and kept too with `keep_refused`, for a caller with
/// nobody to show them), the rest stay for the next try.
pub async fn give_held(agent: &Settings, plan: &Plan, keep_refused: bool) -> anyhow::Result<Given> {
    let (_lock, kept) = held_locked(&agent.name)?;
    let mut given = Given::default();
    let mut waiting = Vec::new();
    for held in kept {
        if let Some(why) = refused(agent, plan, &held) {
            if keep_refused {
                waiting.push(held.clone());
            }
            given.refused.push((held, why));
            continue;
        }
        match deliver_held(agent, plan, &held).await {
            Ok(()) => given.given += 1,
            Err(_) => waiting.push(held),
        }
    }
    given.waiting = waiting.len();
    held_write(&agent.name, &waiting)?;
    Ok(given)
}

/// Gives agent `agent`'s held messages ([`give_held`]), again a while
/// after each try that leaves one it may still be given, until none is
/// left or `stop`.
pub(super) fn give_held_until(agent: &Settings, plan: &Plan, stop: &AtomicBool) {
    let mut wait = RETRY_FIRST;
    while !stop.load(Ordering::Relaxed) {
        match link_runtime::block_on(give_held(agent, plan, true)) {
            Ok(given) if given.waiting == given.refused.len() => {
                if !given.refused.is_empty() {
                    link_format::daemon::info!(
                        "{} held message(s) can't be given here: lnk agent deliver --agent {} \
                         shows them",
                        given.refused.len(),
                        agent.name
                    );
                }
                return;
            }
            Ok(_) => {}
            Err(err) => link_format::daemon::warn!("giving the held messages: {err:#}"),
        }
        if sleep_unless(wait, stop) {
            return;
        }
        wait = (wait * 2).min(RETRY_MOST);
    }
}

/// Why agent `agent`, carried as `plan` says, is never given `held`: a
/// message `lnk` doesn't give, a connection Link doesn't carry for it, or a
/// sender the connection isn't paired with (its `allowed_users`), as for
/// any message. None when it may be.
fn refused(agent: &Settings, plan: &Plan, held: &link_agent_place::Held) -> Option<String> {
    if let Err(err) = held.check() {
        return Some(format!("{err:#}"));
    }
    let connection = &held.connection;
    if !plan.connections.contains(connection) {
        return Some(format!(
            "{connection} isn't a connection Link carries for {}",
            agent.name
        ));
    }
    let Some(section) = section(agent, connection) else {
        return Some(format!("{} has no {connection} connected", agent.name));
    };
    if !paired(section.expose(), &held.from) {
        return Some(format!(
            "{} isn't who {} on {connection} is paired with",
            link_format::clean(&held.from),
            agent.name
        ));
    }
    None
}

/// Gives agent `agent`, carried as `plan` says, a message that came for
/// it while it was paused on a cloud that hosts agents: asked of its
/// harness as a message of its owner's in that chat, as the connection's
/// bridge would, and the answer sent there (`lnk-<connection> send`, which
/// sends only to the owner's own chat or a group they talk to it in).
/// Only when it isn't [`refused`].
pub async fn deliver_held(
    agent: &Settings,
    plan: &Plan,
    held: &link_agent_place::Held,
) -> anyhow::Result<()> {
    if let Some(why) = refused(agent, plan, held) {
        anyhow::bail!("{why}: not given");
    }
    let way_in = WayIn::new(agent, plan);
    let thread = format!("{}:{}", held.connection, held.chat);
    let (progress, _) = tokio::sync::watch::channel(String::new());
    let asked =
        link_connection_contract::ask_for(&way_in.endpoint, &thread, &held.text, &progress).await?;
    if let link_connection_contract::Asked::Answer(answer) = asked
        && !answer.trim().is_empty()
    {
        way_in.deliver(&thread, &answer)?;
    }
    Ok(())
}

/// Whether `from` is one a connection's settings `section` is paired with:
/// in its `allowed_users`, by number or by name.
pub(super) fn paired(section: &serde_json::Value, from: &str) -> bool {
    section["allowed_users"].as_array().is_some_and(|users| {
        users.iter().any(|user| match user {
            serde_json::Value::String(user) => user == from,
            serde_json::Value::Number(user) => user.to_string() == from,
            _ => false,
        })
    })
}
