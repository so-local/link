//! Each agent's model calls, part by part ([`link_model_contract::context`]):
//! a chat's in a file of its own in Link's settings
//! (`model/context/<agent ID>/<chat>.jsonl`, 600), a [`Line`] a call,
//! names and numbers only, each naming its schema. A chat's file keeps at most [`CHAT_BYTES`],
//! its oldest half dropped past it; all go with the agent.
//!
//! A chat is named by its caller: Link Harness's thread, given with
//! its request. An outside harness's is its opening message's
//! fingerprint (`opening-<hex>`), the one thing every call of a chat
//! shares, until compaction rewrites it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use link_model_contract::chat::Tokens;
use link_model_contract::context::{self, Api, Call, Kept, Part};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most a chat's file holds before its oldest half is dropped.
pub const CHAT_BYTES: u64 = 4 << 20;

/// The longest a chat's name is kept: past it, or not plain, its file
/// is named by its fingerprint.
const NAME_MAX: usize = 64;

/// One call, as a chat's file holds it: a line naming its schema.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    chat: String,
    call: Call,
}

link_files::specification!(Line, include_str!("context/line.schema.toml"));

/// What a call asked, taken from its request before it's sent: its chat
/// and its parts.
#[derive(Debug)]
pub struct Asked {
    pub chat: String,
    pub model: Option<String>,
    pub window: Option<u64>,
    pub price: Option<link_model_contract::Price>,
    pub above: Vec<link_model_contract::Jump>,
    /// The chat that asked for its chat's work, where its caller said.
    pub parent: Option<String>,
    pub parts: Vec<Part>,
}

impl Asked {
    /// What `body`, a request in `api`, asks, in the chat `given` with
    /// it, else its opening message's; none with neither.
    pub fn of(api: Api, body: &Value, given: Option<&str>) -> Option<Asked> {
        let model = body["model"].as_str().map(str::to_owned);
        // The price Link knows of it now, as it's asked.
        let facts = model
            .as_deref()
            .and_then(|name| link_model_contract::known(name).ok().flatten())
            .map(|known| known.facts)
            .unwrap_or_default();
        Some(Asked {
            chat: chat(given, context::opening(api, body))?,
            model,
            window: None,
            price: facts.price,
            above: facts.above,
            parent: None,
            parts: context::census(api, body),
        })
    }

    /// Kept as `agent`'s call in Link's `settings`, with what its
    /// answer measured: none when it gave no usage.
    pub fn keep(self, settings: &Path, agent: &str, tokens: Option<Tokens>) -> anyhow::Result<()> {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let call = Call {
            seq: 0,
            at,
            model: self.model,
            window: self.window,
            tokens,
            price: self.price,
            above: self.above,
            parent: self.parent,
            parts: self.parts,
        };
        add(settings, agent, &self.chat, call)
    }
}

/// The folder of `agent`'s calls in Link's `settings`.
pub fn dir(settings: &Path, agent: &str) -> anyhow::Result<PathBuf> {
    context::dir(settings, agent)
}

/// A chat's name, as its caller gave it, else the fingerprint of the
/// request's opening message; none when there's neither.
pub fn chat(given: Option<&str>, opening: Option<u64>) -> Option<String> {
    match given.filter(|g| !g.is_empty()) {
        Some(given) => Some(given.chars().take(256).collect()),
        None => opening.map(|o| format!("opening-{o:016x}")),
    }
}

/// The file `chat`'s calls are in, under `dir`.
fn file(dir: &Path, chat: &str) -> PathBuf {
    let plain = chat.len() <= NAME_MAX
        && !chat.starts_with('.')
        && chat
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b));
    let name = match plain {
        true => chat.to_owned(),
        false => format!("chat-{:016x}", context::fingerprint(chat.as_bytes())),
    };
    dir.join(format!("{name}.jsonl"))
}

