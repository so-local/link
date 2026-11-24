//! A language server's client: one server process, spoken to over the
//! Language Server Protocol on its input and output, framed by
//! `Content-Length`.
//!
//! A thread reads what the server sends: replies to its requests, the
//! diagnostics it publishes and the work it says is in progress, and the
//! few requests a server makes of its client, each answered as a client
//! that offers nothing more than its settings (no edits, no watching of
//! files). Everything it sends is bounded ([`crate::limits`]). The server
//! runs in a process group of its own, stopped with everything it
//! started when its client is, and when this program ends, however it
//! ends, on a Mac too, where no server learns its parent ended.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::limits::limits;

/// One language server, started and initialized.
pub struct Lsp {
    child: Mutex<Child>,
    input: Arc<Mutex<ChildStdin>>,
    shared: Arc<Shared>,
    next: Mutex<i64>,
    group: i32,
    /// Whether it was stopped: its group is signalled once, while its
    /// leader, unreaped till then, keeps the group's ID its own.
    stopped: std::sync::atomic::AtomicBool,
    /// Held while a file is given, its version taken and sent as one, so
    /// a server gets each file's versions in order.
    syncing: Mutex<()>,
    /// The files under its root it's told of changes to, as last looked
    /// at; none until first looked.
    watched: Mutex<Option<HashMap<PathBuf, Seen>>>,
}

/// What's seen of a file to tell it changed: when it was last changed and
/// its length.
pub type Seen = (std::time::SystemTime, u64);

/// What the reader and the callers share.
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    /// Its settings, by section, given whole when it asks.
    settings: Value,
}

#[derive(Default)]
struct State {
    /// The requests a caller still waits on, by id: a reply to another,
    /// late or never asked, is dropped.
    waiting: HashSet<i64>,
    /// Replies not yet taken, by their request's id.
    replies: HashMap<i64, Value>,
    /// The diagnostics last published, by the file's URI.
    diagnostics: HashMap<String, Vec<Value>>,
    /// How many times each file's were published.
    published: HashMap<String, u64>,
    /// The work it says is in progress, by its token.
    progress: HashSet<String>,
    /// Whether it's quiescent, for a server that says
    /// (`experimental/serverStatus`); none, it never said.
    quiescent: Option<bool>,
    /// When it last published or said what it's doing.
    last: Option<Instant>,
    /// Files opened, by URI: their version, language and the text it was
    /// given.
    opened: HashMap<String, (i64, String, String)>,
    /// Why it ended, once it did.
    ended: Option<String>,
    /// The end of what it wrote on its errors.
    errors: VecDeque<u8>,
    /// Whether its errors have closed: all it wrote there is in `errors`.
    errors_closed: bool,
}

