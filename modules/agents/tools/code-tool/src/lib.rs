//! Link code: an MCP server over stdio offering `diagnostics`,
//! `definition` and `references`, which ask the project's language
//! servers what is wrong in a file and where a name is defined and used.
//!
//! It starts a language server ([`lsp`]) for a language the first time a
//! file of it is asked about, in the nearest folder above the file that
//! marks a project's root, and keeps it while its session lasts. Each
//! server runs under the sandbox it's given (`lnk sandbox run` and its
//! flags, from the agent's sandbox file, no network): a server runs the
//! project's code, its build scripts and macros. It reads only files in
//! the files folder, and shows a line of a file only from there, each
//! opened once by its handle, never through a link.

pub mod cli;
pub mod limits;
pub mod lsp;
mod symbols;

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use limits::limits;
use link_agent_spec::CodeServer;
use lsp::{Lsp, capped};
use serde_json::{Value, json};

/// What it runs with.
pub struct Config {
    /// The agent's files folder: the only files it asks about.
    pub files: PathBuf,
    /// The PATH a server's program is found on.
    pub path: String,
    /// The sandbox each server runs under: a program and its arguments,
    /// to which `--`, the server and its arguments are added. Empty:
    /// none, for tests.
    pub sandbox: Vec<String>,
    /// The language servers, by language.
    pub servers: BTreeMap<String, CodeServer>,
    /// The languages whose server it may not start, and why (it isn't on
    /// this machine, the sandbox file doesn't allow it).
    pub unavailable: BTreeMap<String, String>,
    /// How long a call waits on a server, its start included.
    pub timeout: Duration,
    /// The variables each server gets, set inside its sandbox, which clears
    /// the environment: those `run` gives its commands, so a server sees
    /// the dependencies `run` fetched.
    pub env: Vec<(String, String)>,
}

pub use link_code_tool_contract::env_program;

/// The tool: its settings and the servers it started, by language and
/// root.
pub struct Code {
    config: Config,
    /// The files folder as the servers name it, links resolved.
    files: PathBuf,
    /// The home folder, links resolved, a path outside the files folder
    /// shown from it (`~/...`); none, shown whole.
    home: Option<PathBuf>,
    /// Each server's place, by language and root: held only to find it, so
    /// one server starting never holds up a call to another.
    running: Mutex<HashMap<(String, PathBuf), Arc<Slot>>>,
}

/// A server's place: empty until it starts, its lock held while it does.
type Slot = Mutex<Option<Arc<Lsp>>>;

impl Code {
    pub fn new(config: Config) -> anyhow::Result<Code> {
        let files = config
            .files
            .canonicalize()
            .with_context(|| format!("no files folder {}", config.files.display()))?;
        Ok(Code {
            config,
            files,
            home: link_files::try_home_dir().ok(),
            running: Mutex::new(HashMap::new()),
        })
    }

    /// Stops every server it started.
    pub fn stop(&self) {
        let slots: Vec<_> = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_, slot)| slot)
            .collect();
        for slot in slots {
            let lsp = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(lsp) = lsp {
                lsp.stop();
            }
        }
    }
}

/// Answers the messages on `input` on `output`, until `input` ends: each
/// call on a thread of its own. Then stops the servers it started.
pub fn serve(code: &Code, input: impl BufRead, output: impl Write + Send) -> anyhow::Result<()> {
    let output = Mutex::new(output);
    let served = std::thread::scope(|scope| -> anyhow::Result<()> {
        for line in input.lines() {
            let line = line.context("reading a message")?;
            if line.trim().is_empty() {
                continue;
            }
            let output = &output;
            scope.spawn(move || {
                if let Some(reply) = answer(code, &line) {
                    let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = writeln!(output, "{reply}");
                    let _ = output.flush();
                }
            });
        }
        Ok(())
    });
    code.stop();
    served
}

