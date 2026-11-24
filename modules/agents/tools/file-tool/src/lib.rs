//! Link file: an MCP server over stdio offering `read`, `write`,
//! `edit`, `glob` and `grep` over an agent's files folder, by the names
//! and arguments of Claude Code's Read, Write, Edit, Glob and Grep.
//!
//! The server touches no file itself: its calls run in `lnk-file-tool
//! apply` under the sandbox it's given (`lnk sandbox run` and its flags,
//! from the agent's sandbox file: the files folder read, only the
//! folders `[files]` names written, no network), one for the session,
//! each call a line on its input and its answer a line on its output
//! ([`Sandboxed`]). [`apply`] keeps
//! to the same folders before the sandbox does, so a call it refuses
//! says why in words, and never follows a link.
//!
//! An agent on a team reaches what it's granted of the team's shared
//! folder at `teams/<team>/` of its paths ([`Team`]).

use std::ffi::OsStr;
use std::io::{BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, anyhow, bail, ensure};
use link_files::nofollow::{Dir, Kind};
use serde_json::{Value, json};

mod bridged;
mod checks;
pub mod cli;
mod contents;
mod find;
mod offer;
mod places;
mod sandboxed;
pub(crate) use contents::span_of;
use contents::{given, lines, media_of, put, read, text_of, unchanged, with_change};
pub use places::Caller;
use places::{Place, inside, parts, writable};
pub(crate) use places::{home_writable, writable_said};

pub use bridged::{Bridges, Seen, bridged, serve_bridged};
use find::{Query, find};
pub use offer::Offer;
pub use sandboxed::Sandboxed;

/// The protocol versions it speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Link file's limits, as `limits.toml` sets them.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Limits {
    /// The largest image or PDF `read` gives whole, in bytes.
    media_max: u64,
    /// The longest `glob` or `grep` looks, in milliseconds.
    find_time_max_ms: u64,
    /// The largest `.gitignore` `glob` and `grep` keep to, in bytes.
    ignore_file_max: u64,
    /// The most bytes JSON escapes one byte of a string to.
    escaped_max: u64,
    /// The largest file it reads or writes, in bytes.
    file_max: u64,
    /// The most of a file one `read` gives, in bytes, whole lines.
    read_max: usize,
    /// The most files `glob` or lines `grep` gives.
    found_max: usize,
    /// The most of each line `grep` gives, in bytes.
    line_max: usize,
    /// The most files and folders `glob` or `grep` looks at.
    files_max: usize,
    /// The largest file `grep` reads, in bytes.
    searched_max: u64,
    /// The most a `glob` or `grep` pattern compiles to, in bytes.
    pattern_max: usize,
    /// The longest diff a `write` or `edit` answers with, in bytes.
    diff_max: usize,
    /// The fewest columns `read` right-aligns a line's number in.
    number_width: usize,
    /// What `read` puts between a line's number and the line.
    number_mark: String,
}

impl Limits {
    /// The longest `glob` or `grep` looks: past it, it answers with what it found
    /// and says it stopped.
    fn find_time_max(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.find_time_max_ms)
    }

    /// The most a call and its answer take, in bytes, between the server
    /// and its sandbox: an `edit`'s `old_string` and `new_string`, each as
    /// large as a file, escaped as JSON escapes them at most, so a file written under
    /// `file_max` ([`Limits`]) always fits, whatever it holds.
    fn message_max(&self) -> u64 {
        2 * self.escaped_max * self.file_max + (64 << 10)
    }
}

/// [`Limits`], read once.
fn limits() -> &'static Limits {
    static LIMITS: std::sync::LazyLock<Limits> = std::sync::LazyLock::new(|| {
        let limits: Limits =
            toml::from_str(include_str!("limits.toml")).expect("Link file's limits.toml");
        assert!(limits.media_max <= limits.file_max && limits.escaped_max >= 1);
        assert!(limits.read_max as u64 <= limits.file_max && limits.line_max <= limits.read_max);
        assert!(limits.searched_max <= limits.file_max && limits.found_max > 0);
        limits
    });
    &LIMITS
}

/// The agent's folder: its files folder (Home), and its own folder beside
/// it where its instructions, skills and conversations are; the folders a
/// call may write (or `.` for all of Home); its teams'.
pub struct Files {
    pub folder: link_agent_contract::folder::Folder,
    pub write: Vec<String>,
    pub teams: Vec<Team>,
}