impl Lsp {
    /// Starts `argv` in `root`, programs found on `path`, and initializes
    /// it for the workspace `root` with `settings` (by section), by
    /// `deadline`.
    pub fn start(
        argv: &[String],
        root: &Path,
        path: &str,
        settings: Value,
        deadline: Instant,
    ) -> anyhow::Result<Lsp> {
        use std::os::unix::process::CommandExt;
        let program = argv.first().context("no server to start")?;
        let mut command = link_secretive::process::command(program);
        command
            .args(&argv[1..])
            .current_dir(root)
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        // The server and its group die with this program, however it ends.
        link_secretive::process::watched_by_parent(&mut command, libc::SIGKILL)
            .context("watching the server")?;
        let mut child = command
            .spawn()
            .with_context(|| format!("starting {program}"))?;
        let group = child.id() as i32;
        let input = Arc::new(Mutex::new(child.stdin.take().context("its input")?));
        let output = child.stdout.take().context("its output")?;
        let errors = child.stderr.take().context("its errors")?;
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: Condvar::new(),
            settings,
        });
        {
            let shared = shared.clone();
            std::thread::spawn(move || keep_errors(errors, &shared));
        }
        {
            let (shared, input) = (shared.clone(), input.clone());
            let root = root.to_path_buf();
            std::thread::spawn(move || read(output, &shared, &input, &root));
        }
        let lsp = Lsp {
            child: Mutex::new(child),
            input,
            shared,
            next: Mutex::new(0),
            group,
            stopped: Default::default(),
            syncing: Mutex::new(()),
            watched: Mutex::new(None),
        };
        let uri = uri_of(root);
        let name = root
            .file_name()
            .map_or("files".into(), |n| n.to_string_lossy().into_owned());
        let initialized = lsp.request(
            "initialize",
            json!({
                // None: a server in the sandbox's own process namespace
                // can't see this one, and one that polls for it would end.
                "processId": Value::Null,
                "clientInfo": {"name": "link-code", "version": env!("CARGO_PKG_VERSION")},
                "rootUri": uri,
                "rootPath": root.display().to_string(),
                "workspaceFolders": [{"uri": uri, "name": name}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-16"]},
                    "textDocument": {
                        "synchronization": {"didSave": true},
                        "publishDiagnostics": {"relatedInformation": false},
                        "definition": {"linkSupport": true},
                        "references": {},
                    },
                    "workspace": {
                        "symbol": {},
                        "configuration": true,
                        "didChangeConfiguration": {"dynamicRegistration": false},
                        // A server's registration to have files watched is
                        // refused: it's told of changes under its root
                        // instead ([`Lsp::look`]).
                        "didChangeWatchedFiles": {"dynamicRegistration": false},
                        "workspaceFolders": true,
                    },
                    "window": {"workDoneProgress": true},
                    "experimental": {"serverStatusNotification": true},
                },
            }),
            deadline,
        );
        if let Err(err) = initialized {
            lsp.stop();
            return Err(err);
        }
        lsp.notify("initialized", json!({}))?;
        // Its settings told, which a server that pulls them (rust-analyzer)
        // asks for again, by section.
        if lsp
            .shared
            .settings
            .as_object()
            .is_some_and(|s| !s.is_empty())
        {
            let settings = lsp.shared.settings.clone();
            lsp.notify(
                "workspace/didChangeConfiguration",
                json!({"settings": settings}),
            )?;
        }
        Ok(lsp)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Why it ended, if it did: the end of its errors.
    pub fn ended(&self) -> Option<String> {
        self.state().ended.clone()
    }

    /// Sends `message`; a server that ended is said to have stopped, and
    /// why, rather than a broken pipe.
    fn send(&self, message: &Value) -> anyhow::Result<()> {
        let written = {
            let mut input = self.input.lock().unwrap_or_else(|e| e.into_inner());
            write_message(&mut *input, message)
        };
        let Err(err) = written else {
            return Ok(());
        };
        // Its end is told once its errors are read, soon after its pipe broke.
        let state = self.state();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, limits().errors_wait(), |state| state.ended.is_none())
            .unwrap_or_else(|e| e.into_inner());
        match &state.ended {
            Some(why) => bail!("the language server stopped: {why}"),
            None => Err(err).context("writing to the language server"),
        }
    }

    /// Sends notification `method`.
    pub fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Asks `method` and waits for its result until `deadline`: an error
    /// it answers with, its end or the deadline is an error.
    pub fn request(&self, method: &str, params: Value, deadline: Instant) -> anyhow::Result<Value> {
        let id = {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            *next += 1;
            *next
        };
        self.state().waiting.insert(id);
        let asked =
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut state = self.state();
        if let Err(err) = asked {
            state.waiting.remove(&id);
            return Err(err);
        }
        let answered = loop {
            if let Some(reply) = state.replies.remove(&id) {
                break match reply.get("error") {
                    Some(error) => {
                        let message = error.get("message").and_then(Value::as_str).unwrap_or("");
                        Err(anyhow::anyhow!(
                            "the language server answered {method} with an error:\n{}",
                            server_said(message)
                        ))
                    }
                    None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
                };
            }
            if let Some(why) = &state.ended {
                break Err(anyhow::anyhow!("the language server stopped: {why}"));
            }
            let now = Instant::now();
            if now >= deadline {
                // No longer waited on: its late reply is dropped.
                state.waiting.remove(&id);
                drop(state);
                let _ = self.notify("$/cancelRequest", json!({"id": id}));
                bail!("the language server didn't answer {method} in time");
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        state.waiting.remove(&id);
        answered
    }

    /// The files it was given, by path.
    pub fn opened(&self) -> Vec<PathBuf> {
        let mut opened: Vec<PathBuf> = self
            .state()
            .opened
            .keys()
            .filter_map(|uri| path_of(uri))
            .collect();
        opened.sort();
        opened
    }

    /// Gives it file `path`, of `language`, as `text`, read from disk:
    /// opened the first time, changed and saved when it changed since. How
    /// many times its diagnostics were published before, to wait past;
    /// none when it was given this text already, so nothing new is to be
    /// published.
    pub fn sync(&self, path: &Path, language: &str, text: String) -> anyhow::Result<Option<u64>> {
        let uri = uri_of(path);
        let _syncing = self.syncing.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state();
        let before = state.published.get(&uri).copied().unwrap_or(0);
        let message = match state.opened.get(&uri) {
            Some((_, _, kept)) if *kept == text => return Ok(None),
            Some((version, _, _)) => {
                let version = version + 1;
                state
                    .opened
                    .insert(uri.clone(), (version, language.to_string(), text.clone()));
                Some(version)
            }
            None => {
                state
                    .opened
                    .insert(uri.clone(), (1, language.to_string(), text.clone()));
                None
            }
        };
        // Its state is the reader's too: none held while writing, only the
        // lock that keeps versions in order.
        drop(state);
        match message {
            None => self.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": language, "version": 1, "text": text}}),
            )?,
            Some(version) => {
                self.notify(
                    "textDocument/didChange",
                    json!({"textDocument": {"uri": uri, "version": version},
                        "contentChanges": [{"text": text}]}),
                )?;
                self.notify("textDocument/didSave", json!({"textDocument": {"uri": uri}}))?;
            }
        }
        Ok(Some(before))
    }

    /// Tells it of the files under its root changed since last looked, as
    /// `scan` sees them now: created, changed and deleted
    /// (`workspace/didChangeWatchedFiles`). It's refused watching them
    /// itself, so a file it indexed but wasn't given is seen to change
    /// only so. The first look only takes them in.
    pub fn look(&self, scan: impl FnOnce() -> HashMap<PathBuf, Seen>) -> anyhow::Result<()> {
        let mut watched = self.watched.lock().unwrap_or_else(|e| e.into_inner());
        let now = scan();
        let Some(before) = watched.replace(now.clone()) else {
            return Ok(());
        };
        // The kinds of change, as the protocol numbers them.
        const CREATED: u8 = 1;
        const CHANGED: u8 = 2;
        const DELETED: u8 = 3;
        let mut changes: Vec<(&PathBuf, u8)> = Vec::new();
        for (path, seen) in &now {
            match before.get(path) {
                None => changes.push((path, CREATED)),
                Some(was) if was != seen => changes.push((path, CHANGED)),
                Some(_) => {}
            }
        }
        changes.extend(
            before
                .keys()
                .filter(|path| !now.contains_key(*path))
                .map(|path| (path, DELETED)),
        );
        if changes.is_empty() {
            return Ok(());
        }
        changes.sort();
        let changes: Vec<Value> = changes
            .into_iter()
            .map(|(path, kind)| json!({"uri": uri_of(path), "type": kind}))
            .collect();
        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": changes}),
        )
    }

    /// Closes file `path`, gone from disk since it was given.
    pub fn close(&self, path: &Path) -> anyhow::Result<()> {
        let uri = uri_of(path);
        {
            // A file gone has no problems left to tell.
            let mut state = self.state();
            state.diagnostics.remove(&uri);
            if state.opened.remove(&uri).is_none() {
                return Ok(());
            }
        }
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
    }

    /// The language `path` was given as, if it was.
    pub fn language_of(&self, path: &Path) -> Option<String> {
        self.state()
            .opened
            .get(&uri_of(path))
            .map(|(_, language, _)| language.clone())
    }

    /// Waits until it's quiet with no work in progress, and, for `file`
    /// (its URI and the count [`Lsp::sync`] gave), until it published past
    /// that count or stayed quiet `unpublished-ms`; or until `deadline`.
    /// Whether it settled.
    pub fn settle(&self, file: Option<(&str, u64)>, deadline: Instant) -> bool {
        let started = Instant::now();
        let mut state = self.state();
        loop {
            if state.ended.is_some() {
                return false;
            }
            let now = Instant::now();
            let quiet_since = state.last.unwrap_or(started).max(started);
            let idle = state.progress.is_empty() && state.quiescent != Some(false);
            let quiet = now.duration_since(quiet_since);
            let published = match file {
                None => true,
                Some((uri, before)) => {
                    state.published.get(uri).copied().unwrap_or(0) > before
                        || quiet >= limits().unpublished()
                }
            };
            if idle && quiet >= limits().quiet() && published {
                return true;
            }
            if now >= deadline {
                return false;
            }
            let wait = (deadline - now).min(limits().quiet());
            state = self
                .shared
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Every file's diagnostics as last published, by URI.
    pub fn diagnostics(&self) -> Vec<(String, Vec<Value>)> {
        let mut all: Vec<_> = self
            .state()
            .diagnostics
            .iter()
            .map(|(uri, d)| (uri.clone(), d.clone()))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    /// Asks it to shut down, then stops its group: what it started too.
    #[expect(
        clippy::disallowed_methods,
        reason = "polls a stopping server until it ends"
    )]
    pub fn stop(&self) {
        if self.stopped.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if self.ended().is_none() {
            let deadline = Instant::now() + limits().stop();
            if self.request("shutdown", Value::Null, deadline).is_ok() {
                let _ = self.notify("exit", Value::Null);
            }
            let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(limits().stop_poll());
            }
        }
        // Never group 1 or below: `kill(-1)` is every process.
        if self.group > 1 {
            // SAFETY: a signal to the process group the server leads.
            unsafe { libc::kill(-self.group, libc::SIGKILL) };
        }
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Keeps the end of what the server writes on its errors, and says when
/// they close.
fn keep_errors(mut errors: impl Read, shared: &Shared) {
    let mut buffer = [0u8; 4096];
    while let Ok(n @ 1..) = errors.read(&mut buffer) {
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.errors.extend(&buffer[..n]);
        let over = state.errors.len().saturating_sub(limits().errors_kept);
        state.errors.drain(..over);
    }
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    state.errors_closed = true;
    shared.changed.notify_all();
}

/// Reads what the server sends until it ends, answering its requests on
/// `input`.
fn read(output: impl Read, shared: &Shared, input: &Mutex<ChildStdin>, root: &Path) {
    let settings = &shared.settings;
    let mut output = BufReader::new(output);
    let why = loop {
        let message = match read_message(&mut output) {
            Ok(Some(message)) => message,
            Ok(None) => break "it ended".to_string(),
            Err(err) => break format!("{err:#}"),
        };
        let method = message.get("method").and_then(Value::as_str);
        match (method, message.get("id")) {
            // A reply to one of ours a caller waits on: ours are numbers.
            (None, Some(id)) => {
                if let Some(id) = id.as_i64() {
                    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    if state.waiting.contains(&id) {
                        state.replies.insert(id, message);
                        shared.changed.notify_all();
                    }
                }
            }
            (Some(method), Some(id)) => {
                let reply = answer(method, message.get("params"), root, settings, id.clone());
                let mut input = input.lock().unwrap_or_else(|e| e.into_inner());
                let _ = write_message(&mut *input, &reply);
            }
            (Some(method), None) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                if heard(&mut state, method, params) {
                    state.last = Some(Instant::now());
                    shared.changed.notify_all();
                }
            }
            (None, None) => {}
        }
    };
    // Its output and its errors close apart: why it ended is the end of
    // its errors, so they're read to their close first. A process it left
    // behind can hold them open, so not for longer than `errors-wait-ms`.
    let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    let (mut state, _) = shared
        .changed
        .wait_timeout_while(state, limits().errors_wait(), |state| !state.errors_closed)
        .unwrap_or_else(|e| e.into_inner());
    let errors = String::from_utf8_lossy(state.errors.make_contiguous())
        .trim()
        .to_string();
    state.ended = Some(match errors.is_empty() {
        true => why,
        false => format!("{why}; it said:\n{}", server_said(&errors)),
    });
    shared.changed.notify_all();
}