/// The reply to message `line`: none for a notification.
pub fn answer(code: &Code, line: &str) -> Option<Value> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "not JSON".into()));
    };
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let versions = &limits().versions;
            let version = versions
                .iter()
                .find(|v| Some(v.as_str()) == asked)
                .unwrap_or(&versions[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "link-code", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": instructions(code),
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => match offered(code) {
            Ok(tools) => result(id, json!({"tools": tools})),
            Err(why) => error(id, -32603, format!("{why:#}")),
        },
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            if !link_code_tool_contract::calls().is_ok_and(|calls| calls.iter().any(|c| c == name))
            {
                return Some(error(id, -32602, format!("no tool {name:?}")));
            }
            // Its arguments, refused by their declaration before its handler.
            if let Err(refused) = crate::cli::check_call(name, &arguments) {
                return Some(result(
                    id,
                    json!({"content": [{"type": "text", "text": refused.to_string()}], "isError": true}),
                ));
            }
            let deadline = Instant::now() + code.config.timeout;
            let called = match name {
                "diagnostics" => diagnostics(code, &arguments, deadline),
                _ => places(code, name, &arguments, deadline),
            };
            match called {
                Ok((text, value)) => result(
                    id,
                    json!({"content": [{"type": "text", "text": text}],
                        "structuredContent": value}),
                ),
                Err(err) => result(
                    id,
                    json!({"content": [{"type": "text", "text": format!("{err:#}")}],
                        "isError": true}),
                ),
            }
        }
        other => error(id, -32601, format!("no method {other:?}")),
    })
}

/// The languages it serves here, and those it doesn't and why.
fn served(code: &Code) -> (Vec<String>, Vec<String>) {
    let mut ready = Vec::new();
    let mut not = Vec::new();
    for (language, server) in &code.config.servers {
        let Some(program) = server.command().first() else {
            continue;
        };
        match code.config.unavailable.get(language) {
            Some(why) => not.push(format!("{language} ({program}: {why})")),
            None => ready.push(format!("{language} ({program})")),
        }
    }
    (ready, not)
}

/// What it tells the model it's for.
fn instructions(code: &Code) -> String {
    let (ready, not) = served(code);
    let mut text = "Ask the project's language servers rather than searching text: what's wrong \
        in a file ('diagnostics'), where a name is defined ('definition') and where it's used \
        ('references'). A server starts the first time its language is asked about, which can \
        take a while in a large project; ask again once you change a file."
        .to_string();
    match ready.is_empty() {
        true => text.push_str(" No language server runs here."),
        false => text.push_str(&format!(" Servers here: {}.", ready.join(", "))),
    }
    if !not.is_empty() {
        text.push_str(&format!(" Not here: {}.", not.join(", ")));
    }
    text
}

/// The calls `actions.toml` declares, as its contract renders them.
const TEMPLATE: &str = link_code_tool_contract::TOOLS;

/// Its tools, from `actions.toml`, each saying the languages it serves.
fn offered(code: &Code) -> anyhow::Result<Value> {
    use link_actions::tools::{Fill, Fills};
    let (ready, _) = served(code);
    let languages = match ready.is_empty() {
        true => "no language yet".to_string(),
        false => ready.join(", "),
    };
    let fills = Fills {
        values: [("languages", Fill::Text(languages))].into(),
        ..Fills::default()
    };
    link_actions::tools::offer(TEMPLATE, &fills)
}

/// A file asked about: where it is, its text, its language and the server
/// that serves it.
struct Asked {
    path: PathBuf,
    /// Its text, read once by its handle: what the server is given and
    /// positions are counted in.
    text: String,
    language: String,
    server: CodeServer,
}

/// The file `name`, a path from the files folder: refused outside it, or
/// when no server serves its language here, saying which would.
fn asked(code: &Code, name: &str) -> anyhow::Result<Asked> {
    let joined = code.files.join(name);
    let path = joined
        .canonicalize()
        .with_context(|| format!("no file {name} in your files folder"))?;
    ensure!(
        path.starts_with(&code.files),
        "{name} isn't in your files folder"
    );
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let found = code
        .config
        .servers
        .iter()
        .find(|(_, server)| server.files().iter().any(|e| e == extension));
    let Some((language, server)) = found else {
        bail!("no language server serves .{extension} files here");
    };
    let Some(program) = server.command().first() else {
        bail!("no language server serves {language} here: its command is empty");
    };
    if let Some(why) = code.config.unavailable.get(language) {
        bail!("no language server for {language} here: it would run {program}, but {why}");
    }
    // Read by its handle, through no link: a link swapped in since it was
    // checked is refused, never followed.
    let text = read_beneath(code, &path).with_context(|| format!("reading {name}"))?;
    Ok(Asked {
        path,
        text,
        language: language.clone(),
        server: server.clone(),
    })
}