impl Files {
    /// The agent's own folder, where a path's first part (of its parsed
    /// `parts`) names one of its folders there
    /// ([`link_agent_contract::folder::SEEN`]).
    fn own_of(&self, parts: &[String]) -> Option<&Path> {
        parts
            .first()
            .and_then(|top| link_agent_contract::folder::seen_collection(top))
            .map(|_| self.folder.agent.as_path())
    }
}

/// A team's shared folder, at `teams/<name>/` of a call's paths: the
/// folders of it a call may write, and those it may only read, each a
/// folder's name in it or `.` for all of it.
pub struct Team {
    pub name: String,
    pub root: PathBuf,
    pub write: Vec<String>,
    pub read: Vec<String>,
}

/// Where a team's shared folder is in a call's paths (`teams`).
pub use link_sandbox_contract::spec::team_paths;

/// The name for a team's whole folder in [`Team`]'s lists, and for the
/// whole files folder in [`Files`]' `write`.
const WHOLE: &str = ".";

/// The files folder's conversations, its harness's alone.
const CONVERSATIONS: &str = link_store_contract::storage::Collection::Conversations.directory();

impl Team {
    /// Whether it may reach `top`, a folder of the team's (none: the
    /// folder itself), written or not.
    fn reaches(&self, top: Option<&str>, writing: bool) -> bool {
        let lists: &[&Vec<String>] = match writing {
            true => &[&self.write],
            false => &[&self.write, &self.read],
        };
        lists.iter().any(|list| {
            list.iter()
                .any(|n| n == WHOLE || top.is_some_and(|top| n == top))
        })
    }

    fn granted(&self) -> String {
        let all: Vec<String> = self
            .write
            .iter()
            .map(|n| (n, "written"))
            .chain(self.read.iter().map(|n| (n, "read")))
            .map(|(n, how)| match n.as_str() {
                WHOLE => format!("all of it ({how})"),
                n => format!("{}/{}/{n}/ ({how})", team_paths(), self.name),
            })
            .collect();
        match all.is_empty() {
            true => "none of it".into(),
            false => all.join(", "),
        }
    }
}

/// Answers the messages on `input` on `output`, until `input` ends, each
/// call run as `apply` (a program and its arguments, its sandbox's), in
/// one sandbox for the session ([`Sandboxed`]).
pub fn serve(
    apply: &[String],
    offer: &Offer,
    input: impl BufRead,
    output: impl Write,
) -> anyhow::Result<()> {
    let sandboxed = Sandboxed::new(apply);
    let versions = Versions::default();
    let run = |name: &str, arguments: &Value| versions.call(&sandboxed, name, arguments);
    serve_with(&run, offer, input, output)
}

/// [`serve`], but the folders of the teams `teams` names are reached
/// through their files bridge: a call naming one (`teams/<team>/...` as
/// its `file_path`, or a `glob`'s or `grep`'s `path`) runs as [`bridged`]
/// does, every other call as `apply`. A `glob` or `grep` with no `path`
/// looks in the files folder alone.
pub fn serve_teamed(
    apply: &[String],
    teams: &BridgedTeams,
    offer: &Offer,
    input: impl BufRead,
    output: impl Write,
) -> anyhow::Result<()> {
    let sandboxed = Sandboxed::new(apply);
    let seen = Seen::default();
    let bridges = Bridges {
        files: &teams.bridge,
        store: None,
    };
    let versions = Versions::default();
    let run = |name: &str, arguments: &Value| match teams.reaches(arguments) {
        true => bridged(&bridges, &[], &seen, name, arguments),
        false => versions.call(&sandboxed, name, arguments),
    };
    serve_with(&run, offer, input, output)
}

/// What a session saw of each file on this machine: the SHA-256 of what
/// it last read or wrote, which its next `write` or `edit` of the file expects, so
/// a file changed meanwhile (by a person, or another of the agent's
/// conversations) is refused and read again, never overwritten unseen, as
/// the files bridge refuses one. A `write` of a file it
/// never read replaces what's there, and an `edit` changes it.
///
/// Kept by the path's parts as spelled, and by the file itself (its
/// device and number, [`FILE`]): a path spelled another way that reaches
/// the same file, as `plan.md` does `Plan.md` where names ignore case,
/// expects the version seen of it too.
#[derive(Debug, Default)]
struct Versions(std::cell::RefCell<SeenHere>);