/// `text`, its first `most` characters and an ellipsis when it's longer.
pub(crate) fn capped(text: &str, most: usize) -> String {
    match text.chars().count() > most {
        true => text.chars().take(most).collect::<String>() + "…",
        false => text.to_string(),
    }
}

/// `text` a language server sent, capped at `message-max` characters
/// and fenced as a program's output: a server's errors can hold what
/// the project's build scripts and macros print.
pub(crate) fn server_said(text: &str) -> String {
    let text = capped(text, limits().message_max);
    link_untrusted::fence(
        link_untrusted::Source {
            kind: link_untrusted::Kind::Output,
            name: "the language server",
        },
        &text,
    )
    .unwrap_or_else(|_| "(withheld: no randomness to fence it)".to_string())
}

/// Takes notification `method` in; whether it says what the server is
/// doing (published, or work begun or ended).
fn heard(state: &mut State, method: &str, params: Value) -> bool {
    match method {
        "textDocument/publishDiagnostics" => {
            let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                return false;
            };
            // Named as this client names it, however the server encoded it.
            let uri = normal(uri);
            let mut diagnostics = match params.get("diagnostics") {
                Some(Value::Array(list)) => list.clone(),
                _ => Vec::new(),
            };
            diagnostics.truncate(limits().diagnostics_max);
            *state.published.entry(uri.clone()).or_default() += 1;
            match diagnostics.is_empty() {
                true => state.diagnostics.remove(&uri),
                false => state.diagnostics.insert(uri, diagnostics),
            };
            true
        }
        "$/progress" => {
            let token = match params.get("token") {
                Some(Value::String(token)) => token.clone(),
                Some(token) => token.to_string(),
                None => return false,
            };
            match params.pointer("/value/kind").and_then(Value::as_str) {
                // Past the most followed, more work is taken as the same
                // work: it's still in progress.
                Some("begin") if state.progress.len() < limits().progress_max => {
                    state.progress.insert(token);
                }
                Some("end") => {
                    state.progress.remove(&token);
                }
                _ => {}
            }
            true
        }
        "experimental/serverStatus" => {
            state.quiescent = params.get("quiescent").and_then(Value::as_bool);
            true
        }
        _ => false,
    }
}

