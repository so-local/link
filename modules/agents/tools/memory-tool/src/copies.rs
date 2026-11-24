//! The copy for people of a conversation kept without one on disk: kept
//! compact (`events.zst`, [`link_store_contract::store::compact`]) or by a store
//! driver (an empty `store.<name>`, [`link_store_contract::store`]), as the
//! agent's spec said (`[storage] conversations`). Memory makes its own,
//! from the events, the copy a log's `conversation.md` holds
//! ([`link_store_contract::store::copy`]), and indexes it as that file.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use link_store_contract::store::{self, Agent, Driver, Store as _, compact, copy};
use serde_json::Value;

use crate::folder;

/// The copy's name in its conversation's folder, as a log's is.
pub const COPY: &str = "conversation.md";

/// The most its tail is read to: as far as its store lets it grow, its
/// folds lost.
const TAIL_MAX: u64 = compact::TAIL_MAX;

/// How a conversation without a copy is kept.
#[derive(Clone, Debug, PartialEq)]
pub enum Kept {
    Compact,
    /// By the driver of this name, `lnk-<name>`.
    Driver(String),
    /// By its place's driver, through the agent's storage bridge
    /// ([`crate::tree::Stored`]).
    Stored,
}

/// How the conversation whose folder holds `names` is kept, when it has
/// no copy of its own: none when it has one, or isn't kept either way.
pub fn kept(names: &[&str]) -> Option<Kept> {
    if names.contains(&COPY) {
        return None;
    }
    let driver = names.iter().find_map(|name| {
        name.strip_prefix("store.")
            .filter(|d| store::check_name(d).is_ok())
    });
    match driver {
        Some(name) => Some(Kept::Driver(name.to_string())),
        None if names.contains(&compact::FRAMES) => Some(Kept::Compact),
        None => None,
    }
}

/// A conversation's copy, or what it grew by: its markdown, and the
/// number of the last event it holds, where the next read starts after.
#[derive(Debug, PartialEq)]
pub struct Copy {
    pub text: String,
    pub last: u64,
}

/// `events`, every event after `after` and maybe one more, as the copy
/// reads them from there: none when they don't start where the copy
/// left off (the conversation shrank or was written again), so it's
/// read whole. With `after` 0, the copy whole, headed with `thread`.
pub fn since(thread: &str, events: &[Value], after: u64) -> Option<Copy> {
    let events = match after {
        0 => events,
        _ => {
            let (first, rest) = events.split_first()?;
            (store::seq_of(first) == Some(after)).then_some(rest)?
        }
    };
    let said: Vec<copy::Said> = events.iter().map(copy::Said::of).collect();
    // Up to the agent's last answer: what follows is read next time.
    let answered = said.iter().rposition(copy::Said::is_answer);
    let mut text = match after {
        0 => copy::header(thread),
        _ => String::new(),
    };
    let mut last = after;
    if let Some(end) = answered {
        for said in &said[..=end] {
            text += &copy::section(said).unwrap_or_default();
        }
        last = store::seq_of(&events[end]).unwrap_or(after);
    }
    Some(Copy { text, last })
}

/// The copy of the compact conversation in `rel` (`chats/<x>`), read
/// through `root`, from after event `after` ([`since`]): its frames
/// decoded only back to the one holding it
/// ([`compact::read_from`]). None when its frames are gone, or it
/// doesn't go on from there. Frames larger than `frames-max-bytes`
/// (`limits.toml`) aren't read.
pub fn compact(root: &folder::Dir, rel: &str, after: u64) -> Result<Option<Copy>> {
    let frames_at = format!("{rel}/{}", compact::FRAMES);
    let most = crate::limits::limits().frames_max_bytes;
    let frames = match root.read(&frames_at, most)? {
        Some(frames) => frames,
        None if root.read_tail(&frames_at, 0)?.is_some() => {
            anyhow::bail!("its frames are past {} MiB", most >> 20)
        }
        None => return Ok(None),
    };
    let tail = root
        .read(&format!("{rel}/{}", compact::TAIL), TAIL_MAX)?
        .unwrap_or_default();
    let events = compact::read_from(&frames, &tail, after.max(1))?;
    let thread = compact::thread(&frames).unwrap_or_else(|| folder_name(rel));
    Ok(since(&thread, &events, after))
}

/// What the drivers keep of an agent's conversations: each driver's
/// threads, by their folder's name, asked once a walk.
pub struct Drivers {
    /// The agent's `chats/`, where a driver on this machine keeps
    /// what it keeps.
    dir: std::path::PathBuf,
    whose: Agent,
    threads: BTreeMap<String, Result<BTreeMap<String, store::Kept>, String>>,
}

