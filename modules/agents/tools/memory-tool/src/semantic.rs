//! Search by meaning: each document split into passages, each passage's
//! vector from an embedding model on this machine (served for it outside
//! its sandbox, or `lnk model serve --model <model> --json` run here; the
//! OpenAI-style `/v1/embeddings`), kept in the same
//! SQLite index, and compared with the query's in Rust.
//!
//! Embedding runs beside the server, in a thread of its own with its own
//! connection to the index file ([`start`]), newest documents first, so
//! the first index doesn't hold up a search: what's embedded so far is
//! searched by meaning, the rest by words.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// The longest passage, in characters: about 400 tokens.
pub const CHUNK: usize = 1500;

/// The embedding model `lnk agent memory start link` asks for.
pub const DEFAULT_MODEL: &str = "qwen3-embedding";

/// What `lnk model serve` exits with when no runtime here serves the
/// model (`docs/architecture/modules/contracts.md`).
const NO_SUCH_MODEL: i32 = 2;

/// A model that turns text into vectors.
pub trait Embedder: Send + Sync {
    /// Its name, kept with each vector: another model's are embedded again.
    fn model(&self) -> &str;
    /// One vector per text, in order.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    /// A search's words as a vector, soon or not at all: a search waits
    /// for it, and falls back to words alone when it fails.
    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.embed(&[query.to_string()])?
            .pop()
            .context("the embedding model gave no vector")
    }
}

/// What's embedded to learn a model served for it answers.
const PROBE: &str = "ready";

/// How long after a model didn't answer it's asked again.
const PROBE_AGAIN: Duration = Duration::from_millis(500);

/// How long a search waits for its words' vector.
const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// An OpenAI-style embeddings endpoint: `POST <url>/v1/embeddings`. A
/// `unix:<path>` address is one on a Unix socket. Bound agents prove
/// their current binding there; the server resolves their owner.
pub struct Http {
    url: String,
    model: String,
    authentication: Option<link_memory_tool_contract::embedding::Authentication>,
    client: reqwest::blocking::Client,
}

impl Http {
    pub fn new(url: &str, model: &str) -> Result<Self> {
        link_fetch::install_crypto_provider();
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            authentication: None,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(300))
                .no_proxy()
                // The model's server on this machine answers itself: a
                // redirect is never followed elsewhere.
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    /// A live binding's authentication is sent only to a Unix endpoint.
    pub fn for_binding(
        url: &str,
        model: &str,
        binding: &link_sandbox_binding::Binding,
        team: Option<link_memory_tool_contract::embedding::Team>,
    ) -> Result<Self> {
        anyhow::ensure!(
            url.starts_with("unix:"),
            "a bound embedding endpoint is a Unix socket"
        );
        let mut http = Self::new(url, model)?;
        http.authentication = Some(match team {
            Some(team) => {
                link_memory_tool_contract::embedding::Authentication::for_team(binding, team)?
            }
            None => link_memory_tool_contract::embedding::Authentication::new(binding)?,
        });
        Ok(http)
    }
}

/// The most of an embedding model's answer read: a batch's vectors are
/// a few MiB.
const REPLY_MAX: usize = 64 << 20;