/// Keeps `call` as `chat`'s next, `agent`'s, in Link's `settings`: its
/// `seq` the one after the chat's last. Every agent's calls are kept
/// under one lock, so two bridges' are both kept.
pub fn add(settings: &Path, agent: &str, chat: &str, mut call: Call) -> anyhow::Result<()> {
    let dir = dir(settings, agent)?;
    let _locked = link_files::lock_file(&dir.join(".lock"))?;
    let path = file(&dir, chat);
    call.seq = last(&path).map_or(1, |l| l.call.seq + 1);
    let line = link_files::saved::line(&Line {
        chat: chat.to_owned(),
        call,
    })?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    // Its folder is this user's alone ([`link_files::lock_file`]).
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    out.write_all(&line)?;
    if out.metadata()?.len() > CHAT_BYTES {
        halve(&path)?;
    }
    Ok(())
}

/// How much of a chat's file's end is read for its last call: more than
/// a call of the most parts takes, else the whole file is.
const TAIL: u64 = 128 << 10;

/// A chat's last call, from its file's end.
fn last(path: &Path) -> Option<Line> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut from = len.saturating_sub(TAIL);
    loop {
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail).ok()?;
        let found = tail
            .split(|&b| b == b'\n')
            .rev()
            .find_map(|line| link_files::saved::parse_line(line).ok());
        if found.is_some() || from == 0 {
            return found;
        }
        from = 0;
    }
}

/// Drops the oldest half of a chat's calls.
fn halve(path: &Path) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)?;
    let lines: Vec<&[u8]> = bytes
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .collect();
    let mut kept = lines[lines.len() / 2..].join(&b'\n');
    kept.push(b'\n');
    link_files::write_private(path, &kept).with_context(|| format!("writing {}", path.display()))
}

/// A chat's kept calls, oldest first.
fn calls(path: &Path) -> Option<(String, Vec<Call>)> {
    let bytes = std::fs::read(path).ok()?;
    let mut chat = None;
    let calls = bytes
        .split(|&b| b == b'\n')
        .filter_map(|line| link_files::saved::parse_line::<Line>(line).ok())
        .map(|line| {
            chat.get_or_insert(line.chat);
            line.call
        })
        .collect();
    Some((chat?, calls))
}

/// What `agent`'s calls report, in Link's `settings`: the chats kept,
/// the latest first, and the report of `chat`, else of the latest a
/// person had, each with the work it asked for ([`with_work`]).
pub fn read(
    settings: &Path,
    agent: &str,
    chat: Option<&str>,
) -> anyhow::Result<link_model_contract::api::Context> {
    let dir = dir(settings, agent)?;
    // Each with when its file was last written, finer than a call's time.
    let mut chats = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        let Some(latest) = last(&path) else {
            continue;
        };
        let written = entry.metadata().and_then(|m| m.modified()).ok();
        let kept = Kept {
            chat: latest.chat,
            calls: latest.call.seq,
            at: latest.call.at,
            parent: latest.call.parent,
        };
        chats.push((written, kept));
    }
    chats.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| x.chat.cmp(&y.chat)));
    let chats: Vec<Kept> = chats.into_iter().map(|(_, kept)| kept).collect();
    let named = match chat {
        Some(chat) => match chats.iter().find(|k| k.chat == chat) {
            Some(kept) => Some(kept.chat.clone()),
            None => anyhow::bail!("no calls are kept for chat {:?}", link_format::clean(chat)),
        },
        // The latest a person had: not a subagent's, nor a compaction's.
        None => chats
            .iter()
            .find(|k| k.parent.is_none())
            .or(chats.first())
            .map(|k| k.chat.clone()),
    };
    let report = named.and_then(|name| with_work(&dir, &chats, &name, 0));
    Ok(link_model_contract::api::Context {
        chats,
        chat: report,
    })
}

/// The deepest chats asking chats are followed.
const DEPTH: usize = 8;

/// `name`'s report, with the work each chat it asked for (its
/// compaction, its goals, its subagents, and theirs) counted in it.
fn with_work(dir: &Path, chats: &[Kept], name: &str, depth: usize) -> Option<context::Report> {
    let (name, calls) = calls(&file(dir, name))?;
    let mut report = context::report(&name, &calls)?;
    if depth < DEPTH {
        for child in chats
            .iter()
            .filter(|k| k.parent.as_deref() == Some(name.as_str()))
        {
            if child.chat == name {
                continue;
            }
            if let Some(work) = with_work(dir, chats, &child.chat, depth + 1) {
                report.adopt(&context::adopted_under(&name, &child.chat), &work);
            }
        }
    }
    Some(report)
}

#[cfg(test)]
mod tests;
