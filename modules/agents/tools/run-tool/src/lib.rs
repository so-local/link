//! Link run: an MCP server over stdio offering `command`, which runs a
//! shell command (`sh -c`) in an agent's files folder and answers with
//! what it printed: Claude Code's Bash call, `command`, `timeout`,
//! `run_in_background` and `description`.
//!
//! It runs nothing itself: each call runs under the sandbox it's given
//! (`lnk sandbox run` and its flags, from the agent's sandbox file), a
//! new one per call, ended with it. It offers a command only where the
//! shell is among the programs it's told this machine has, and keeps what
//! the command printed to the size it's given, its start and its end. A
//! command still running at its timeout is stopped with its process
//! group: on Linux, everything it started.
//!
//! Given a place for them ([`Background`]), a command may run in the
//! background ([`background`]), outliving the turn that started it under
//! a supervisor of its own, the agent told through its bridge when it
//! ends; `list`, `read` and `stop` reach it.

pub mod background;
mod calls;
mod checks;
pub mod cli;

use std::collections::VecDeque;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
pub use background::Background;
pub use calls::Calls;
use calls::{Entered, kill_group};
use link_files::nofollow::Dir;
use serde_json::{Value, json};

/// The protocol versions it speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// A variable `--var` gives, `NAME=VALUE`: one the sandbox lets a caller
/// give a program ([`link_sandbox_client::check_given_variable`]), never
/// the dynamic loader's, a proxy's or one it keeps itself, and with no
/// NUL in its value.
pub fn variable(given: &str) -> anyhow::Result<(String, String)> {
    let (name, value) = given
        .split_once('=')
        .with_context(|| format!("--var {given:?} is NAME=VALUE"))?;
    link_sandbox_client::check_given_variable(name).context("--var")?;
    ensure!(!value.contains('\0'), "--var {name} has a NUL in it");
    Ok((name.to_string(), value.to_string()))
}

/// Descriptor four is distinct from the background gate on three.
pub const CONTEXT_FD: i32 = 4;
pub struct Run {
    pub context: Option<Arc<link_run_tool_contract::context::Context>>,
    /// The agent's files folder: each command starts in it.
    pub files: PathBuf,
    /// The PATH a program's name is found on.
    pub path: String,
    /// The programs a command may start, as its sandbox file names them
    /// (names or absolute paths), or `*` alone for any: a command is
    /// offered only where the shell is one.
    pub commands: Vec<String>,
    /// The sandbox each call runs under: a program and its arguments, to
    /// which `--`, the program and its arguments are added. Empty: none,
    /// for tests.
    pub sandbox: Vec<String>,
    /// Variables each command gets, since the sandbox clears its
    /// environment: a desktop's `DISPLAY`. Handed to the sandbox with its
    /// context on a descriptor, whose variable refusals they pass.
    pub env: Vec<(String, String)>,
    /// The longest a command runs before it's stopped, and how long when
    /// its call doesn't say.
    pub timeout: Duration,
    /// What's kept of each of its output and its errors, in bytes; and
    /// the most a background command's `read` gives at once.
    pub output: usize,
    /// Where background commands are kept, and their caps: none, no
    /// background commands.
    pub background: Option<Background>,
    /// The agent's conversations folder (`chats/`), which its sandbox
    /// hides: each call reads only its own conversation's folder in it,
    /// and a stranger's call none. None: no conversation's folder is read.
    pub conversations: Option<PathBuf>,
}

/// Answers the messages on `input` on `output`, until `input` ends: each
/// call on a thread of its own, so a round's calls run at once, at most
/// [`link_run_tool_contract::Limits::calls_at_once`] of them.
pub fn serve(run: &Run, input: impl BufRead, output: impl Write + Send) -> anyhow::Result<()> {
    let output = Mutex::new(output);
    let calls = Calls::default();
    std::thread::scope(|scope| -> anyhow::Result<()> {
        for line in input.lines() {
            let line = line.context("reading a message")?;
            if line.trim().is_empty() {
                continue;
            }
            let (output, calls) = (&output, &calls);
            scope.spawn(move || {
                if let Some(reply) = answer_in(run, calls, &line) {
                    let mut output = output.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = writeln!(output, "{reply}");
                    let _ = output.flush();
                }
            });
        }
        Ok(())
    })
}