/// At most `max` bytes of `response`, and one more if there are more.
fn read_capped(response: impl std::io::Read, max: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    response.take(max as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

impl Embedder for Http {
    fn model(&self) -> &str {
        &self.model
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.request(texts, None)
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.request(&[query.to_string()], Some(QUERY_TIMEOUT))?
            .pop()
            .context("the embedding model gave no vector")
    }
}

impl Http {
    /// `texts`' vectors, within `timeout` when given, else the client's.
    fn request(&self, texts: &[String], timeout: Option<Duration>) -> Result<Vec<Vec<f32>>> {
        #[derive(serde::Deserialize)]
        struct Reply {
            data: Vec<Item>,
        }
        #[derive(serde::Deserialize)]
        struct Item {
            #[serde(default)]
            index: usize,
            embedding: Vec<f32>,
        }
        let body = serde_json::json!({"model": self.model, "input": texts});
        let (status, bytes) = match self.url.strip_prefix("unix:") {
            Some(socket) => unix_post(
                Path::new(socket),
                &serde_json::to_vec(&body)?,
                self.authentication.as_ref(),
                timeout.unwrap_or(Duration::from_secs(300)),
            )?,
            None => {
                let mut request = self
                    .client
                    .post(format!("{}/v1/embeddings", self.url))
                    .json(&body);
                if let Some(timeout) = timeout {
                    request = request.timeout(timeout);
                }
                let response = request.send().context("reaching the embedding model")?;
                let status = response.status().as_u16();
                let most = match status {
                    200..=299 => REPLY_MAX,
                    _ => 4096,
                };
                (
                    status,
                    read_capped(response, most).context("reading the embeddings")?,
                )
            }
        };
        if !(200..300).contains(&status) {
            let body = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
            bail!(
                "the embedding model answered {status}: {}",
                link_format::clean(body.chars().take(200).collect::<String>().trim())
            );
        }
        anyhow::ensure!(
            bytes.len() <= REPLY_MAX,
            "the embedding model's answer is over {} MiB",
            REPLY_MAX >> 20
        );
        let mut reply: Reply = serde_json::from_slice(&bytes).context("reading the embeddings")?;
        if reply.data.len() != texts.len() {
            bail!(
                "the embedding model gave {} vectors for {} texts",
                reply.data.len(),
                texts.len()
            );
        }
        reply.data.sort_by_key(|item| item.index);
        Ok(reply.data.into_iter().map(|item| item.embedding).collect())
    }
}

/// `body` posted to `/v1/embeddings` on the Unix socket `socket`, as
/// HTTP/1.1, the connection closed after: its status and body, at most
/// [`REPLY_MAX`] and one more byte.
fn unix_post(
    socket: &Path,
    body: &[u8],
    authentication: Option<&link_memory_tool_contract::embedding::Authentication>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>)> {
    use std::io::{Read, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket)
        .with_context(|| format!("reaching the embedding model at {}", socket.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let head = format!(
        "POST /v1/embeddings HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    let head = match authentication {
        Some(authentication) => authentication
            .header()
            .expose_map(|authorization| format!("{head}Authorization: {authorization}\r\n\r\n")),
        None => format!("{head}\r\n").into(),
    };
    stream.write_all(head.expose().as_bytes())?;
    drop(head);
    stream.write_all(body)?;
    let mut reply = Vec::new();
    (&mut stream)
        .take(REPLY_MAX as u64 + (64 << 10))
        .read_to_end(&mut reply)
        .context("reading the embeddings")?;
    let end = reply
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("the embedding model's answer has no headers")?;
    let head = String::from_utf8_lossy(&reply[..end]).to_ascii_lowercase();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .context("the embedding model's answer has no status")?;
    let mut body = reply[end + 4..].to_vec();
    if head.contains("transfer-encoding: chunked") {
        body = dechunk(&body)?;
    }
    Ok((status, body))
}

/// A chunked HTTP body, whole.
fn dechunk(mut bytes: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line = bytes
            .windows(2)
            .position(|w| w == b"\r\n")
            .context("a chunked answer cut short")?;
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&bytes[..line])
                .split(';')
                .next()
                .unwrap_or("")
                .trim(),
            16,
        )
        .context("a chunked answer's size")?;
        bytes = &bytes[line + 2..];
        if size == 0 {
            return Ok(out);
        }
        anyhow::ensure!(bytes.len() >= size + 2, "a chunked answer cut short");
        out.extend_from_slice(&bytes[..size]);
        bytes = &bytes[size + 2..];
    }
}

/// Where the embedding model comes from.
#[derive(Debug)]
pub enum Source {
    /// `lnk model serve --model <model> --json`, kept running while this serves.
    Serve(String),
    /// An endpoint already running (served for it, or a test's), and
    /// the model to ask it for.
    Url(String, String),
    /// An endpoint a machine serving many owners' agents runs, the model
    /// to ask it for, and its current binding's credential.
    Bound {
        url: String,
        model: String,
        binding: link_sandbox_binding::Binding,
        team: Option<link_memory_tool_contract::embedding::Team>,
    },
    /// A model that was to be served for it and couldn't be: no runtime
    /// here has it.
    Missing(String),
}

/// Whether search by meaning is on, as searches see it.
#[derive(Clone)]
pub enum State {
    /// Not asked for (no `--embed`).
    Off(String),
    /// Finding the model.
    Starting,
    On(Arc<dyn Embedder>),
}

/// How far embedding has got, and whether it's on, for a search's reply.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Status {
    /// Passages embedded with the current model, of all; left out once
    /// every passage is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded: Option<Embedded>,
    /// Why search is by words alone, and how to turn meaning on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic: Option<String>,
    /// What memory has to say of what it indexes: once, that it indexes
    /// less than it did; and each document found by its name alone, and
    /// why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
    /// What isn't indexed yet: left out once everything memory was told
    /// of is, so a file written a moment ago may not be found while it's
    /// here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing: Option<Indexing>,
}

