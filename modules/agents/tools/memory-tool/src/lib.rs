//! Link Memory: a folder an agent can search, read, list and follow the
//! links of, over MCP
//! ([`mcp`]). Keyless and local: an index of passages in SQLite and of
//! their words in Tantivy, with English stemming, over the folder's text
//! files.
//!
//! What's under [`CONVERSATIONS`] is a conversation (its markdown copy,
//! the one a person reads; the events beside it are left out); anything
//! else is a file you dropped in. Every result says which. A
//! conversation kept without a copy (compact, or by a store driver) gets
//! one memory makes itself, from its events ([`copies`]).
//!
//! The index is only ever what's in the folder, brought up to it by one
//! writer ([`serve::Writer`]) while calls read through connections of
//! their own ([`Reader`]), so no call waits for indexing. It never
//! follows a symbolic link, not even one swapped in while it reads
//! ([`folder`]), or goes into a hidden folder, and [`Reader::read`]
//! serves only what's indexed, and a skill's text file under `skills/`
//! by its path ([`tree::Files`]), so nothing outside the folder is
//! reached. The index keeps where each passage of a document is, never
//! its text, which is read from the file ([`source`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, named_params, params};
use serde::Serialize;

pub mod cache;
mod checks;
pub mod cli;
#[cfg(unix)]
pub mod copies;
pub mod extract;
mod folder;
pub mod index;
pub mod limits;
pub mod links;
pub mod mcp;
pub mod notes;
mod opening;
pub mod opinions;
pub mod pages;
pub mod queue;
mod reader;
mod reading;
mod refreshing;
mod schema;
mod searching;
pub mod semantic;
#[cfg(unix)]
pub mod serve;
#[cfg(unix)]
pub mod service;
mod source;
pub mod tree;
pub mod vectors;
mod writing;
pub use reader::{Common, Reader};
use reading::kind_of;
pub use reading::parse_since;
use refreshing::title;
pub use schema::{check_copy, open_file};
pub use searching::Searched;

/// A warning logged along the way: the index holds no `Out`. The error's
/// words follow the headline.
pub(crate) fn warn(headline: link_format::Sentence, err: &anyhow::Error) {
    link_format::daemon::warn!("{headline} {err:#}");
}
use schema::create;

use semantic::{Embedded, Shared, State, Status};
use std::sync::Arc;

/// The folder, in the one served, that holds conversations: one folder
/// per conversation, with its events and a markdown copy.
pub const CONVERSATIONS: &str = link_harness_contract::harness::layout::CONVERSATIONS;

/// The files indexed outside [`CONVERSATIONS`], by extension: text a
/// person writes or keeps. Inside, only markdown.
pub(crate) const TEXT: &[&str] = &[
    "md", "markdown", "mdx", "txt", "text", "org", "rst", "adoc", "csv", "tsv", "json", "jsonl",
    "yaml", "yml", "toml", "html", "htm", "xml", "log",
];

/// The index's layout; any other is rebuilt.
const SCHEMA: i64 = 10;

/// Counts the passages and those embedded, in `meta` (`passages`,
/// `embedded`) as each is written: a search's reply says how far
/// embedding has got, and nothing counts them.
const COUNT_ADDED: &str = "INSERT INTO meta (key, value)
     VALUES ('passages', 1), ('embedded', new.slot IS NOT NULL)
     ON CONFLICT (key) DO UPDATE SET value = value + excluded.value;";
const COUNT_REMOVED: &str = "INSERT INTO meta (key, value)
     VALUES ('passages', -1), ('embedded', -(old.slot IS NOT NULL))
     ON CONFLICT (key) DO UPDATE SET value = value + excluded.value;";
/// Keeps the embedding queue as passages are written and embedded: one
/// not embedded waits, by its priority, conversations and notes first,
/// then documents whose text was extracted ([`QUEUE_LATER`] on), each
/// newest first ([`queue`]).
const QUEUED: &str = "INSERT OR REPLACE INTO queue (passage, priority)
     SELECT new.id, CASE WHEN d.kind = 'conversation'
             OR NOT EXISTS (SELECT 1 FROM derived WHERE document = d.id) THEN 0
             ELSE {QUEUE_LATER} END - d.modified
     FROM documents d WHERE d.id = new.document AND new.slot IS NULL;
     DELETE FROM queue WHERE passage = new.id AND new.slot IS NOT NULL;";
const UNQUEUED: &str = "DELETE FROM queue WHERE passage = old.id;";