/// The reply to the server's request `method`, as a client offering
/// nothing beyond `settings` and what it asked at its start.
fn answer(method: &str, params: Option<&Value>, root: &Path, settings: &Value, id: Value) -> Value {
    let result = match method {
        // Each section asked, from its settings: the server's default
        // (null) where they have none.
        "workspace/configuration" => {
            let items = params
                .and_then(|p| p.get("items"))
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            Value::Array(
                items
                    .iter()
                    .map(|item| section(settings, item.get("section").and_then(Value::as_str)))
                    .collect(),
            )
        }
        "workspace/workspaceFolders" => {
            json!([{"uri": uri_of(root), "name": "files"}])
        }
        // It watches no files: a registration to have them watched is
        // refused; it's told of changes under its root instead.
        "client/registerCapability" if watching(params) => {
            return json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32803, "message": "this client watches no files"}});
        }
        "window/workDoneProgress/create"
        | "client/registerCapability"
        | "client/unregisterCapability"
        | "window/showMessageRequest"
        | "workspace/semanticTokens/refresh"
        | "workspace/inlayHint/refresh"
        | "workspace/codeLens/refresh"
        | "workspace/diagnostic/refresh" => Value::Null,
        // It reads; it never changes a file.
        "workspace/applyEdit" => json!({"applied": false}),
        other => {
            return json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": format!("no method {other}")}});
        }
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Whether a registration asks to have files watched.
fn watching(params: Option<&Value>) -> bool {
    params
        .and_then(|p| p.get("registrations"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|r| r.get("method").and_then(Value::as_str) == Some("workspace/didChangeWatchedFiles"))
}

/// Section `name` (dotted: `rust-analyzer.check`) of `settings`; all of
/// them for none, and null where they have none.
fn section(settings: &Value, name: Option<&str>) -> Value {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return settings.clone();
    };
    name.split('.')
        .try_fold(settings, |value, part| value.get(part))
        .cloned()
        .unwrap_or(Value::Null)
}