#[derive(Debug, Default)]
struct SeenHere {
    /// Each path's version, and the file it was.
    by_path: std::collections::BTreeMap<String, (String, Option<String>)>,
    /// Each file's version, by [`FILE`].
    by_file: serde_json::Map<String, Value>,
}

impl Versions {
    /// Call `name` in `sandboxed`, a `write` or an `edit` expecting the version last
    /// seen; the version its answer names kept, and taken out of it.
    fn call(&self, sandboxed: &Sandboxed, name: &str, arguments: &Value) -> anyhow::Result<Value> {
        let key = arguments
            .get(FILE_PATH)
            .and_then(Value::as_str)
            .and_then(|path| parts(path).ok())
            .map(|parts| parts.join("/"));
        let expect = match (name, &key) {
            ("write" | "edit", Some(key)) => {
                let seen = self.0.borrow();
                let version = seen.by_path.get(key).map(|(version, _)| version.clone());
                // Each file seen only when its path as spelled wasn't.
                let by_file = match version {
                    Some(_) => Default::default(),
                    None => seen.by_file.clone(),
                };
                Expect { version, by_file }
            }
            _ => Expect::default(),
        };
        let answered = sandboxed.call_expecting(name, arguments, &expect);
        // A read that fails leaves the session nothing seen, so a file
        // removed since is written anew, never refused for good.
        if let (Err(_), "read", Some(key)) = (&answered, name, &key) {
            let mut seen = self.0.borrow_mut();
            if let Some((_, Some(file))) = seen.by_path.remove(key) {
                seen.by_file.remove(&file);
            }
        }
        let mut answer = answered?;
        let (version, file) = match answer.as_object_mut() {
            Some(answer) => (answer.remove(VERSION), answer.remove(FILE)),
            None => (None, None),
        };
        if let (Some(key), Some(Value::String(version))) = (key, version) {
            let file = file.and_then(|f| f.as_str().map(str::to_string));
            let mut seen = self.0.borrow_mut();
            if let Some(file) = &file {
                seen.by_file.insert(file.clone(), version.clone().into());
            }
            if let Some((_, Some(was))) = seen.by_path.insert(key, (version, file.clone()))
                && Some(&was) != file.as_ref()
            {
                seen.by_file.remove(&was);
            }
        }
        Ok(answer)
    }
}

/// What a `write` or an `edit` expects of its file: the version seen by its path as
/// spelled, else the one seen of the file it reaches (by [`FILE`]).
#[derive(Debug, Default)]
pub struct Expect {
    pub version: Option<String>,
    pub by_file: serde_json::Map<String, Value>,
}

impl Expect {
    /// What's expected of `file` (its [`FILE`], none if it isn't there).
    fn of(&self, file: Option<&str>) -> Option<&str> {
        self.version.as_deref().or_else(|| {
            file.and_then(|file| self.by_file.get(file))
                .and_then(Value::as_str)
        })
    }
}

/// The key of the version of a file an answer from `apply` names: the
/// SHA-256 of what it read or wrote, which `serve` keeps and takes out.
const VERSION: &str = "version";

/// The key of the file an answer from `apply` names, by its device and
/// its number there (`<dev>:<ino>`), whatever name reached it: `serve`
/// keeps it and takes it out.
const FILE: &str = "file";

/// [`FILE`] of file `name` in `dir`: none when nothing's there.
fn file_of(dir: &Dir, name: &OsStr) -> Option<String> {
    dir.stat(name)
        .ok()
        .filter(|stat| stat.kind == Kind::File)
        .map(|stat| format!("{}:{}", stat.dev, stat.ino))
}

/// The teams whose folders a session reaches through the files bridge
/// `bridge` as its agent, by the names its paths give them, where its
/// other files are a folder of this machine's.
#[derive(Debug)]
pub struct BridgedTeams {
    pub bridge: link_store_contract::files::Client,
    pub teams: Vec<String>,
}

impl BridgedTeams {
    /// Whether a call with `arguments` names a folder of one of its teams.
    pub fn reaches(&self, arguments: &Value) -> bool {
        let named = [FILE_PATH, FOLDER]
            .iter()
            .find_map(|key| arguments.get(*key).and_then(Value::as_str));
        let Some(Ok(parts)) = named.map(parts) else {
            return false;
        };
        matches!(parts.as_slice(), [top, team, ..] if *top == team_paths() && self.teams.contains(team))
    }
}