/// What isn't indexed yet, as a call's answer says it.
#[derive(Debug, Serialize, PartialEq)]
pub struct Indexing {
    /// Whether the folder is still being read for the first time.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub walking: bool,
    /// The changes memory was told of and hasn't indexed yet.
    pub waiting: usize,
    /// Why indexing is paused, while it is: the disk short of what it keeps
    /// free. Calls go on answering from what was indexed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Embedded {
    pub done: usize,
    pub total: usize,
}

/// What a search by meaning and one by words share.
pub struct Shared {
    pub state: Mutex<State>,
    /// The embedding thread's last error, until it succeeds again.
    pub failing: Mutex<Option<String>>,
    /// Set when its index is closed: the embedding thread lets it go.
    stopped: std::sync::atomic::AtomicBool,
    /// Whether the process's embedding thread embeds its index.
    queued: std::sync::atomic::AtomicBool,
    /// The documents read lately, whose passages move up the queue.
    read: Mutex<Vec<i64>>,
}

impl Shared {
    pub fn new(state: State) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            failing: Mutex::new(None),
            stopped: std::sync::atomic::AtomicBool::new(false),
            queued: std::sync::atomic::AtomicBool::new(false),
            read: Mutex::new(Vec::new()),
        })
    }

    /// Lets its index go from the embedding thread, at its next turn.
    pub fn stop(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.wake();
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stopped.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Tells the embedding thread there are passages to embed.
    pub fn wake(&self) {
        if self.queued.load(std::sync::atomic::Ordering::SeqCst) {
            crate::queue::Embedding::of_process().woken();
        }
    }

    /// Document `document` was read: its passages move up the queue.
    pub(crate) fn was_read(&self, document: i64) {
        let mut read = self.read.lock().unwrap_or_else(|e| e.into_inner());
        // A few hundred at most between batches: the rest wait their turn.
        if read.len() < 256 && !read.contains(&document) {
            read.push(document);
        }
    }

    /// The documents read since it was last asked.
    pub(crate) fn take_read(&self) -> Vec<i64> {
        std::mem::take(&mut *self.read.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set(&self, state: State) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }

    pub(crate) fn fail(&self, why: Option<String>) {
        *self.failing.lock().unwrap_or_else(|e| e.into_inner()) = why;
    }

    pub fn failing(&self) -> Option<String> {
        self.failing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// Starts embedding the index `file` in the background, from `source`:
/// the model found (or why not) goes in `shared`, then the process's one
/// embedding thread embeds every passage not embedded with it, a batch at
/// a time in turn with its other indexes ([`crate::queue::Embedding`]).
#[expect(clippy::disallowed_methods, reason = "backs off before asking again")]
pub fn start(
    file: PathBuf,
    files: Arc<dyn crate::tree::Files>,
    source: Source,
    shared: Arc<Shared>,
    dimensions: u32,
) {
    std::thread::spawn(move || {
        // Held for as long as it's embedded with: closing its stdin stops it.
        let (served, embedder) = match find(&source) {
            Ok(found) => found,
            Err(why) => {
                shared.set(State::Off(why));
                return;
            }
        };
        let embedder: Arc<dyn Embedder> = Arc::new(embedder);
        // A model served for it may still be starting (its service listens
        // before its models answer): on once it answers, by words till then.
        if let Source::Url(..) = source {
            while embedder.embed_query(PROBE).is_err() {
                if shared.stopped() {
                    return;
                }
                std::thread::sleep(PROBE_AGAIN);
            }
        }
        shared.set(State::On(embedder.clone()));
        let db = match crate::open_file(&file) {
            Ok(db) => db,
            Err(err) => {
                shared.set(State::Off(format!("{err:#}")));
                return;
            }
        };
        let _ = db.busy_timeout(Duration::from_secs(10));
        let kept = crate::vectors::Kept::new(&file, embedder.model(), dimensions);
        if let Err(err) = kept.take_over(&db) {
            shared.fail(Some(format!("{err:#}")));
        }
        shared
            .queued
            .store(true, std::sync::atomic::Ordering::SeqCst);
        crate::queue::Embedding::of_process().add(crate::queue::Job {
            db,
            files,
            embedder,
            kept,
            shared,
            _served: served,
        });
    });
}

/// The model `source` names, and the server kept running for it.
fn find(source: &Source) -> Result<(Option<Child>, Http), String> {
    match source {
        Source::Url(url, model) => Http::new(url, model)
            .map(|http| (None, http))
            .map_err(|err| format!("{err:#}")),
        Source::Bound {
            url,
            model,
            binding,
            team,
        } => Http::for_binding(url, model, binding, team.clone())
            .map(|http| (None, http))
            .map_err(|err| format!("{err:#}")),
        Source::Missing(model) => Err(no_such_model(model)),
        Source::Serve(model) => {
            let mut child = link_core::lnk()
                .args(["model", "serve", "--model", model, "--json"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|err| format!("running 'lnk model serve': {err}"))?;
            let stdout = child.stdout.take().ok_or("no output")?;
            let mut line = String::new();
            let _ = std::io::BufReader::new(stdout).read_line(&mut line);
            let url = serde_json::from_str::<serde_json::Value>(&line)
                .ok()
                .and_then(|v| v.get("url")?.as_str().map(String::from));
            let Some(url) = url else {
                let status = child.wait().ok().and_then(|s| s.code());
                return Err(match status {
                    Some(NO_SUCH_MODEL) => no_such_model(model),
                    _ => format!("'lnk model serve {model}' didn't start"),
                });
            };
            let http = Http::new(&url, model).map_err(|err| format!("{err:#}"))?;
            Ok((Some(child), http))
        }
    }
}

pub(crate) fn no_such_model(model: &str) -> String {
    format!("no model runtime here serves {model}: 'lnk model install {model}' installs it")
}

/// A title as words, as a passage is embedded with it: a file's name
/// without its extension, its dashes and underscores spaces
/// (`passport-renewal.txt`: `passport renewal`), so a title counts by
/// meaning as it does by words.
pub fn title_words(title: &str) -> String {
    let name = match title.rsplit_once('.') {
        Some((name, extension))
            if !name.is_empty() && !extension.is_empty() && !extension.contains(' ') =>
        {
            name
        }
        _ => title,
    };
    name.replace(['-', '_'], " ").trim().to_string()
}

/// A passage of a document: its bytes, `start` to `end`, and the line
/// it starts on, from 1. What a search ranks, what is embedded, and where
/// a result points; the index keeps where it is, never its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Passage {
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

/// How many characters `bytes` holds, as UTF-8 counts them: every byte
/// that doesn't continue one.
fn characters(bytes: &[u8]) -> usize {
    bytes.iter().filter(|b| (**b as i8) >= -0x40).count()
}

/// `body` in passages of up to [`CHUNK`] characters: split at markdown
/// headings (a conversation's turns among them), then at blank lines,
/// short paragraphs packed together; a paragraph longer than a passage
/// is cut. Each is a range of `body`'s own bytes, so what a file holds
/// is read from the file.
pub fn passages(body: &[u8]) -> Vec<Passage> {
    let mut out: Vec<Passage> = Vec::new();
    let mut current: Option<Passage> = None;
    let mut paragraph: Option<Passage> = None;
    let close_paragraph =
        |paragraph: &mut Option<Passage>, current: &mut Option<Passage>, out: &mut Vec<Passage>| {
            let Some(p) = paragraph.take() else { return };
            for piece in cut(body, p) {
                match current {
                    Some(c) if characters(&body[c.start..piece.end]) <= CHUNK => c.end = piece.end,
                    _ => {
                        out.extend(current.take());
                        *current = Some(piece);
                    }
                }
            }
        };
    let mut start = 0;
    let mut n = 0;
    while start < body.len() {
        let end = body[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(body.len(), |at| start + at);
        let mut line = &body[start..end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        n += 1;
        if line.iter().all(u8::is_ascii_whitespace) {
            close_paragraph(&mut paragraph, &mut current, &mut out);
        } else {
            if line.first() == Some(&b'#') {
                close_paragraph(&mut paragraph, &mut current, &mut out);
                out.extend(current.take());
            }
            let line_end = start + line.len();
            match &mut paragraph {
                Some(p) => p.end = line_end,
                None => {
                    paragraph = Some(Passage {
                        start,
                        end: line_end,
                        line: n,
                    })
                }
            }
        }
        start = end + 1;
    }
    close_paragraph(&mut paragraph, &mut current, &mut out);
    out.extend(current);
    out
}

/// A paragraph cut into pieces of at most [`CHUNK`] characters, each
/// starting at the line it starts in, never inside a character.
fn cut(body: &[u8], paragraph: Passage) -> Vec<Passage> {
    if characters(&body[paragraph.start..paragraph.end]) <= CHUNK {
        return vec![paragraph];
    }
    let mut pieces = Vec::new();
    let (mut line, mut start, mut start_line, mut count) =
        (paragraph.line, paragraph.start, paragraph.line, 0);
    for (at, &byte) in body
        .iter()
        .enumerate()
        .take(paragraph.end)
        .skip(paragraph.start)
    {
        // A character starts here.
        if (byte as i8) >= -0x40 {
            if count == CHUNK {
                pieces.push(Passage {
                    start,
                    end: at,
                    line: start_line,
                });
                (start, start_line, count) = (at, line, 0);
            }
            count += 1;
        }
        if byte == b'\n' {
            line += 1;
        }
    }
    if !body[start..paragraph.end]
        .iter()
        .all(u8::is_ascii_whitespace)
    {
        pieces.push(Passage {
            start,
            end: paragraph.end,
            line: start_line,
        });
    }
    pieces
}

/// A passage's bytes' hash: how one that didn't change is known, kept
/// with its vector. The first 8 bytes of their SHA-256.
pub fn hash(bytes: &[u8]) -> i64 {
    let digest = link_digest::sha256_bytes(bytes);
    i64::from_le_bytes(digest[..8].try_into().expect("8 bytes"))
}

/// `vector` scaled to length 1, so a dot product is the cosine.
pub fn normalized(mut vector: Vec<f32>) -> Vec<f32> {
    let length = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length > 0.0 {
        vector.iter_mut().for_each(|x| *x /= length);
    }
    vector
}

/// The dot product of `a` and `b`, over the shorter's length. Summed in
/// 16 lanes, so the compiler vectorizes it: one running sum is a chain
/// of additions it may not reorder.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    const LANES: usize = 16;
    let n = a.len().min(b.len());
    let (a, a_rest) = a[..n].as_chunks::<LANES>();
    let (b, b_rest) = b[..n].as_chunks::<LANES>();
    let mut sums = [0.0f32; LANES];
    for (x, y) in a.iter().zip(b) {
        for ((sum, x), y) in sums.iter_mut().zip(x).zip(y) {
            *sum += x * y;
        }
    }
    let rest: f32 = a_rest.iter().zip(b_rest).map(|(x, y)| x * y).sum();
    sums.iter().sum::<f32>() + rest
}

/// Reciprocal rank fusion: each list's ranks (best first) merged, a
/// document scoring Σ 1 / (60 + rank). Each with its score and which
/// lists had it (words, meaning), best first.
pub fn fuse(words: &[i64], meaning: &[i64]) -> Vec<(i64, f64, bool, bool)> {
    const K: f64 = 60.0;
    let mut scored: Vec<(i64, f64, bool, bool)> = Vec::new();
    let mut add = |list: &[i64], is_words: bool| {
        for (rank, id) in list.iter().enumerate() {
            let score = 1.0 / (K + rank as f64 + 1.0);
            match scored.iter_mut().find(|(i, ..)| i == id) {
                Some(entry) => {
                    entry.1 += score;
                    if is_words {
                        entry.2 = true
                    } else {
                        entry.3 = true
                    }
                }
                None => scored.push((*id, score, is_words, !is_words)),
            }
        }
    };
    add(words, true);
    add(meaning, false);
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer past the most is read only one byte past it.
    #[test]
    fn an_answer_is_read_to_its_most() {
        let long = [b'x'; 100];
        assert_eq!(read_capped(&long[..], 10).unwrap().len(), 11);
        assert_eq!(read_capped(&long[..], 100).unwrap().len(), 100);
    }

    /// Each passage's text, as `body` holds it.
    fn texts<'a>(body: &'a str, passages: &[Passage]) -> Vec<(usize, &'a str)> {
        passages
            .iter()
            .map(|p| (p.line, &body[p.start..p.end]))
            .collect()
    }

    #[test]
    fn passages_split_at_headings_then_blank_lines() {
        let body = "# Title\n\nFirst paragraph.\n\nSecond.\n## You, then\n\nA turn.\n";
        let got = passages(body.as_bytes());
        assert_eq!(
            texts(body, &got),
            [
                (1, "# Title\n\nFirst paragraph.\n\nSecond."),
                (6, "## You, then\n\nA turn."),
            ]
        );
    }

    #[test]
    fn a_long_paragraph_is_cut_and_short_ones_packed() {
        let long = "x".repeat(CHUNK * 2 + 10);
        let body = format!("{long}\n\nshort\n\nshorter\n");
        let got = passages(body.as_bytes());
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(
            got.iter()
                .all(|p| characters(&body.as_bytes()[p.start..p.end]) <= CHUNK)
        );
        assert!(
            body[got[2].start..got[2].end].ends_with("short\n\nshorter"),
            "{got:?}"
        );
        assert_eq!(got[2].line, 1);
    }

    /// A cut never falls inside a character, and a carriage return ends
    /// a line as a newline does.
    #[test]
    fn a_passage_is_whole_characters() {
        let body = format!("{}\r\n\r\nnext\r\n", "é".repeat(CHUNK + 3));
        let got = passages(body.as_bytes());
        assert_eq!(got.len(), 2, "{got:?}");
        for p in &got {
            assert!(body.get(p.start..p.end).is_some(), "{p:?}");
        }
        assert_eq!(texts(&body, &got)[1].1, "ééé\r\n\r\nnext");
        let next = passages(b"a\r\n\r\nb\r\n");
        assert_eq!(texts("a\r\n\r\nb\r\n", &next), [(1, "a\r\n\r\nb")]);
    }

    #[test]
    fn fusion_ranks_both_lists_and_says_which() {
        let fused = fuse(&[1, 2], &[3, 1]);
        assert_eq!(fused[0].0, 1);
        assert!(fused[0].2 && fused[0].3);
        let three = fused.iter().find(|f| f.0 == 3).unwrap();
        assert!(!three.2 && three.3);
    }

    #[test]
    fn a_normalized_vector_has_length_one() {
        let v = normalized(vec![3.0, 4.0]);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_dot_product_in_lanes_is_the_plain_one() {
        let a: Vec<f32> = (0..1027)
            .map(|i| ((i * 7 % 13) as f32 - 6.0) / 8.0)
            .collect();
        let b: Vec<f32> = (0..1027)
            .map(|i| ((i * 5 % 11) as f32 - 5.0) / 4.0)
            .collect();
        let plain: f64 = a.iter().zip(&b).map(|(x, y)| f64::from(x * y)).sum();
        assert!((f64::from(dot(&a, &b)) - plain).abs() < 1e-3);
        assert_eq!(dot(&a[..3], &b), dot(&a[..3], &b[..3]));
    }
}
