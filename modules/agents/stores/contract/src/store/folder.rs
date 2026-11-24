//! A conversation as a folder of files: how Link Harness keeps one on
//! `file` (`chats/<folder>/`), and how a move carries one
//! (`files/chats/<folder>/` in its stream, [`crate::moving`]).
//! Every end that turns a conversation into files or files into a
//! conversation does it here, so a move out of a store and back into a
//! folder, or the other way, carries the same things:
//!
//! - [`EVENTS`], its events, a line of JSON each, in order;
//! - [`MARKDOWN`], a copy for people, whose first line names its thread
//!   (`# telegram:42`), derived from the events ([`super::copy`]);
//! - [`TOOLS`], the tools it was narrowed to (`Said::tools`), when a list
//!   narrowed it: a JSON array of names. Left out, it has every tool.
//!
//! A conversation kept `compact` has its events in [`super::compact`]'s
//! frames beside a tail of [`EVENTS`].

use std::collections::BTreeMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Its events, a line of JSON each.
pub const EVENTS: &str = "events.jsonl";

/// The copy for people.
pub const MARKDOWN: &str = "conversation.md";

/// The tools it was narrowed to.
pub const TOOLS: &str = "tools.json";

/// The most [`TOOLS`] holds: [`crate::connection::TOOLS_MAX`] names of at
/// most 128 bytes, as JSON.
pub const TOOLS_FILE_MAX: u64 = 64 << 10;

/// The folder conversation `thread` is kept in: its name, each `:` a `-`.
pub fn folder_of(thread: &str) -> String {
    thread.replace(':', "-")
}

/// The tools [`TOOLS`] holds: an error for a file larger than
/// [`TOOLS_FILE_MAX`], or one that isn't a list a conversation may be
/// narrowed to ([`super::check_tools`]). Read where a narrowed list is
/// kept, an error fails what reads it rather than give every tool.
pub fn read_tools(bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(
        bytes.len() as u64 <= TOOLS_FILE_MAX,
        "{TOOLS} is larger than {TOOLS_FILE_MAX} bytes"
    );
    let tools: Vec<String> =
        serde_json::from_slice(bytes).with_context(|| format!("reading {TOOLS}"))?;
    super::check_tools(&tools).with_context(|| format!("reading {TOOLS}"))?;
    Ok(tools)
}

/// [`TOOLS`] holding `tools`.
pub fn tools_file(tools: &[String]) -> anyhow::Result<Vec<u8>> {
    super::check_tools(tools)?;
    Ok(serde_json::to_vec(tools)?)
}

/// One conversation, whole: its thread, its events as written, and the
/// tools it was narrowed to (none: every tool).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub thread: String,
    pub events: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}