/// `uri` as [`uri_of`] names its path; another URI as it is.
fn normal(uri: &str) -> String {
    match path_of(uri) {
        Some(path) => uri_of(&path),
        None => uri.to_string(),
    }
}

/// Writes one message, framed.
pub fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// Reads one message; none at the end of `input`. A header line or a body
/// past its limit is an error.
pub fn read_message(input: &mut impl BufRead) -> anyhow::Result<Option<Value>> {
    let mut length = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        let most = limits().header_max as u64 + 1;
        if input.by_ref().take(most).read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        if line.last() != Some(&b'\n') && line.len() as u64 == most {
            bail!("a header line past {} bytes", limits().header_max);
        }
        let header = String::from_utf8_lossy(&line);
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            match length {
                Some(_) => break,
                // Blank lines before a header: nothing yet.
                None => continue,
            }
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let n: usize = value
                .trim()
                .parse()
                .context("a Content-Length that isn't a number")?;
            let most = limits().body_max;
            if n > most {
                bail!("a message of {n} bytes, past {most}");
            }
            length = Some(n);
        }
    }
    let mut body = vec![0; length.unwrap_or(0)];
    input.read_exact(&mut body).context("a message cut short")?;
    Ok(Some(
        serde_json::from_slice(&body).context("a message that isn't JSON")?,
    ))
}