/// How a call is run: its name and arguments, to its answer.
pub type Run<'a> = dyn Fn(&str, &Value) -> anyhow::Result<Value> + 'a;

fn serve_with(
    run: &Run,
    offer: &Offer,
    input: impl BufRead,
    mut output: impl Write,
) -> anyhow::Result<()> {
    for line in input.lines() {
        let line = line.context("reading a message")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = answer_with(run, offer, &line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// The reply to message `line`: none for a notification.
pub fn answer(apply: &[String], line: &str) -> Option<Value> {
    let sandboxed = Sandboxed::new(apply);
    let run = |name: &str, arguments: &Value| sandboxed.call(name, arguments);
    answer_with(&run, &Offer::default(), line)
}

/// The reply to message `line`, each call `offer` offers run with `run`:
/// none for a notification.
pub fn answer_with(run: &Run, offer: &Offer, line: &str) -> Option<Value> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "not JSON".into()));
    };
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = VERSIONS
                .iter()
                .find(|v| Some(**v) == asked)
                .unwrap_or(&VERSIONS[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "link-file", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": offer.instructions(),
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => match offer.offered() {
            Ok(tools) => result(id, json!({"tools": tools})),
            Err(why) => error(id, -32603, format!("{why:#}")),
        },
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let caller = Caller::of(params.get("_meta"));
            if !link_file_tool_contract::calls().is_ok_and(|calls| calls.iter().any(|c| c == name))
                || !offer.offers(name)
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
            // Of the conversations, only its own read; none
            // is ever written.
            let reading = matches!(name, "read" | "glob" | "grep");
            let reached = [FILE_PATH, FOLDER]
                .iter()
                .filter_map(|key| arguments.get(*key).and_then(Value::as_str))
                .filter(|_| reading)
                .try_for_each(|path| match parts(path) {
                    Ok(parts) => caller.reaches(&parts),
                    // An empty folder is the files folder; what isn't a
                    // path is refused by its call, saying so.
                    Err(_) => Ok(()),
                });
            if let Err(refused) = reached {
                return Some(result(
                    id,
                    json!({"content": [{"type": "text", "text": format!("{refused:#}")}], "isError": true}),
                ));
            }
            let answered = run(name, &arguments)
                .map(|mut answer| {
                    // A file's version is the tool's own, never the model's.
                    if let Some(answer) = answer.as_object_mut() {
                        answer.remove(VERSION);
                        answer.remove(FILE);
                    }
                    offer.answer(answer)
                })
                .and_then(|value| match value.get("data") {
                    Some(_) => given_content(value),
                    None => text_content(name, &arguments, value),
                });
            match answered {
                Ok(content) => result(id, content),
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

/// A call's answer as MCP gives it: `read`'s text, or the answer as
/// JSON, fenced as a file's where it holds what a file says (what's read,
/// what's found and a change's diff), with its structured content
/// ([`structured`]). A `read`'s receipt is checked against the fenced
/// text, unfenced.
fn text_content(name: &str, arguments: &Value, value: Value) -> anyhow::Result<Value> {
    let finds = matches!(name, "glob" | "grep");
    let text = match &value["text"] {
        Value::String(text) if name == "read" => text.clone(),
        _ if finds => crate::find::listed(name, &value),
        _ => value.to_string(),
    };
    let from = match name {
        _ if finds => arguments.get(FOLDER).and_then(Value::as_str),
        "read" => value["path"].as_str(),
        _ if value.get("diff").is_some() => value["path"].as_str(),
        _ => None,
    };
    let text = match from {
        Some(from) => file_fenced(if from.is_empty() { FILES } else { from }, &text)?,
        None if finds => file_fenced(FILES, &text)?,
        None => text,
    };
    Ok(json!({"content": [{"type": "text", "text": text}], "structuredContent": structured(value)}))
}

/// An answer's structured content: the answer without what a file says,
/// which only the fenced text gives (what `read` read, a line `grep`
/// found, a change's diff), so a client that gives a model this sees none
/// of it bare. `read`'s receipt (`read`) stays, which names the text by
/// its digest alone.
fn structured(mut value: Value) -> Value {
    if let Some(answer) = value.as_object_mut() {
        answer.remove("diff");
        answer.remove("text");
    }
    let found = value.get_mut("found").and_then(Value::as_array_mut);
    for found in found.into_iter().flatten() {
        if let Some(found) = found.as_object_mut() {
            found.remove("text");
        }
    }
    value
}

/// The argument naming the file a `read`, `write` or `edit` works on, as
/// a model calls it.
const FILE_PATH: &str = "file_path";

/// The argument naming the folder a `glob` or `grep` looks in.
const FOLDER: &str = "path";

/// What the fence names as the source of what's found in the whole
/// files folder.
const FILES: &str = "your files folder";

/// `text` fenced as the file (or folder) `from`'s.
fn file_fenced(from: &str, text: &str) -> anyhow::Result<String> {
    let source = link_untrusted::Source {
        kind: link_untrusted::Kind::File,
        name: from,
    };
    Ok(link_untrusted::fence(source, text)?)
}

/// `read`'s answer of an image or a PDF ([`given`]) as MCP gives it: a
/// line saying what it is, fenced as the file's, then the image
/// (`image`), or the PDF as a resource's `blob`, for a model that reads
/// them; its structured content without its bytes.
fn given_content(mut value: Value) -> anyhow::Result<Value> {
    let data = value["data"].take();
    if let Some(value) = value.as_object_mut() {
        value.remove("data");
    }
    let (path, media) = (
        value["path"].as_str().unwrap_or_default(),
        value["media"].as_str().unwrap_or_default(),
    );
    let said = file_fenced(path, &format!("{path}: {media}, {} bytes", value["bytes"]))?;
    let part = match media.starts_with("image/") {
        true => json!({"type": "image", "data": data, "mimeType": media}),
        false => json!({"type": "resource", "resource": {"uri": format!("file:///{path}"),
            "mimeType": media, "blob": data}}),
    };
    Ok(json!({"content": [{"type": "text", "text": said}, part], "structuredContent": value}))
}

/// `apply`: each call on `input`, a line each ([`apply_call`]), its
/// answer on `output` as a line, `{"ok": ...}` or `{"error": "..."}`,
/// until `input` ends.
pub fn apply(files: &Files, mut input: impl BufRead, mut output: impl Write) -> anyhow::Result<()> {
    loop {
        let mut call = Vec::new();
        (&mut input)
            .take(limits().message_max())
            .read_until(b'\n', &mut call)?;
        if call.iter().all(u8::is_ascii_whitespace) {
            if call.is_empty() {
                return Ok(());
            }
            continue;
        }
        // A call past the most never ends where its line does: the last.
        let whole = call.ends_with(b"\n") || (call.len() as u64) < limits().message_max();
        let answer = match serde_json::from_slice::<Value>(&call) {
            Ok(call) if whole => {
                let name = call["name"].as_str().unwrap_or("");
                let expect = Expect {
                    version: call["expect"].as_str().map(str::to_string),
                    by_file: call["by_file"].as_object().cloned().unwrap_or_default(),
                };
                match apply_expecting(files, name, &call["arguments"], &expect) {
                    Ok(value) => json!({"ok": value}),
                    Err(err) => json!({"error": format!("{err:#}")}),
                }
            }
            _ => json!({"error": "the call isn't JSON"}),
        };
        writeln!(output, "{answer}")?;
        output.flush()?;
        if !whole {
            return Ok(());
        }
    }
}

/// Runs call `name` with `arguments` over `files`.
pub fn apply_call(files: &Files, name: &str, arguments: &Value) -> anyhow::Result<Value> {
    apply_expecting(files, name, arguments, &Expect::default())
}

/// [`apply_call`], a `write` or an `edit` refused where the file isn't the version
/// `expect` names (its SHA-256, as an answer's `version` named it):
/// changed since the session read or wrote it. Each `read`, `write` and
/// `edit` answers with the file's version and the file it is
/// ([`FILE`]). A `write` and an `edit` hold their folder's lock from
/// their check to their file put in place, so two of them never both
/// pass the check.
pub fn apply_expecting(
    files: &Files,
    name: &str,
    arguments: &Value,
    expect: &Expect,
) -> anyhow::Result<Value> {
    let text = |key: &str| -> anyhow::Result<&str> {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .with_context(|| format!("{name} needs '{key}', a string"))
    };
    match name {
        "read" => read(files, text(FILE_PATH)?, span_of(arguments)?),
        "write" => {
            let path = text(FILE_PATH)?;
            let content = text("content")?;
            let file = writable(files, path)?;
            let (dir, name) = file.parent(true)?;
            let _locked = dir
                .lock(libc::LOCK_EX)
                .with_context(|| format!("locking {path}'s folder"))?;
            let created = dir.kind(name).is_err();
            if let Some(expect) = expect.of(file_of(&dir, name).as_deref()) {
                unchanged(&dir, name, path, expect)?;
            }
            // What it held, to show the change: unknown if it isn't text.
            let before = match created {
                true => Some(None),
                false => text_of(&dir, name, path).ok().map(Some),
            };
            put(&dir, name, content)?;
            Ok(with_change(
                json!({"wrote": path, "bytes": content.len(), "created": created,
                    VERSION: link_digest::sha256(content.as_bytes()), FILE: file_of(&dir, name)}),
                before.map(|before| crate::diff::unified(path, before.as_deref(), Some(content))),
                content,
            ))
        }
        "edit" => {
            let path = text(FILE_PATH)?;
            let (old, new) = (text("old_string")?, text("new_string")?);
            ensure!(
                !old.is_empty(),
                "'old_string' is the text to replace: it can't be empty"
            );
            let all = arguments
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let file = writable(files, path)?;
            let (dir, name) = file.parent(false)?;
            let _locked = dir
                .lock(libc::LOCK_EX)
                .with_context(|| format!("locking {path}'s folder"))?;
            // Changed since the session read or wrote it: refused, as a
            // `write` is, so the edit never lands on text it didn't see.
            if let Some(expect) = expect.of(file_of(&dir, name).as_deref()) {
                unchanged(&dir, name, path, expect)?;
            }
            let content = text_of(&dir, name, path)?;
            let found = content.matches(old).count();
            ensure!(
                found > 0,
                "{path} doesn't hold 'old_string': read it, and give its text exactly"
            );
            ensure!(
                found == 1 || all,
                "{path} holds 'old_string' {found} times: give more of the text around it, \
                 or 'replace_all'"
            );
            // Its size known before it's made: `replace_all` can't build more
            // than a file may be.
            let size = content.len() as u128 - (found * old.len()) as u128
                + found as u128 * new.len() as u128;
            ensure!(
                size <= limits().file_max as u128,
                "replaced, {path} would be over {} MiB, larger than a file written",
                limits().file_max >> 20
            );
            let after = content.replace(old, new);
            // Changed while it was replaced in: refused, never lost.
            unchanged(&dir, name, path, &link_digest::sha256(content.as_bytes()))?;
            put(&dir, name, &after)?;
            Ok(with_change(
                json!({"replaced": found, "path": path,
                    VERSION: link_digest::sha256(after.as_bytes()), FILE: file_of(&dir, name)}),
                Some(crate::diff::unified(path, Some(&content), Some(&after))),
                &after,
            ))
        }
        "glob" | "grep" => {
            let query = Query::of(name, arguments)?;
            // Left out or empty: Home.
            let folder = match arguments.get(FOLDER).and_then(Value::as_str) {
                Some(folder) if !folder.is_empty() => inside(files, folder)?,
                _ => Place {
                    root: files.folder.home.clone(),
                    shown: String::new(),
                    parts: Vec::new(),
                },
            };
            find(&folder, &query, limits().find_time_max())
        }
        other => bail!("no call {other:?}"),
    }
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub mod diff;

#[cfg(test)]
link_test_kit::program!(system);

#[cfg(test)]
mod limits_tests {
    /// `limits.toml` reads, each limit within what the rest hold to: a
    /// whole image or PDF is no larger than a file, and a call carrying a
    /// file's text escaped fits a message.
    #[test]
    fn its_limits_read() {
        let limits = super::limits();
        // What `read` and a diff give stays within what a harness keeps of
        // an answer (Link's 32 KB), so nothing is cut unsaid.
        assert!(limits.read_max < 32 << 10 && limits.diff_max < 32 << 10);
        assert!(limits.line_max <= limits.read_max && limits.found_max > 0);
        assert!(limits.searched_max <= limits.file_max && limits.files_max > 0);
        assert!(limits.pattern_max > 0);
        assert!(limits.media_max <= limits.file_max);
        assert!(limits.find_time_max() >= std::time::Duration::from_secs(1));
        assert!(limits.message_max() > 2 * limits.file_max);
        assert!(limits.ignore_file_max > 0);
    }
}