/// The text of `path`, a file in the files folder, opened folder by folder
/// from the files folder's handle, never through a link; refused outside
/// it, not a regular file, past `file-max` bytes or not text.
fn read_beneath(code: &Code, path: &Path) -> anyhow::Result<String> {
    use std::path::Component;
    let relative = path
        .strip_prefix(&code.files)
        .ok()
        .context("it isn't in your files folder")?;
    let mut parts: Vec<&std::ffi::OsStr> = Vec::new();
    for part in relative.components() {
        match part {
            Component::Normal(name) => parts.push(name),
            _ => bail!("it isn't in your files folder"),
        }
    }
    let name = parts.pop().context("it isn't a file")?;
    let mut dir = link_files::nofollow::Dir::open(&code.files)?;
    for folder in parts {
        // A link where a folder was is refused as ENOTDIR on Linux.
        dir = dir.folder(folder).map_err(|err| match err.raw_os_error() {
            Some(libc::ENOTDIR) => anyhow::anyhow!("a link or a file on its way, never followed"),
            _ => refused(err),
        })?;
    }
    let bytes = dir.read(name, limits().file_max).map_err(refused)?;
    String::from_utf8(bytes).ok().context("it isn't text")
}

/// `err`, a link refused or a file that isn't one, said so.
fn refused(err: std::io::Error) -> anyhow::Error {
    match link_files::nofollow::is_link(&err) {
        true => anyhow::anyhow!("a link on its way, never followed"),
        false if err.kind() == std::io::ErrorKind::InvalidData => {
            anyhow::anyhow!("it isn't a file, or it's past {} bytes", limits().file_max)
        }
        false => err.into(),
    }
}

/// The project root of `file`: the nearest folder above it, in the files
/// folder, holding one of `server`'s root files; none, the files folder.
fn root_of(code: &Code, file: &Path, server: &CodeServer) -> PathBuf {
    let mut folder = file.parent();
    while let Some(dir) = folder {
        if !dir.starts_with(&code.files) {
            break;
        }
        if server.roots().iter().any(|root| dir.join(root).exists()) {
            return dir.to_path_buf();
        }
        folder = dir.parent();
    }
    code.files.clone()
}

/// The server of `asked`'s language for its root, started if it isn't,
/// and given `asked` and every file it was given before as each is now,
/// and told of the files changed under its root. Only a call to the same
/// server waits on its start. How many times `asked`'s
/// diagnostics were published before, none when it was given unchanged.
fn server(
    code: &Code,
    asked: &Asked,
    deadline: Instant,
) -> anyhow::Result<(Arc<Lsp>, Option<u64>)> {
    let root = root_of(code, &asked.path, &asked.server);
    let key = (asked.language.clone(), root.clone());
    let slot = code
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key)
        .or_default()
        .clone();
    let lsp = {
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        match slot.as_ref().filter(|lsp| lsp.ended().is_none()) {
            Some(lsp) => lsp.clone(),
            // Not started, or stopped since: started again.
            None => {
                let program = &asked.server.command()[0];
                let found =
                    link_harness_contract::harness::find_program(program, &code.config.path)
                        .with_context(|| format!("{program} isn't on this machine"))?;
                let argv = server_argv(&code.config, &found, &asked.server.command()[1..]);
                let lsp = Lsp::start(
                    &argv,
                    &root,
                    &code.config.path,
                    asked.server.settings(),
                    deadline,
                )
                .with_context(|| format!("starting {program} for {}", asked.language))?;
                let lsp = Arc::new(lsp);
                *slot = Some(lsp.clone());
                lsp
            }
        }
    };
    refresh(code, &lsp, Some(&asked.path))?;
    lsp.look(|| watched(code, &root, &asked.server))?;
    let before = lsp.sync(&asked.path, &asked.language, asked.text.clone())?;
    Ok((lsp, before))
}