/// `path`'s `file:` URI, each byte but the unreserved and `/` escaped.
pub fn uri_of(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(b as char)
            }
            _ => uri.push_str(&format!("%{b:02X}")),
        }
    }
    uri
}

/// The path of `file:` URI `uri`; none for any other.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let rest = uri.strip_prefix("file://")?;
    // `file://host/path`: only the local host's.
    let rest = match rest.strip_prefix("localhost") {
        Some(rest) => rest,
        None => rest,
    };
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = rest.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_and_its_uri_round_trip() {
        for path in ["/a/b.rs", "/with space/ü.rs", "/odd%name#1?.rs"] {
            let uri = uri_of(Path::new(path));
            assert!(!uri[7..].contains([' ', '#', '?']), "{uri}");
            assert_eq!(path_of(&uri).unwrap(), Path::new(path));
        }
        assert_eq!(path_of("file:///x%20y").unwrap(), Path::new("/x y"));
        assert_eq!(path_of("file://localhost/x").unwrap(), Path::new("/x"));
        assert!(path_of("https://example.com/x").is_none());
        assert!(path_of("file://elsewhere/x").is_none());
    }

    #[test]
    fn messages_are_framed_by_their_length() {
        let mut out = Vec::new();
        write_message(&mut out, &json!({"a": "é"})).unwrap();
        write_message(&mut out, &json!({"b": 2})).unwrap();
        let mut input = std::io::Cursor::new(out);
        assert_eq!(
            read_message(&mut input).unwrap().unwrap(),
            json!({"a": "é"})
        );
        assert_eq!(read_message(&mut input).unwrap().unwrap(), json!({"b": 2}));
        assert!(read_message(&mut input).unwrap().is_none());
        let huge = format!("Content-Length: {}\r\n\r\n", limits().body_max + 1);
        assert!(read_message(&mut std::io::Cursor::new(huge)).is_err());
        let cut = "Content-Length: 10\r\n\r\n{}";
        assert!(read_message(&mut std::io::Cursor::new(cut)).is_err());
        // A header line without its end is bounded, never read whole.
        let endless = "X".repeat(limits().header_max * 4);
        let err = read_message(&mut std::io::Cursor::new(endless)).unwrap_err();
        assert!(format!("{err:#}").contains("header line past"), "{err:#}");
    }

    /// A reply no caller waits on, late or never asked, is dropped; work
    /// in progress and a file's diagnostics are bounded.
    #[test]
    fn what_a_server_sends_is_bounded() {
        let mut state = State::default();
        for token in 0..limits().progress_max * 2 {
            heard(
                &mut state,
                "$/progress",
                json!({"token": token, "value": {"kind": "begin"}}),
            );
        }
        assert_eq!(state.progress.len(), limits().progress_max);
        let many: Vec<Value> = (0..limits().diagnostics_max + 5)
            .map(|n| json!({"message": n.to_string()}))
            .collect();
        heard(
            &mut state,
            "textDocument/publishDiagnostics",
            json!({"uri": "file:///a.rs", "diagnostics": many}),
        );
        assert_eq!(
            state.diagnostics["file:///a.rs"].len(),
            limits().diagnostics_max
        );
    }

    /// Diagnostics published under another encoding of a file's URI are
    /// that file's.
    #[test]
    fn a_file_is_one_file_however_its_uri_is_encoded() {
        let mut state = State::default();
        heard(
            &mut state,
            "textDocument/publishDiagnostics",
            json!({"uri": "file:///w/%61%20b.rs", "diagnostics": [{"message": "x"}]}),
        );
        let ours = uri_of(Path::new("/w/a b.rs"));
        assert_eq!(state.diagnostics[&ours].len(), 1);
        assert_eq!(state.published[&ours], 1);
    }

    #[test]
    fn work_in_progress_and_diagnostics_are_followed() {
        let mut state = State::default();
        assert!(heard(
            &mut state,
            "$/progress",
            json!({"token": 1, "value": {"kind": "begin"}})
        ));
        assert_eq!(state.progress.len(), 1);
        heard(
            &mut state,
            "$/progress",
            json!({"token": 1, "value": {"kind": "end"}}),
        );
        assert!(state.progress.is_empty());
        let uri = "file:///a.rs";
        heard(
            &mut state,
            "textDocument/publishDiagnostics",
            json!({"uri": uri, "diagnostics": [{"message": "x"}]}),
        );
        assert_eq!(state.diagnostics[uri].len(), 1);
        heard(
            &mut state,
            "textDocument/publishDiagnostics",
            json!({"uri": uri, "diagnostics": []}),
        );
        assert!(!state.diagnostics.contains_key(uri));
        assert_eq!(state.published[uri], 2);
        assert!(!heard(&mut state, "window/logMessage", json!({})));
    }

    #[test]
    fn a_server_s_requests_are_answered_as_a_client_offering_its_settings() {
        let root = Path::new("/r");
        let none = json!({});
        let config = answer(
            "workspace/configuration",
            Some(&json!({"items": [{}, {}]})),
            root,
            &none,
            json!(3),
        );
        assert_eq!(
            config,
            json!({"jsonrpc": "2.0", "id": 3, "result": [{}, {}]})
        );
        // Each section asked, from the settings, dotted or whole.
        let settings = json!({"rust-analyzer": {"workspace": {"symbol": {"search": {"kind": "all_symbols"}}}}});
        let config = answer(
            "workspace/configuration",
            Some(&json!({"items": [
                {"section": "rust-analyzer"},
                {"section": "rust-analyzer.workspace.symbol.search.kind"},
                {"section": "gopls"},
            ]})),
            root,
            &settings,
            json!(4),
        );
        assert_eq!(
            config["result"],
            json!([settings["rust-analyzer"], "all_symbols", null])
        );
        let edit = answer("workspace/applyEdit", None, root, &none, json!("e"));
        assert_eq!(edit["result"]["applied"], false);
        let unknown = answer("custom/thing", None, root, &none, json!(4));
        assert_eq!(unknown["error"]["code"], -32601);
        // Watching files is refused; another registration is taken.
        let watch = json!({"registrations": [
            {"id": "1", "method": "workspace/didChangeWatchedFiles", "registerOptions": {}}]});
        let refused = answer(
            "client/registerCapability",
            Some(&watch),
            root,
            &none,
            json!(5),
        );
        assert_eq!(refused["error"]["code"], -32803);
        let other = json!({"registrations": [{"id": "2", "method": "textDocument/didSave"}]});
        let taken = answer(
            "client/registerCapability",
            Some(&other),
            root,
            &none,
            json!(6),
        );
        assert_eq!(taken["result"], Value::Null);
    }
}