/// The reply to message `line`, on a session of its own: none for a
/// notification.
pub fn answer(run: &Run, line: &str) -> Option<Value> {
    answer_in(run, &Calls::default(), line)
}

/// The reply to message `line`, among the calls its session runs
/// (`calls`): none for a notification.
pub fn answer_in(run: &Run, calls: &Calls, line: &str) -> Option<Value> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "not JSON".into()));
    };
    let method = message.get("method").and_then(Value::as_str)?;
    if method == "notifications/cancelled" {
        if let Some(id) = message.pointer("/params/requestId") {
            calls.cancel(id);
        }
        return None;
    }
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
                    "serverInfo": {"name": "link-run", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": instructions(run),
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => match offered(run) {
            Ok(tools) => result(id, json!({"tools": tools})),
            Err(why) => error(id, -32603, format!("{why:#}")),
        },
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            // A call it offers now: a background one only with background
            // commands.
            let offered = offered(run).is_ok_and(|tools| {
                tools
                    .as_array()
                    .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
            });
            if !offered {
                return Some(error(id, -32602, format!("no tool {name:?}")));
            }
            // Its arguments, refused by their declaration before its handler.
            if let Err(refused) = crate::cli::check_call(name, &arguments) {
                return Some(result(
                    id,
                    json!({"content": [{"type": "text", "text": refused.to_string()}], "isError": true}),
                ));
            }
            let Some(entered) = calls.enter(&id) else {
                return Some(result(
                    id,
                    json!({"content": [{"type": "text", "text": format!(
                        "{} calls run at once already, the most there may be, or one with this \
                         call's id: wait for one to end",
                        link_run_tool_contract::limits().calls_at_once
                    )}], "isError": true}),
                ));
            };
            let meta = params.get("_meta").cloned().unwrap_or(Value::Null);
            let called = match name {
                "command" if arguments[BACKGROUND_ARGUMENT] == true => {
                    start_background(run, &arguments, &meta)
                }
                // The command, as the fence names what printed it.
                "command" => command_in(run, &arguments, &meta, Some(&entered)).and_then(|ran| {
                    let command = arguments["command"].as_str().unwrap_or_default();
                    Ok((ran.text(command)?, ran.value()))
                }),
                _ => call_background(run, name, &arguments, &meta),
            };
            drop(entered);
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

/// What it tells the model it's for.
fn instructions(run: &Run) -> String {
    let mut text = "Run a shell command in your files folder when a task needs one: a \
        calculation, a script, a conversion. Each call starts afresh in a sandbox of its own, \
        so keep what you need in files."
        .to_string();
    if run.background.is_some() {
        text.push_str(
            " Run a long one (a build, a test suite, a download) with run_in_background: your \
             turn goes on, and you're told here when it ends.",
        );
    }
    text
}

/// What `{which}` says when it may start any program.
const ANY_PROGRAM: &str = "any program on its PATH";

/// Whether it may start any program: its sandbox file's `[commands]
/// allow` is `["*"]`.
fn any(run: &Run) -> bool {
    link_harness_contract::harness::Commands {
        allow: run.commands.clone(),
        toolchains: Vec::new(),
    }
    .any()
}

/// The shell a command runs in, as its programs name it
/// ([`link_run_tool_contract::shell`]). None: no command offered.
fn shell(run: &Run) -> Option<String> {
    link_run_tool_contract::shell(&run.commands, any(run))
}

/// The argument that runs a command in the background, as a model calls
/// it: the declared `run-in-background`.
const BACKGROUND_ARGUMENT: &str = "run_in_background";

/// The calls `actions.toml` declares, as its contract renders them.
const TEMPLATE: &str = link_run_tool_contract::TOOLS;

/// Its tools, from `actions.toml`: a command where the shell is among its
/// programs, which its description names, and the background calls
/// beside it where it keeps them. Without background commands, the
/// command's `run_in_background` isn't offered either.
fn offered(run: &Run) -> anyhow::Result<Value> {
    use link_actions::tools::{Fill, Fills};
    let which = match any(run) {
        true => ANY_PROGRAM.to_string(),
        false => run.commands.join(", "),
    };
    let shell = shell(run).is_some();
    let (longest, most) = match &run.background {
        Some(background) => (humane(background.timeout), background.at_once.to_string()),
        None => (String::new(), String::new()),
    };
    let fills = Fills {
        values: [
            ("which", Fill::Text(which)),
            ("timeout", Fill::Text(run.timeout.as_secs().to_string())),
            ("longest", Fill::Text(longest)),
            ("folder", Fill::Text(background::FOLDER.to_string())),
            ("most", Fill::Text(most)),
            ("output", Fill::Text(run.output.to_string())),
        ]
        .into(),
        conditions: [
            shell.then_some("shell"),
            (shell && run.background.is_some()).then_some("background"),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let mut tools = link_actions::tools::offer(TEMPLATE, &fills)?;
    if run.background.is_none() {
        for tool in tools.as_array_mut().into_iter().flatten() {
            if let Some(properties) = tool["inputSchema"]["properties"].as_object_mut() {
                properties.remove(BACKGROUND_ARGUMENT);
            }
        }
    }
    Ok(tools)
}

/// `duration` as a person says it: `2h`, `90m`, `45s`.
fn humane(duration: Duration) -> String {
    let secs = duration.as_secs();
    match secs {
        s if s % 3600 == 0 && s > 0 => format!("{}h", s / 3600),
        s if s % 60 == 0 && s > 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

/// A command run in the background: started under a supervisor of its
/// own, its id answered at once.
fn start_background(run: &Run, arguments: &Value, meta: &Value) -> anyhow::Result<(String, Value)> {
    let background = run
        .background
        .as_ref()
        .context("background commands aren't on here: run it without run_in_background")?;
    let thread = meta
        .get(link_harness_contract::connection::access::THREAD_META)
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .context(
            "a background command needs the conversation it's for, which this harness doesn't \
             say",
        )?;
    let hops = meta
        .get("link.local/hops")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32;
    let prepared = prepare(run, arguments, meta, background.timeout)?;
    let id = background::start(
        background,
        background::Starting {
            context: prepared.with.map(Arc::new),
            command: prepared.command,
            argv: prepared.argv,
            timeout: prepared.timeout,
            path: run.path.clone(),
            files: run.files.clone(),
            thread: thread.to_string(),
            hops,
            tail: (run.output - run.output / 2) as u64,
        },
    )?;
    let output = background::output_name(&id);
    Ok((
        format!(
            "Started {id}. It runs on its own: you'll be told here when it ends. Its output is \
             {output} in your files folder (task__read)."
        ),
        json!({"id": id, "output": output}),
    ))
}

/// A background command's call `name`: its answer in words and as a value.
#[expect(clippy::disallowed_methods, reason = "retries while the output is cut")]
fn call_background(
    run: &Run,
    name: &str,
    arguments: &Value,
    meta: &Value,
) -> anyhow::Result<(String, Value)> {
    let background = run
        .background
        .as_ref()
        .context("no background commands here")?;
    let state = &background.state;
    let id_of = || -> anyhow::Result<String> {
        Ok(arguments
            .get("id")
            .and_then(Value::as_str)
            .context("it needs the command's 'id'")?
            .to_string())
    };
    // Each conversation reaches only the background commands it started.
    let thread = || {
        meta.get(link_harness_contract::connection::access::THREAD_META)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .context(
                "a background command needs the conversation it's for, which this harness \
                 doesn't say",
            )
    };
    let ours = |id: &str| -> anyhow::Result<background::Record> {
        let record = background::load(state, id)?;
        anyhow::ensure!(
            record.thread == thread()?,
            "there's no background command {id} in this conversation"
        );
        Ok(record)
    };
    match name {
        "list" => {
            let (value, text) = background::list(state, thread()?);
            Ok((text, value))
        }
        "read" => {
            let id = id_of()?;
            background::sweep_quietly(state);
            let most = run.output as u64;
            // A cut its record doesn't say yet is read again: the record
            // follows within a tick.
            let mut tries = 0;
            let (record, read) = loop {
                let record = ours(&id)?;
                let total = record.bytes();
                let from = match arguments.get("from") {
                    None | Some(Value::Null) => total.saturating_sub(most),
                    Some(from) => from.as_u64().context("'from' is a byte, 0 or more")?,
                };
                match background::read_output(&record, from, most) {
                    Err(err) if err.is::<background::Cutting>() && tries < 20 => {
                        tries += 1;
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    read => break (record, read?),
                }
            };
            let mut text = format!(
                "{id}, {}: bytes {} to {} of {}",
                record.how(),
                read.from,
                read.to,
                read.total
            );
            if read.dropped > 0 {
                text.push_str(&format!(
                    " (its first {} bytes were dropped, past what's kept)",
                    read.dropped
                ));
            }
            let said = link_untrusted::fence(
                link_untrusted::Source {
                    kind: link_untrusted::Kind::Output,
                    name: &id,
                },
                &read.text,
            )?;
            text.push_str(&format!(":\n{said}"));
            // What it printed is in the text alone, not sent twice.
            Ok((
                text,
                json!({"id": id, "state": record.state, "from": read.from, "next": read.to,
                    "total": read.total, "dropped": read.dropped}),
            ))
        }
        "stop" => {
            let id = id_of()?;
            ours(&id)?;
            let record = background::stop(state, &id)?;
            Ok((
                format!(
                    "{id}: {}, after it ran {}s.",
                    record.how(),
                    record.value()["seconds"]
                ),
                record.value(),
            ))
        }
        other => bail!("no tool {other:?}"),
    }
}

/// What a command did.
pub struct Ran {
    /// Its exit status; none when a signal ended it.
    pub exit: Option<i32>,
    /// It ran past its timeout and was stopped.
    pub stopped: bool,
    pub stdout: Kept,
    pub stderr: Kept,
    pub took: Duration,
}

impl Ran {
    /// The answer as the model reads it: what `command` printed, each
    /// stream fenced as third-party text.
    pub fn text(&self, command: &str) -> anyhow::Result<String> {
        let mut text = match (self.stopped, self.exit) {
            (true, _) => format!(
                "stopped after {}s, still running at its timeout",
                self.took.as_secs()
            ),
            (false, Some(code)) => format!("exit {code}"),
            (false, None) => "ended by a signal".to_string(),
        };
        let fence = link_untrusted::Fence::new()?;
        for (name, kept) in [("output", &self.stdout), ("errors", &self.stderr)] {
            if kept.total > 0 {
                let source = link_untrusted::Source {
                    kind: link_untrusted::Kind::Output,
                    name: command,
                };
                text.push_str(&format!(
                    "\n\n{name}:\n{}",
                    fence.wrap(source, &kept.text())
                ));
            }
        }
        Ok(text)
    }

    /// The answer as a value: what it printed is in the text alone, not
    /// sent twice.
    pub fn value(&self) -> Value {
        json!({
            "exit": self.exit,
            "stopped": self.stopped,
            "stdout_bytes": self.stdout.total,
            "stderr_bytes": self.stderr.total,
            "seconds": self.took.as_secs_f64(),
        })
    }
}

/// What's kept of a stream: its start and its end, each up to half the
/// most kept, and how much there was.
#[derive(Debug, Default)]
pub struct Kept {
    most: usize,
    head: Vec<u8>,
    tail: VecDeque<u8>,
    pub total: u64,
}

impl Kept {
    pub fn new(most: usize) -> Self {
        Self {
            most,
            ..Self::default()
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        let head_room = (self.most / 2).saturating_sub(self.head.len());
        let (head, rest) = bytes.split_at(head_room.min(bytes.len()));
        self.head.extend_from_slice(head);
        let tail_most = self.most - self.most / 2;
        let rest = &rest[rest.len().saturating_sub(tail_most)..];
        self.tail.extend(rest);
        while self.tail.len() > tail_most {
            self.tail.pop_front();
        }
    }

    /// What's kept, with how much was cut between its start and its end.
    pub fn text(&self) -> String {
        let kept = self.head.len() + self.tail.len();
        let cut = self.total.saturating_sub(kept as u64);
        let tail: Vec<u8> = self.tail.iter().copied().collect();
        match cut {
            0 => String::from_utf8_lossy(&[self.head.as_slice(), &tail].concat()).into_owned(),
            cut => format!(
                "{}\n[... {cut} bytes cut ...]\n{}",
                String::from_utf8_lossy(&self.head),
                String::from_utf8_lossy(&tail)
            ),
        }
    }
}

/// A call's command, checked, with what runs it.
struct Prepared {
    /// The command, as the call gave it.
    command: String,
    /// The sandbox, then the shell found, `-c` and the command.
    argv: Vec<String>,
    /// The files folder, which it starts in.
    dir: Dir,
    /// How long it runs before it's stopped.
    timeout: Duration,
    /// What the sandbox reads on its descriptor ([`CONTEXT_FD`]): the
    /// context's routes and the variables the command gets, which the
    /// sandbox checks as a caller's.
    with: Option<link_sandbox_client::RunWith>,
}

/// What the sandbox is handed on its descriptor: the context, resolved
/// for this call, and the variables each command gets. None with
/// neither, or without a sandbox, where the variables are the command's
/// own.
fn run_with(run: &Run) -> anyhow::Result<Option<link_sandbox_client::RunWith>> {
    let mut with = run
        .context
        .as_ref()
        .map(|context| context.resolve())
        .transpose()?;
    if !run.env.is_empty() && !run.sandbox.is_empty() {
        let with = with.get_or_insert_with(Default::default);
        for (name, value) in &run.env {
            ensure!(
                with.env.insert(name.clone(), value.clone()).is_none(),
                "--var {name} is given twice"
            );
        }
    }
    Ok(with)
}

/// The call's command, as `arguments` say, run by the shell found on its
/// PATH, in the files folder, for at most `longest`: a timeout past it,
/// or none, is `longest`.
fn prepare(
    run: &Run,
    arguments: &Value,
    meta: &Value,
    longest: Duration,
) -> anyhow::Result<Prepared> {
    if let Some(context) = &run.context {
        context.check_variables(&run.env)?;
    }
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .context("it needs its 'command'")?;
    let limits = link_run_tool_contract::limits();
    ensure!(
        command.len() <= limits.command_max,
        "a command is at most {} bytes",
        limits.command_max
    );
    ensure!(!command.contains('\0'), "a command can't hold a NUL");
    let timeout = match arguments.get("timeout") {
        None | Some(Value::Null) => longest,
        Some(ms) => {
            let ms = ms
                .as_u64()
                .filter(|ms| *ms > 0)
                .context("'timeout' is milliseconds, 1 or more")?;
            Duration::from_millis(ms).min(longest)
        }
    };
    let shell = shell(run).with_context(|| {
        format!(
            "a command needs the shell ({}), which isn't a program you may run here; you may \
             run {}",
            limits.shell,
            run.commands.join(", ")
        )
    })?;
    let file = link_harness_contract::harness::find_program(&shell, &run.path)
        .with_context(|| format!("{shell} isn't on this machine"))?;
    let dir = Dir::open(&run.files)
        .with_context(|| format!("opening the files folder {}", run.files.display()))?;
    let mut argv: Vec<String> = run.sandbox.clone();
    ensure!(
        run.context.is_none() || !argv.is_empty(),
        "run context requires a sandbox"
    );
    let with = run_with(run)?;
    if !argv.is_empty() {
        if with.is_some() {
            argv.extend(["--with-fd".into(), CONTEXT_FD.to_string()]);
        }
        if let Some(own) = own_conversation(run, meta) {
            argv.extend(["--files.read".into(), own.display().to_string()]);
        }
        argv.push("--".into());
    }
    argv.extend([file.display().to_string(), "-c".into(), command.into()]);
    Ok(Prepared {
        command: command.into(),
        argv,
        dir,
        timeout,
        with,
    })
}

/// Runs the call's command, as `arguments` say, for the caller `meta`
/// (a call's `_meta`) names.
pub fn command(run: &Run, arguments: &Value, meta: &Value) -> anyhow::Result<Ran> {
    command_in(run, arguments, meta, None)
}

/// [`command`], as call `entered` of its session, which a cancel stops.
fn command_in(
    run: &Run,
    arguments: &Value,
    meta: &Value,
    entered: Option<&Entered>,
) -> anyhow::Result<Ran> {
    let prepared = prepare(run, arguments, meta, run.timeout)?;
    spawn(run, prepared, entered)
}

/// The folder of [`Run::conversations`] the caller `meta` names may read:
/// its own conversation's, when its harness names one and its message is
/// the owner's or a member's, and the folder is there. A stranger's call,
/// or one no harness names, reads none.
fn own_conversation(run: &Run, meta: &Value) -> Option<PathBuf> {
    use link_harness_contract::connection::access::{STANDING_META, Standing, THREAD_META};
    let conversations = run.conversations.as_ref()?;
    let standing = meta
        .get(STANDING_META)
        .and_then(|s| serde_json::from_value::<Standing>(s.clone()).ok());
    if !matches!(standing, Some(Standing::Owner | Standing::Member)) {
        return None;
    }
    let thread = meta
        .get(THREAD_META)
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())?;
    let own = conversations.join(link_harness_contract::store::folder::folder_of(thread));
    own.is_dir().then_some(own)
}

/// Whether child `pid` ended, without reaping it: until it's reaped, its
/// ID, its process group's, can't be another's.
pub(crate) fn exited(pid: i32) -> bool {
    // SAFETY: a zeroed siginfo_t, filled by waitid for our own child,
    // which WNOWAIT leaves to be reaped.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        ) == 0
            && info.si_signo == libc::SIGCHLD
    }
}

/// Runs what `prepared` says, in the files folder by its descriptor, with
/// nothing on its input, in a process group of its own, stopped with everything it
/// started at the timeout, when its call is cancelled, or when this
/// server ends.
#[expect(clippy::disallowed_methods, reason = "polls the running program")]
fn spawn(run: &Run, prepared: Prepared, entered: Option<&Entered>) -> anyhow::Result<Ran> {
    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::process::CommandExt;
    let Prepared {
        argv,
        dir,
        timeout,
        with,
        ..
    } = prepared;
    let started = Instant::now();
    let mut command = link_secretive::process::command(&argv[0]);
    command
        .args(&argv[1..])
        .env("PATH", &run.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // Without a sandbox, its variables are the command's own.
    if run.sandbox.is_empty() {
        command.envs(run.env.iter().map(|(name, value)| (name, value)));
    }
    // It starts in the files folder opened, by its descriptor: one
    // swapped for a link since is never followed.
    let folder = dir.as_fd().as_raw_fd();
    // SAFETY: only fchdir between fork and exec, on a descriptor held
    // open until the child started.
    unsafe {
        command.pre_exec(move || match libc::fchdir(folder) {
            0 => Ok(()),
            _ => Err(std::io::Error::last_os_error()),
        });
    }
    // It, its group and its sandbox end with this server, however it
    // ends, on every system: watched by the process, not this call's
    // thread.
    link_secretive::process::watched_by_parent(&mut command, libc::SIGKILL)
        .context("watching the command")?;
    let handoff = with
        .as_ref()
        .map(|with| with.hand_to(&mut command, CONTEXT_FD))
        .transpose()?;
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {}", argv[0]))?;
    drop(dir);
    if let Some(handoff) = handoff
        && let Err(error) = handoff.send()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let group = child.id() as i32;
    let stop = move || kill_group(group);
    // Cancelled before it started: it's stopped at once.
    if entered.is_some_and(|entered| entered.started(group)) {
        stop();
    }
    let read = |stream: Option<Box<dyn Read + Send>>| {
        let kept = Arc::new(Mutex::new(Kept::new(run.output)));
        let (into, done) = (kept.clone(), std::sync::mpsc::channel::<()>());
        if let Some(mut stream) = stream {
            let sender = done.0;
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                while let Ok(n @ 1..) = stream.read(&mut buffer) {
                    into.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(&buffer[..n]);
                }
                let _ = sender.send(());
            });
        }
        (kept, done.1)
    };
    let (stdout, stdout_done) = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let (stderr, stderr_done) = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let deadline = started + timeout;
    // Its leader is reaped only once its group is stopped: till then its
    // ID, the group's, can't be another's.
    let stopped = loop {
        if exited(group) {
            break false;
        }
        if Instant::now() >= deadline {
            stop();
            break true;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // What it printed, once its streams close; what it left running is
    // stopped after a moment.
    let drained = Instant::now() + link_run_tool_contract::limits().drain();
    for done in [&stdout_done, &stderr_done] {
        let left = drained.saturating_duration_since(Instant::now());
        let _ = done.recv_timeout(left);
    }
    // A group already gone is no harm.
    stop();
    if let Some(entered) = entered {
        entered.ending();
    }
    let status = child.wait().ok();
    let take = |kept: Arc<Mutex<Kept>>| {
        std::mem::take(&mut *kept.lock().unwrap_or_else(|e| e.into_inner()))
    };
    Ok(Ran {
        exit: status.and_then(|s| s.code()),
        stopped,
        stdout: take(stdout),
        stderr: take(stderr),
        took: started.elapsed(),
    })
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: String) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests;

#[cfg(test)]
link_test_kit::program!(system);