impl Drivers {
    pub fn new(dir: std::path::PathBuf, whose: Agent) -> Self {
        Self {
            dir,
            whose,
            threads: BTreeMap::new(),
        }
    }

    /// What driver `name` keeps of the conversation in folder `rel`
    /// (`chats/<x>`): its thread and last number.
    pub fn kept(&mut self, name: &str, rel: &str) -> Result<Option<store::Kept>> {
        let (dir, whose) = (&self.dir, &self.whose);
        let threads = self.threads.entry(name.to_string()).or_insert_with(|| {
            let driver = Driver::find(name, dir).map_err(|e| format!("{e:#}"))?;
            let threads = driver.threads(whose).map_err(|e| format!("{e:#}"))?;
            Ok(threads
                .into_iter()
                .map(|kept| (kept.thread.replace(':', "-"), kept))
                .collect())
        });
        match threads {
            Ok(threads) => Ok(threads.get(&folder_name(rel)).cloned()),
            Err(err) => anyhow::bail!("{err}"),
        }
    }

    /// The copy of `thread`, kept by driver `name`, from after event
    /// `after` ([`since`]).
    pub fn copy(&self, name: &str, thread: &str, after: u64) -> Result<Option<Copy>> {
        let driver = Driver::find(name, &self.dir)?;
        let events = driver
            .read(&self.whose, thread, after.max(1))
            .with_context(|| format!("reading {thread} from lnk-{name}"))?;
        Ok(since(thread, &events, after))
    }
}

/// The last part of `rel`.
fn folder_name(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64, role: &str, text: &str) -> Value {
        serde_json::json!({"seq": seq, "role": role, "text": text, "at": "2026-09-29T14:03:07Z"})
    }

    /// A copy read whole, then from where it left off, is the copy read
    /// whole after: up to the agent's last answer each time.
    #[test]
    fn a_copy_goes_on_from_its_last_event() {
        let events = [
            event(1, "user", "Hello"),
            event(2, "assistant", "Hi."),
            event(3, "user", "Plant the beans"),
            event(4, "assistant", "Noted."),
            event(5, "user", "And the peas"),
        ];
        let first = since("t", &events[..3], 0).unwrap();
        assert_eq!(first.last, 2);
        let grown = since("t", &events[1..], first.last).unwrap();
        assert_eq!(grown.last, 4);
        let whole = since("t", &events, 0).unwrap();
        assert_eq!(first.text + &grown.text, whole.text);
        // Not where it left off: read whole.
        assert!(since("t", &events[2..], 2).is_none());
        assert!(since("t", &[], 2).is_none());
    }

    /// A compact conversation's tail past a frame's worth and an event,
    /// its folds lost, is read as its store reads it: its events in the
    /// copy, never left out.
    #[test]
    fn a_tail_whose_folds_were_lost_is_read() {
        let dir = link_test_kit::TempDir::new("memory-lost-folds");
        std::fs::create_dir_all(dir.join("chats/t")).unwrap();
        std::fs::write(
            dir.join("chats/t").join(compact::FRAMES),
            compact::named("t"),
        )
        .unwrap();
        let words = "w".repeat(60_000);
        let mut tail = Vec::new();
        let mut seq = 0;
        while tail.len() as u64 <= compact::FRAME_BYTES + store::EVENT_MAX + 1 || seq % 2 == 1 {
            seq += 1;
            let role = ["assistant", "user"][seq as usize % 2];
            tail.extend(
                format!("{{\"seq\":{seq},\"role\":\"{role}\",\"text\":\"{words}\"}}\n").bytes(),
            );
        }
        std::fs::write(dir.join("chats/t").join(compact::TAIL), tail).unwrap();
        let root = folder::Dir::open(&dir).unwrap();
        let copy = compact(&root, "chats/t", seq - 2)
            .unwrap()
            .expect("its copy");
        assert_eq!(copy.last, seq);
    }

    #[test]
    fn only_a_conversation_without_a_copy_needs_one() {
        assert_eq!(kept(&["events.jsonl", COPY]), None);
        assert_eq!(kept(&["events.jsonl"]), None);
        assert_eq!(kept(&["events.zst", "events.jsonl"]), Some(Kept::Compact));
        assert_eq!(
            kept(&["store.sqlite", "tools.json"]),
            Some(Kept::Driver("sqlite".into()))
        );
        assert_eq!(kept(&["store.Bad/x"]), None);
    }
}