/// Lets a passage's slot go, for the next passage embedded to take: when
/// the passage goes, or its vector does.
const FREED: &str = "INSERT OR IGNORE INTO free_slots (slot)
     SELECT old.slot WHERE old.slot IS NOT NULL AND old.slot IS NOT new.slot;";
const FREED_GONE: &str = "INSERT OR IGNORE INTO free_slots (slot)
     SELECT old.slot WHERE old.slot IS NOT NULL;";

/// Where documents whose text was extracted start in the queue: after
/// every conversation and note, whenever each changed.
const QUEUE_LATER: i64 = 4_000_000_000_000_000_000;

const COUNT_EMBEDDED: &str = "INSERT INTO meta (key, value)
     VALUES ('embedded', (new.slot IS NOT NULL) - (old.slot IS NOT NULL))
     ON CONFLICT (key) DO UPDATE SET value = value + excluded.value;";

/// The writer's page cache: this share of what the index may hold
/// ([`opinions::Opinions::memory`]).
const WRITER_SHARE: u64 = 2;

/// What Tantivy's writer buffers of the words before it writes a
/// segment: this share of it.
const WORDS_SHARE: u64 = 4;

/// Each reader's page cache, a session's: this share of it.
const READER_SHARE: u64 = 16;

/// How long after the last change it's told of a tree listed only on a
/// change is listed again: a burst of writes, listed once.
pub const SETTLE: std::time::Duration = std::time::Duration::from_secs(2);

/// The most documents each of words and meaning ranks, before fusing.
const RANKED: usize = 50;

/// How close two scores are for their documents to match about as
/// well: within a tenth. Among those, the newer ranks first.
const AS_WELL: f64 = 0.1;

/// Documents of at least this many passages are copies when nearly all
/// their passages are the same ([`NEAR`]); shorter ones only when all
/// are.
const NEAR_PASSAGES: usize = 10;
const NEAR: f64 = 0.9;

/// What a document is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A file you dropped in.
    File,
    /// A conversation's markdown copy.
    Conversation,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Conversation => "conversation",
        }
    }

    pub fn parse(kind: &str) -> Result<Kind> {
        match kind {
            "file" => Ok(Kind::File),
            "conversation" => Ok(Kind::Conversation),
            other => bail!("no kind {other:?}: 'file' or 'conversation'"),
        }
    }
}

/// A search's match.
#[derive(Debug, Serialize)]
pub struct Hit {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    /// Where it matched, the words marked `**`.
    pub snippet: String,
    /// Why it came back: `words`, `meaning` or `both`.
    pub matched: &'static str,
    /// The line where it matched, from 1 (for `read`'s `from`).
    pub line: usize,
    /// Copies of it, the same words at other paths, left out of the
    /// results.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<String>,
    /// How far into what the search ranked it is: the next page starts
    /// here ([`Reader::search_from`]).
    #[serde(skip)]
    pub at: usize,
}

/// A document, as listed.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub size: u64,
    /// When it last changed, in seconds since 1970.
    pub modified: i64,
    /// Listed with `since`: `new` (first seen after it) or `changed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<&'static str>,
    /// When it last changed or was first seen, as the index orders a
    /// list: a page after it names it ([`pages::Page::List`]).
    #[serde(skip)]
    pub changed: i64,
}

/// A document gone from the folder, as `list` gives it with `since`.
#[derive(Debug, Serialize)]
pub struct Removed {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    /// When the index found it gone, in seconds since 1970.
    pub removed: i64,
}

/// What `list` gives: the documents, how many match in all, and with
/// `since`, what was removed after it.
#[derive(Debug, Serialize)]
pub struct Listing {
    pub documents: Vec<Entry>,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<Vec<Removed>>,
    /// Whether more documents follow the last given.
    #[serde(skip)]
    pub more: bool,
}

/// A document at one end of a link.
#[derive(Debug, Serialize)]
pub struct Linked {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    /// The line the link is on, in the document that links.
    pub line: usize,
}

/// A link to nothing memory holds: as written, and its line.
#[derive(Debug, Serialize)]
pub struct Missing {
    pub target: String,
    pub line: usize,
}

/// What `links` gives: a document, what it links to and what links to
/// it, each way the first `links-max` ([`crate::limits::Limits`]).
#[derive(Debug, Serialize)]
pub struct Links {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    pub to: Vec<Linked>,
    pub from: Vec<Linked>,
    /// What it links to that memory doesn't hold.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<Missing>,
}