/// The files under `root` that `server` serves or that mark its roots, by
/// path, as last changed: in the files folder, through no link, not in an
/// unwatched folder, and no more than `watched-max`, the first by name.
fn watched(code: &Code, root: &Path, server: &CodeServer) -> HashMap<PathBuf, lsp::Seen> {
    let mut seen = HashMap::new();
    if !root.starts_with(&code.files) {
        return seen;
    }
    let wanted = |path: &Path| {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        server.files().iter().any(|f| f == extension) || server.roots().iter().any(|r| r == name)
    };
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        for entry in entries {
            // Its own, never what a link names.
            let Ok(meta) = entry.path().symlink_metadata() else {
                continue;
            };
            let path = entry.path();
            if meta.is_dir() {
                let name = entry.file_name();
                if !limits().unwatched.iter().any(|u| *name == **u) {
                    folders.push(path);
                }
            } else if meta.is_file() && wanted(&path) {
                if seen.len() >= limits().watched_max {
                    return seen;
                }
                let changed = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                seen.insert(path, (changed, meta.len()));
            }
        }
    }
    seen
}

/// Gives `lsp` each file it was given before, but `except`, as it is on
/// disk now, closing one that's gone: it watches no files, so a server
/// sees a change to a file it was given only so.
fn refresh(code: &Code, lsp: &Lsp, except: Option<&Path>) -> anyhow::Result<()> {
    for path in lsp.opened() {
        if Some(path.as_path()) == except {
            continue;
        }
        let Some(language) = lsp.language_of(&path) else {
            continue;
        };
        match read_beneath(code, &path) {
            Ok(text) => {
                lsp.sync(&path, &language, text)?;
            }
            Err(_) => lsp.close(&path)?,
        }
    }
    Ok(())
}

/// The servers started so far and still running, each given the files
/// it was before as they are now.
fn started(code: &Code) -> anyhow::Result<Vec<Arc<Lsp>>> {
    let slots: Vec<_> = code
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(key, slot)| (key.clone(), slot.clone()))
        .collect();
    let mut lsps = Vec::new();
    for ((language, root), slot) in slots {
        // A server still starting is passed over: it knows nothing yet.
        // One that stopped too: naming its file starts it again.
        let Some(lsp) = slot
            .try_lock()
            .ok()
            .and_then(|slot| slot.clone())
            .filter(|lsp| lsp.ended().is_none())
        else {
            continue;
        };
        // Refreshed with its slot let go, so another call's answer never
        // passes over it.
        refresh(code, &lsp, None)?;
        if let Some(server) = code.config.servers.get(&language) {
            lsp.look(|| watched(code, &root, server))?;
        }
        lsps.push(lsp);
    }
    Ok(lsps)
}

/// A place in a file, as the model is shown it.
struct Place {
    /// Its path from the files folder, or whole outside it.
    path: String,
    line: u64,
    column: u64,
    /// Its line, for a file in the files folder.
    text: Option<String>,
}

impl Place {
    fn value(&self) -> Value {
        let mut value = json!({"path": self.path, "line": self.line, "column": self.column});
        if let Some(text) = &self.text {
            value["text"] = json!(text);
        }
        value
    }

    fn line(&self) -> String {
        match &self.text {
            Some(text) => format!("{}:{}:{}: {text}", self.path, self.line, self.column),
            None => format!("{}:{}:{}", self.path, self.line, self.column),
        }
    }
}

/// The text of `path`, when it's a file in the files folder: no other
/// file is read, whatever a server names, and none through a link.
fn text_of(code: &Code, path: &Path) -> Option<String> {
    read_beneath(code, path).ok()
}