impl Conversation {
    /// The conversation folder `folder` holds, from its `files` by name:
    /// none when it holds neither events nor a narrowed list. Its thread
    /// is its copy's first line, or a compact log's name, where that
    /// names this folder; else the folder's name.
    pub fn from_files(
        folder: &str,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> anyhow::Result<Option<Self>> {
        let tools = files
            .get(TOOLS)
            .map(|bytes| read_tools(bytes))
            .transpose()
            .with_context(|| format!("conversation {}", link_format::clean(folder)))?;
        let events: Option<Vec<String>> = match (Self::frames(files), files.get(EVENTS)) {
            (Some(frames), tail) => Some(Self::compact(frames, tail)?),
            (None, Some(log)) => Some(
                String::from_utf8_lossy(log)
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(str::to_string)
                    .collect(),
            ),
            (None, None) => None,
        };
        if events.is_none() && tools.is_none() {
            return Ok(None);
        }
        let from_copy = files.get(MARKDOWN).and_then(|md| {
            let first = std::str::from_utf8(md).ok()?.lines().next()?;
            first.strip_prefix("# ").map(str::to_string)
        });
        let thread = from_copy
            .or_else(|| Self::frames(files).and_then(Self::named))
            .filter(|t| folder_of(t) == folder)
            .unwrap_or_else(|| folder.to_string());
        Ok(Some(Self {
            thread,
            events: events.unwrap_or_default(),
            tools,
        }))
    }

    /// Its folder's files, by name: its events when it has any, their
    /// copy always, whose first line names its thread (a folder's name
    /// can't: `:` is `-` in it), and [`TOOLS`] when a list narrowed it.
    pub fn files(&self) -> anyhow::Result<Vec<(&'static str, Vec<u8>)>> {
        let mut files = Vec::new();
        if self.events.is_empty() {
            files.push((MARKDOWN, super::copy::header(&self.thread).into_bytes()));
        } else {
            let mut log = self.events.join("\n");
            log.push('\n');
            let events: Vec<serde_json::Value> = self
                .events
                .iter()
                .filter_map(|e| serde_json::from_str(e).ok())
                .collect();
            let said: Vec<super::copy::Said> = events.iter().map(super::copy::Said::of).collect();
            files.push((EVENTS, log.into_bytes()));
            files.push((
                MARKDOWN,
                super::copy::markdown(&self.thread, &said).into_bytes(),
            ));
        }
        if let Some(tools) = &self.tools {
            files.push((TOOLS, tools_file(tools)?));
        }
        Ok(files)
    }

    #[cfg(feature = "compact")]
    fn frames(files: &BTreeMap<String, Vec<u8>>) -> Option<&[u8]> {
        files.get(super::compact::FRAMES).map(Vec::as_slice)
    }

    #[cfg(not(feature = "compact"))]
    fn frames(_: &BTreeMap<String, Vec<u8>>) -> Option<&[u8]> {
        None
    }

    #[cfg(feature = "compact")]
    fn named(frames: &[u8]) -> Option<String> {
        super::compact::thread(frames)
    }

    #[cfg(not(feature = "compact"))]
    fn named(_: &[u8]) -> Option<String> {
        None
    }

    #[cfg(feature = "compact")]
    fn compact(frames: &[u8], tail: Option<&Vec<u8>>) -> anyhow::Result<Vec<String>> {
        Ok(
            super::compact::read(frames, tail.map(Vec::as_slice).unwrap_or_default())?
                .into_iter()
                .map(|e| e.to_string())
                .collect(),
        )
    }

    #[cfg(not(feature = "compact"))]
    fn compact(_: &[u8], _: Option<&Vec<u8>>) -> anyhow::Result<Vec<String>> {
        unreachable!("frames are read only with compact")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(of: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        of.iter()
            .map(|(name, text)| (name.to_string(), text.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn a_narrowed_conversation_goes_to_files_and_back_whole() {
        let narrowed = Conversation {
            thread: "task:aaaa".into(),
            events: vec![
                r#"{"seq":1,"role":"user","text":"Find it."}"#.into(),
                r#"{"seq":2,"role":"assistant","text":"Found."}"#.into(),
            ],
            tools: Some(vec!["web__search".into()]),
        };
        let written: BTreeMap<String, Vec<u8>> = narrowed
            .files()
            .unwrap()
            .into_iter()
            .map(|(name, bytes)| (name.to_string(), bytes))
            .collect();
        assert_eq!(
            written.keys().collect::<Vec<_>>(),
            [MARKDOWN, EVENTS, TOOLS]
        );
        assert_eq!(written[TOOLS], br#"["web__search"]"#);
        let back = Conversation::from_files(&folder_of(&narrowed.thread), &written).unwrap();
        assert_eq!(back, Some(narrowed));
    }

    #[test]
    fn a_list_alone_is_a_conversation_and_nothing_is_none() {
        // Narrowed before its first event was written (a turn that
        // crashed): still narrow where it goes.
        let alone = Conversation::from_files("task-b", &files(&[(TOOLS, r#"["a__b"]"#)]))
            .unwrap()
            .unwrap();
        assert_eq!(alone.thread, "task-b");
        assert!(alone.events.is_empty());
        assert_eq!(alone.tools, Some(vec!["a__b".to_string()]));
        assert_eq!(
            alone.files().unwrap(),
            [
                (MARKDOWN, b"# task-b\n".to_vec()),
                (TOOLS, br#"["a__b"]"#.to_vec())
            ]
        );
        // Its thread named in its folder, a `:` in it too: read back as
        // itself, never as its folder's name.
        let colon = Conversation {
            thread: "task:b".into(),
            events: Vec::new(),
            tools: Some(vec!["a__b".into()]),
        };
        let written: BTreeMap<String, Vec<u8>> = colon
            .files()
            .unwrap()
            .into_iter()
            .map(|(name, bytes)| (name.to_string(), bytes))
            .collect();
        assert_eq!(
            Conversation::from_files("task-b", &written).unwrap(),
            Some(colon)
        );
        assert_eq!(
            Conversation::from_files("x", &files(&[(MARKDOWN, "# x\n")])).unwrap(),
            None
        );
    }

    #[test]
    fn a_list_that_isnt_one_is_refused_never_read_as_every_tool() {
        for bad in ["{}", "[\"no spaces allowed\"]", "nope", ""] {
            let err = Conversation::from_files("t", &files(&[(EVENTS, "{}\n"), (TOOLS, bad)]));
            assert!(err.is_err(), "{bad:?}");
        }
        let many: Vec<String> = (0..=crate::connection::TOOLS_MAX)
            .map(|n| format!("t{n}"))
            .collect();
        assert!(read_tools(&serde_json::to_vec(&many).unwrap()).is_err());
        assert!(read_tools(&vec![b' '; TOOLS_FILE_MAX as usize + 1]).is_err());
    }

    #[test]
    fn its_thread_is_its_copys_first_line_where_that_names_its_folder() {
        let named = files(&[(EVENTS, "{}\n"), (MARKDOWN, "# telegram:42\n\nhi\n")]);
        let thread = |folder| {
            Conversation::from_files(folder, &named)
                .unwrap()
                .unwrap()
                .thread
        };
        assert_eq!(thread("telegram-42"), "telegram:42");
        assert_eq!(thread("other"), "other");
    }
}