/// Which documents a search or a list looks at: of a kind, under a
/// folder, changed after a time. None of them: every document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub kind: Option<Kind>,
    /// A folder, from the one served (`memory/taxes`, `chats/<thread>`).
    pub under: Option<String>,
    /// Seconds since 1970: only what was changed, or first seen, after.
    pub since: Option<i64>,
}

impl From<Option<Kind>> for Filter {
    fn from(kind: Option<Kind>) -> Self {
        Filter {
            kind,
            ..Filter::default()
        }
    }
}

impl Filter {
    /// Its SQL, over `documents d`, with `:kind`, `:under` and `:since`.
    const SQL: &str = "(:kind IS NULL OR d.kind = :kind)
         AND (:under IS NULL OR d.path LIKE :under ESCAPE '\\')
         AND (:since IS NULL OR d.modified > :since OR d.added > :since)";

    fn kind(&self) -> Option<&'static str> {
        self.kind.map(Kind::as_str)
    }

    /// `under` as a LIKE pattern for the paths in it.
    fn under(&self) -> Option<String> {
        let under = self
            .under
            .as_deref()
            .map(|u| u.trim_matches('/').trim_start_matches("./"))
            .filter(|u| !u.is_empty() && *u != ".")?;
        let escaped = under
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        Some(format!("{escaped}/%"))
    }

    /// `since` in the index's nanoseconds.
    fn since(&self) -> Option<i64> {
        self.since.map(|s| s.saturating_mul(1_000_000_000))
    }
}

/// A document's text, or some of its lines.
#[derive(Debug, Serialize)]
pub struct Document {
    pub path: String,
    pub kind: Kind,
    pub title: String,
    /// Its lines in all.
    pub lines: usize,
    pub text: String,
    /// Source identity and the exact selected text before MCP presentation.
    pub read: link_files::reading::ReadReceipt,
    /// When it stopped before the end: the line to read from next.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<usize>,
    /// Whether a line longer than the size was cut: the rest of it isn't
    /// in `text`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
}

/// About how many tokens `text` takes: four characters to a token, as
/// Link Harness counts them.
pub fn tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// What a refresh changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Read again: new or changed.
    pub indexed: usize,
    pub removed: usize,
    /// In the index after.
    pub total: usize,
}

/// A folder's index: the one writer of its file, and a reader of its own
/// for the calls made of it directly ([`Index::reader`]).
pub struct Index {
    /// The writer's connection.
    db: Connection,
    /// What it indexes.
    tree: Box<dyn tree::Tree>,
    /// The index's file as given; none, it's kept in a temporary file,
    /// removed with it.
    file: Option<PathBuf>,
    /// What it shares with every call.
    common: Arc<Common>,
    /// Its own reader, for calls made of it directly.
    reader: Reader,
    /// When the last refresh finished.
    refreshed: Option<std::time::Instant>,
    /// What the last walk found and isn't written yet.
    queue: Option<refreshing::Queue>,
    /// How many files the last walk found.
    listed_total: usize,
    /// What it may spend.
    opinions: opinions::Opinions,
    /// The watch on its folder, noting changes in its journal, while it
    /// has one ([`Index::watch`]).
    watch: Option<index::Watch>,
    /// Whether Link Document Converter was there at the writer's last turn.
    converter: bool,
    /// What extracts a PDF's or a Word document's text.
    extractor: extract::Extractor,
    /// The thread extracting documents' text in the background, while
    /// it runs ([`Index::extract_pending`]).
    extracting: Option<refreshing::Extraction>,
    /// How fast the last batch of what a walk queued was written, in
    /// bytes a second: the next is sized by it
    /// ([`crate::limits::Limits::batch_millis`]). None before the first.
    speed: Option<f64>,
}

impl Drop for Index {
    /// Its embedding thread ends with it, and its extraction is told to
    /// stop after the document it's on, and waited for. A temporary file
    /// goes with it.
    fn drop(&mut self) {
        self.common.semantic.stop();
        if let Some(extraction) = self.extracting.take() {
            extraction.stop();
        }
        self.common.words.release();
        if self.file.is_none() {
            for end in ["", "-wal", "-shm"] {
                let mut path = self.common.file.clone().into_os_string();
                path.push(end);
                let _ = std::fs::remove_file(path);
            }
            let _ = std::fs::remove_dir_all(self.common.words.folder());
        }
    }
}

#[cfg(test)]
link_test_kit::program!(system);