/// How `path` is shown: from the files folder, else from the home folder
/// (`~/...`), else whole.
fn shown(code: &Code, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(&code.files) {
        return relative.display().to_string();
    }
    match code
        .home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// The place `uri` names at LSP position `position` (0-based, its
/// character in UTF-16).
fn place_of(code: &Code, uri: &str, position: &Value) -> Option<Place> {
    let path = lsp::path_of(uri)?;
    let line = position.get("line")?.as_u64()?;
    let character = position.get("character")?.as_u64()?;
    let text = text_of(code, &path);
    let row = text
        .as_deref()
        .and_then(|t| t.lines().nth(line as usize))
        .map(str::to_string);
    let column = match &row {
        Some(row) => chars_of(row, character),
        None => character,
    };
    Some(Place {
        path: shown(code, &path),
        line: line + 1,
        column: column + 1,
        text: row.map(|r| capped(r.trim(), limits().line_max)),
    })
}

/// How many characters of `row` its first `units` UTF-16 units are.
fn chars_of(row: &str, units: u64) -> u64 {
    let mut seen = 0u64;
    let mut chars = 0u64;
    for c in row.chars() {
        if seen >= units {
            break;
        }
        seen += c.len_utf16() as u64;
        chars += 1;
    }
    chars
}

/// The LSP position of 1-based `line` and `column` (in characters) of
/// `text`: refused past its last line, or past its line's end.
fn position_of(text: &str, line: u64, column: u64) -> anyhow::Result<Value> {
    ensure!(
        line >= 1 && column >= 1,
        "lines and characters count from 1"
    );
    let lines = text.lines().count().max(1) as u64;
    ensure!(line <= lines, "it has {lines} lines, not {line}");
    let row = text.lines().nth(line as usize - 1).unwrap_or("");
    let chars = row.chars().count() as u64;
    ensure!(
        column <= chars + 1,
        "line {line} has {chars} characters, not {column}"
    );
    let units: usize = row
        .chars()
        .take(column as usize - 1)
        .map(char::len_utf16)
        .sum();
    Ok(json!({"line": line - 1, "character": units}))
}

/// `diagnostics`: a file's, or every one the servers report.
fn diagnostics(
    code: &Code,
    arguments: &Value,
    deadline: Instant,
) -> anyhow::Result<(String, Value)> {
    let file = arguments.get("file_path").and_then(Value::as_str);
    let (lsps, only, settled) = match file {
        Some(name) => {
            let asked = asked(code, name)?;
            let (lsp, before) = server(code, &asked, deadline)?;
            let uri = lsp::uri_of(&asked.path);
            let settled = lsp.settle(before.map(|before| (uri.as_str(), before)), deadline);
            (vec![lsp], Some(uri), settled)
        }
        None => {
            let lsps = started(code)?;
            ensure!(
                !lsps.is_empty(),
                "no language server runs yet: name a file to start its language's"
            );
            let mut settled = true;
            for lsp in &lsps {
                settled &= lsp.settle(None, deadline);
            }
            (lsps, None, settled)
        }
    };
    if let Some(why) = lsps.iter().find_map(|lsp| lsp.ended()) {
        bail!("the language server stopped: {why}");
    }
    let mut found = Vec::new();
    for lsp in &lsps {
        for (uri, list) in lsp.diagnostics() {
            if only.as_ref().is_some_and(|only| *only != uri) {
                continue;
            }
            for diagnostic in list {
                let start = diagnostic
                    .pointer("/range/start")
                    .cloned()
                    .unwrap_or(Value::Null);
                let Some(place) = place_of(code, &uri, &start) else {
                    continue;
                };
                found.push((place, diagnostic));
            }
        }
    }
    found.sort_by(|a, b| (&a.0.path, a.0.line, a.0.column).cmp(&(&b.0.path, b.0.line, b.0.column)));
    let total = found.len();
    let mut lines = Vec::new();
    let mut values = Vec::new();
    // Listed until the most listed or the most bytes: the rest counted.
    let mut bytes = 0;
    for (place, diagnostic) in found.into_iter().take(limits().listed_max) {
        let severity = match diagnostic.get("severity").and_then(Value::as_u64) {
            Some(1) | None => "error",
            Some(2) => "warning",
            Some(3) => "information",
            _ => "hint",
        };
        let message = capped(
            diagnostic
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim(),
            limits().message_max,
        )
        .replace('\n', "\n  ");
        let source = diagnostic.get("source").and_then(Value::as_str);
        let code_of = match diagnostic.get("code") {
            Some(Value::String(c)) => Some(c.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        };
        let from = match (source, &code_of) {
            (Some(s), Some(c)) => format!(" ({s} {c})"),
            (Some(s), None) => format!(" ({s})"),
            (None, Some(c)) => format!(" ({c})"),
            (None, None) => String::new(),
        };
        let line = format!(
            "{}:{}:{}: {severity}: {message}{from}",
            place.path, place.line, place.column
        );
        bytes += line.len() + 1;
        if bytes > limits().answer_max && !lines.is_empty() {
            break;
        }
        lines.push(line);
        let mut value = json!({"path": place.path, "line": place.line, "column": place.column,
            "severity": severity, "message": message});
        if let Some(source) = source {
            value["source"] = json!(source);
        }
        if let Some(c) = code_of {
            value["code"] = json!(c);
        }
        values.push(value);
    }
    let mut text = match (total, file) {
        (0, Some(file)) => format!("No problems found in {file}."),
        (0, None) => "No problems found.".to_string(),
        // A server's messages quote the files' code: third-party text.
        _ => fenced(
            link_untrusted::Kind::Diagnostics,
            file.unwrap_or(FILES),
            &lines.join("\n"),
        )?,
    };
    if total > lines.len() {
        text.push_str(&format!("\n…and {} more", total - lines.len()));
    }
    if !settled {
        text.push_str(UNSETTLED);
    }
    Ok((
        text,
        json!({"diagnostics": values, "total": total, "complete": settled}),
    ))
}

/// `definition` or `references` (`call`), at a place or of a symbol.
fn places(
    code: &Code,
    call: &str,
    arguments: &Value,
    deadline: Instant,
) -> anyhow::Result<(String, Value)> {
    let file = arguments.get("file_path").and_then(Value::as_str);
    let symbol = arguments
        .get("symbol")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let line = arguments.get("line").and_then(Value::as_u64);
    let character = arguments.get("character").and_then(Value::as_u64);
    // Where to ask from: the file and position given, or the symbol's.
    // Whether the server settled says whether its answer is all it has.
    let (lsp, uri, position, settled) = match (symbol, file, line, character) {
        (None, Some(name), Some(line), Some(character)) => {
            let asked = asked(code, name)?;
            let position = position_of(&asked.text, line, character)?;
            let (lsp, _) = server(code, &asked, deadline)?;
            let settled = lsp.settle(None, limits().settle_by(deadline));
            (lsp, lsp::uri_of(&asked.path), position, settled)
        }
        (Some(symbol), file, None, None) => {
            let lsps = match file {
                Some(name) => {
                    let asked = asked(code, name)?;
                    vec![server(code, &asked, deadline)?.0]
                }
                None => started(code)?,
            };
            ensure!(
                !lsps.is_empty(),
                "no language server runs yet: give a file of the project with the symbol"
            );
            let (mut found, settled) = symbols::named(code, &lsps, symbol, deadline)?;
            if call == "definition" {
                // Every symbol of the name is a definition: all listed.
                let places = found
                    .iter()
                    .filter_map(|(_, uri, position)| place_of(code, uri, position))
                    .collect();
                return answer_places(call, places, settled, symbol_note(symbol, found.len()));
            }
            // Its references are one symbol's: several of the name are
            // said, never one chosen for the caller.
            if found.len() > 1 {
                let places: Vec<String> = found
                    .iter()
                    .filter_map(|(_, uri, position)| place_of(code, uri, position))
                    .map(|p| format!("{}:{}:{}", p.path, p.line, p.column))
                    .collect();
                bail!(
                    "{} symbols are named {symbol}, at {}: give the file, line and character of \
                     the one you mean",
                    found.len(),
                    places.join(", ")
                );
            }
            let (lsp, uri, position) = found.pop().context("a symbol found")?;
            (lsp, uri, position, settled)
        }
        _ => bail!(
            "give a file_path, a line and a character, or a symbol (and a file_path of its project)"
        ),
    };
    let params = match call {
        "definition" => json!({"textDocument": {"uri": uri}, "position": position}),
        _ => json!({"textDocument": {"uri": uri}, "position": position,
            "context": {"includeDeclaration": true}}),
    };
    let method = format!("textDocument/{call}");
    let found = lsp.request(&method, params, deadline)?;
    let locations: Vec<Value> = match found {
        Value::Array(list) => list,
        Value::Null => Vec::new(),
        one => vec![one],
    };
    let mut places = Vec::new();
    for location in &locations {
        // A Location, or a LocationLink: its target's name.
        let (uri, start) = match location.get("targetUri") {
            Some(uri) => (
                uri,
                location
                    .pointer("/targetSelectionRange/start")
                    .or_else(|| location.pointer("/targetRange/start")),
            ),
            None => match location.get("uri") {
                Some(uri) => (uri, location.pointer("/range/start")),
                None => continue,
            },
        };
        if let (Some(uri), Some(start)) = (uri.as_str(), start)
            && let Some(place) = place_of(code, uri, start)
        {
            places.push(place);
        }
    }
    let settled = settled && lsp.ended().is_none();
    answer_places(call, places, settled, None)
}

/// What's said of `count` symbols named `symbol`, when that's several.
fn symbol_note(symbol: &str, count: usize) -> Option<String> {
    (count > 1).then(|| format!("{count} symbols are named {symbol}: each is listed."))
}

/// What an answer says when its server hadn't settled: it may not be all.
const UNSETTLED: &str = "\n(the language server was still working when the call's time ran out: \
    this may not be all)";

/// The answer listing `places` found by `call`, saying whether that's all
/// the server has (`complete`) and `note`.
fn answer_places(
    call: &str,
    mut places: Vec<Place>,
    complete: bool,
    note: Option<String>,
) -> anyhow::Result<(String, Value)> {
    places.sort_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
    places.dedup_by(|a, b| (&a.path, a.line, a.column) == (&b.path, b.line, b.column));
    let total = places.len();
    // Listed until the most listed or the most bytes: the rest counted.
    let mut listed: Vec<&Place> = Vec::new();
    let mut bytes = 0;
    for place in places.iter().take(limits().listed_max) {
        bytes += place.line().len() + 1;
        if bytes > limits().answer_max && !listed.is_empty() {
            break;
        }
        listed.push(place);
    }
    let mut text = match total {
        0 => match call {
            "definition" => "No definition found.".to_string(),
            _ => "No references found.".to_string(),
        },
        // Each place shows its line of a file: third-party text.
        _ => fenced(
            link_untrusted::Kind::File,
            FILES,
            &listed
                .iter()
                .map(|p| p.line())
                .collect::<Vec<_>>()
                .join("\n"),
        )?,
    };
    if total > listed.len() {
        text.push_str(&format!("\n…and {} more", total - listed.len()));
    }
    if let Some(note) = note {
        text.push('\n');
        text.push_str(&note);
    }
    if !complete {
        text.push_str(UNSETTLED);
    }
    let values: Vec<Value> = listed.iter().map(|p| p.value()).collect();
    Ok((
        text,
        json!({"places": values, "total": total, "complete": complete}),
    ))
}

/// What the fence names as the source of lines from several files.
const FILES: &str = "your files folder";

/// `text` from `name`, of `kind`, in the fence every tool puts around
/// third-party text.
fn fenced(kind: link_untrusted::Kind, name: &str, text: &str) -> anyhow::Result<String> {
    Ok(link_untrusted::fence(
        link_untrusted::Source { kind, name },
        text,
    )?)
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The command line starting `program` with its `args` as a server: under
/// the sandbox, after its `--`, its variables set inside it by [`env_program`].
fn server_argv(config: &Config, program: &Path, args: &[String]) -> Vec<String> {
    let mut argv = config.sandbox.clone();
    if !argv.is_empty() {
        argv.push("--".into());
    }
    if !config.env.is_empty() {
        argv.push(env_program().into());
        argv.extend(
            config
                .env
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        );
    }
    argv.push(program.display().to_string());
    argv.extend(args.iter().cloned());
    argv
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(unit);
