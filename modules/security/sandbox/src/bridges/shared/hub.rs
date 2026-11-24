//! A shared tool's hub: its one server, started and initialized once,
//! and each session's messages passed on with ids of its own.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::bail;
use link_format::WithinExt;
use serde_json::{Value, json};

use super::{MAX_LOG, key, socket};
use crate::bridges::tools::Tool;

/// The largest message passed on, each way, as the proxy's.
const MAX_MESSAGE: u64 = link_sandbox_client::routes::TOOL_MESSAGE_MAX as u64;

/// The most answers a session may have waiting for it to read: past
/// them it has stopped reading, and is ended, as if it had left.
const UNREAD_ANSWERS: usize = 64;

/// The protocol version the hub asks its server for.
const VERSION: &str = "2025-06-18";

/// Passes `from` on to `to`, at most `max` bytes of it, then a line
/// saying the rest is left out; reads it all, so its writer never waits.
fn copy_capped(mut from: impl std::io::Read, mut to: impl Write, max: u64) {
    let mut written = 0u64;
    let mut buf = [0u8; 8192];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if written < max {
            let keep = n.min((max - written) as usize);
            let _ = to.write_all(&buf[..keep]);
            written += keep as u64;
            if written >= max {
                let _ = writeln!(
                    to,
                    "\n(the rest of the server's output is left out of this log)"
                );
                let _ = to.flush();
            }
        }
    }
}

/// The hub's state: its server's input, the requests it waits on, and
/// the sessions connected.
struct Hub {
    /// Its server's input, until the hub stops and closes it.
    input: Mutex<Option<std::process::ChildStdin>>,
    /// The hub's id of each request passed on: the session that asked,
    /// and its own id.
    pending: Mutex<HashMap<u64, (u64, Value)>>,
    /// Each session's connection, to answer it on.
    sessions: Mutex<HashMap<u64, UnixStream>>,
    /// Each answered session's answers, waiting for its own thread to
    /// write them ([`Hub::answer`]).
    writers: Mutex<HashMap<u64, std::sync::mpsc::SyncSender<Vec<u8>>>>,
    next: AtomicU64,
    /// What the server answered `initialize` with, every session's.
    initialized: Value,
    /// When the last session ended, while none is open.
    idle_since: Mutex<Option<Instant>>,
}

impl Hub {
    /// Closes the server's input: its sign to end, closing what it
    /// started, such as a browser.
    fn close(&self) {
        if let Ok(mut input) = self.input.lock() {
            input.take();
        }
    }

    fn write(&self, message: &Value) -> anyhow::Result<()> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        let mut input = self
            .input
            .lock()
            .map_err(|_| anyhow::anyhow!("the server's input"))?;
        let input = input
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("the server's input is closed"))?;
        input.write_all(&line)?;
        input.flush()?;
        Ok(())
    }

    /// Passes `message` to `session`'s own thread, which writes it, in
    /// order, never waiting: a session that stops reading holds no other's
    /// answers, nor the thread that reads the server. One with
    /// [`UNREAD_ANSWERS`] unread is ended.
    fn answer(&self, session: u64, message: &Value) {
        use std::collections::hash_map::Entry;
        let mut line = serde_json::to_vec(message).unwrap_or_default();
        line.push(b'\n');
        let (Ok(sessions), Ok(mut writers)) = (self.sessions.lock(), self.writers.lock()) else {
            return;
        };
        // Those of sessions gone end, once they've written what they had.
        writers.retain(|s, _| sessions.contains_key(s));
        let Some(stream) = sessions.get(&session) else {
            return;
        };
        let writer = match writers.entry(session) {
            Entry::Occupied(writer) => writer.into_mut(),
            Entry::Vacant(vacant) => {
                let Ok(mut out) = stream.try_clone() else {
                    return;
                };
                let (writer, lines) = std::sync::mpsc::sync_channel::<Vec<u8>>(UNREAD_ANSWERS);
                let spawned = std::thread::Builder::new().spawn(move || {
                    for line in lines {
                        if out.write_all(&line).is_err() {
                            // Gone, or ended: the rest dropped.
                            let _ = out.shutdown(std::net::Shutdown::Both);
                            return;
                        }
                    }
                });
                if spawned.is_err() {
                    return;
                }
                vacant.insert(writer)
            }
        };
        if let Err(std::sync::mpsc::TrySendError::Full(_)) = writer.try_send(line) {
            // It stopped reading: ended, so its session ends too.
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// `lnk sandbox mcp-shared <name> <key>`: tool `name`'s hub, as it's
/// defined with that key ([`key`]), for as long as a session uses it;
/// nothing when another hub already runs it.
#[expect(
    clippy::disallowed_methods,
    reason = "polls its lock and idleness, and rests after a failed accept"
)]
pub fn serve(
    runtime: &Path,
    dir: &Path,
    name: &str,
    defined: &str,
    idle: Duration,
) -> anyhow::Result<()> {
    let tool = crate::bridges::tools::load(dir, name)?;
    anyhow::ensure!(tool.shared, "tool {name} isn't shared: its proxy runs it");
    anyhow::ensure!(
        key(dir, &tool)? == defined,
        "tool {name} was defined again: its next session starts its new server"
    );
    let socket = socket(runtime, dir, &tool)?;
    link_files::create_private_dir(socket.parent().expect("a shared tool has a runtime folder"))?;
    let lock_path = socket.with_extension("lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .within(format!("opening {}", lock_path.display()))?;
    // Another hub runs it, or one going idle is about to stop: waited
    // for a moment, then this one runs it.
    let tried = Instant::now();
    loop {
        // SAFETY: flock on a descriptor this process holds open.
        let locked = unsafe {
            libc::flock(
                std::os::fd::AsRawFd::as_raw_fd(&lock_file),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        };
        if locked == 0 {
            break;
        }
        if link_files::nofollow::connect(&socket).is_ok()
            || tried.elapsed() > Duration::from_secs(5)
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let listener = link_sandbox_bridge::socket::listen_private(&socket)?;
    listener.set_nonblocking(true)?;
    let (stop, stopped) = std::sync::mpsc::channel();
    let ended = || {
        let _ = std::fs::remove_file(&socket);
        // The lock's inode stays: unlinking a held lock would let the
        // next hub lock another file and overlap this one's shutdown.
    };
    let started = start_server(&tool);
    let (child, hub, output) = match started {
        Ok(started) => started,
        Err(err) => {
            ended();
            return Err(err);
        }
    };
    let hub = Arc::new(hub);
    let finished = stop.clone();
    // Its server's answers, each to the session that asked.
    let reading = hub.clone();
    std::thread::spawn(move || {
        read_server(&reading, output);
        // The server ended: so does the hub, and every session with it.
        let _ = finished.send(());
    });
    // Gone idle: stopped, its socket first, so no session reaches a hub
    // that's going.
    let watching = hub.clone();
    let idle_socket = socket.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(200).min(idle));
            let since = watching.idle_since.lock().ok().and_then(|s| *s);
            if since.is_some_and(|since| since.elapsed() >= idle) {
                let _ = std::fs::remove_file(&idle_socket);
                let _ = stop.send(());
                return;
            }
        }
    });
    for stream in listener.incoming() {
        if stopped.try_recv().is_ok() {
            break;
        }
        let Ok(stream) = stream else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        let Ok(answers) = stream.try_clone() else {
            continue;
        };
        let id = hub.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut sessions) = hub.sessions.lock() {
            sessions.insert(id, answers);
        }
        if let Ok(mut since) = hub.idle_since.lock() {
            *since = None;
        }
        let hub = hub.clone();
        std::thread::spawn(move || {
            read_session(&hub, id, BufReader::new(stream));
            let open = match hub.sessions.lock() {
                Ok(mut sessions) => {
                    sessions.remove(&id);
                    sessions.len()
                }
                Err(_) => 0,
            };
            if let Ok(mut pending) = hub.pending.lock() {
                pending.retain(|_, (session, _)| *session != id);
            }
            if open == 0
                && let Ok(mut since) = hub.idle_since.lock()
            {
                *since = Some(Instant::now());
            }
        });
    }
    // Its input closed first, so it ends on its own if it can.
    hub.close();
    drop(child);
    ended();
    Ok(())
}

/// The server owns a process group. Every exit path, including a failed
/// initialization, ends its group ([`crate::bridges::stop`]) and reaps
/// the direct child while the hub still exists. Cleanup never depends on
/// the host's init reaper.
struct Server(std::process::Child);
impl std::ops::Deref for Server {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Server {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        // Its input, where it's still held here, closed first.
        drop(self.0.stdin.take());
        crate::bridges::stop::server(self.0.id(), crate::settings::get().tool_stop);
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What reads the server's output.
type Output = BufReader<std::process::ChildStdout>;

/// `tool`'s server, started and initialized once.
fn start_server(tool: &Tool) -> anyhow::Result<(Server, Hub, Output)> {
    let mut command = crate::bridges::tools::server(tool);
    link_secretive::process::ends_with_parent(&mut command, libc::SIGKILL);
    let mut child = Server(
        command
            .process_group(0)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .within(format!(
                "starting tool {}: {}",
                tool.name,
                tool.argv.join(" ")
            ))?,
    );
    // Its output, to the hub's log, at most MAX_LOG of it.
    if let Some(errors) = child.stderr.take() {
        std::thread::spawn(move || {
            copy_capped(
                errors,
                link_format::streams::passed(link_format::streams::Stream::Err),
                MAX_LOG,
            )
        });
    }
    let mut input = child.stdin.take().within("the server's input")?;
    let output = child.stdout.take().within("the server's output")?;
    let mut output = BufReader::new(output);
    let ask = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
        "protocolVersion": VERSION,
        "capabilities": {},
        "clientInfo": {"name": "link-sandbox", "version": env!("CARGO_PKG_VERSION")}}});
    writeln!(input, "{ask}")?;
    input.flush()?;
    let initialized = loop {
        let Some(line) = read_line(&mut output)? else {
            bail!("tool {} ended before it started", tool.name);
        };
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        match (message.get("id"), message.get("method")) {
            (Some(id), None) if *id == json!(0) => {
                break message
                    .get("result")
                    .cloned()
                    .within(format!("tool {} refused to start: {message}", tool.name))?;
            }
            // Its requests of the client, refused as ever.
            (Some(id), Some(_)) => writeln!(
                input,
                "{}",
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601,
                    "message": "Link's sandbox doesn't pass a tool's requests to the client"}})
            )?,
            _ => {}
        }
    };
    writeln!(
        input,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )?;
    input.flush()?;
    Ok((
        child,
        Hub {
            input: Mutex::new(Some(input)),
            pending: Default::default(),
            sessions: Default::default(),
            writers: Default::default(),
            next: AtomicU64::new(1),
            initialized,
            // Idle from its start: a hub no session reaches stops too.
            idle_since: Mutex::new(Some(Instant::now())),
        },
        output,
    ))
}

/// One session's messages, until it ends: `initialize` answered with the
/// server's own, once; requests passed on with ids of the hub's; of its
/// notifications, a cancel, its request's id made the hub's. The rest
/// the server already had once.
fn read_session(hub: &Hub, session: u64, mut input: BufReader<UnixStream>) {
    while let Ok(Some(line)) = read_line(&mut input) {
        let Ok(mut message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match (message.get("id").cloned(), method.as_str()) {
            (Some(id), "initialize") => {
                hub.answer(
                    session,
                    &json!({"jsonrpc": "2.0", "id": id, "result": hub.initialized}),
                );
            }
            (Some(id), method) if !method.is_empty() => {
                let ours = hub.next.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut pending) = hub.pending.lock() {
                    pending.insert(ours, (session, id.clone()));
                }
                message["id"] = json!(ours);
                if hub.write(&message).is_err() {
                    hub.answer(
                        session,
                        &json!({"jsonrpc": "2.0", "id": id, "error": {
                            "code": -32603, "message": "the shared tool's server is gone"}}),
                    );
                }
            }
            (None, "notifications/cancelled") => {
                let asked = message.pointer("/params/requestId").cloned();
                let ours = hub.pending.lock().ok().and_then(|pending| {
                    pending
                        .iter()
                        .find(|(_, (s, id))| *s == session && Some(id) == asked.as_ref())
                        .map(|(ours, _)| *ours)
                });
                if let Some(ours) = ours {
                    message["params"]["requestId"] = json!(ours);
                    let _ = hub.write(&message);
                }
            }
            _ => {}
        }
    }
}

/// The server's messages: each answer to the session that asked, with
/// its own id; a ping answered here; its other requests refused, as the
/// proxy refuses them; its notifications dropped. What it says to the
/// server is written on a thread of its own: this one alone reads the
/// server, which may be blocked writing to it meanwhile, and the server
/// blocked on a full input with it.
fn read_server(hub: &Hub, mut output: Output) {
    std::thread::scope(|scope| {
        let (to_server, said) = std::sync::mpsc::channel::<Value>();
        scope.spawn(move || {
            for message in said {
                let _ = hub.write(&message);
            }
        });
        read_server_lines(hub, &mut output, &to_server);
        // Its writer ends with what it had.
        drop(to_server);
    });
}

/// [`read_server`]'s lines, until the server's output ends; what it says
/// to the server sent to `to_server`.
fn read_server_lines(hub: &Hub, output: &mut Output, to_server: &std::sync::mpsc::Sender<Value>) {
    while let Ok(Some(line)) = read_line(output) {
        let Ok(mut message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        match (message.get("id").cloned(), message.get("method").cloned()) {
            (Some(id), Some(method)) => {
                let answer = match method.as_str() {
                    Some("ping") => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                    _ => json!({"jsonrpc": "2.0", "id": id, "error": {
                        "code": -32601,
                        "message": "Link's sandbox doesn't pass a tool's requests to the client"
                    }}),
                };
                let _ = to_server.send(answer);
            }
            (Some(id), None) => {
                let asked = id
                    .as_u64()
                    .and_then(|ours| hub.pending.lock().ok()?.remove(&ours));
                if let Some((session, theirs)) = asked {
                    message["id"] = theirs;
                    hub.answer(session, &message);
                }
            }
            _ => {}
        }
    }
}

/// One line of at most [`MAX_MESSAGE`], without its end; none at the end.
fn read_line(reader: &mut impl BufRead) -> anyhow::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let n = std::io::Read::take(&mut *reader, MAX_MESSAGE + 1).read_until(b'\n', &mut line)?;
    if n == 0 {
        return Ok(None);
    }
    anyhow::ensure!(
        line.len() as u64 <= MAX_MESSAGE,
        "a message over {} MB",
        MAX_MESSAGE >> 20
    );
    while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        line.pop();
    }
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The server's output is kept up to the most, then said to be cut,
    /// and read to its end all the same.
    #[test]
    fn the_log_is_capped() {
        let from = [b'x'; 100];
        let mut to = Vec::new();
        copy_capped(&from[..], &mut to, 10);
        let text = String::from_utf8(to).unwrap();
        assert!(text.starts_with("xxxxxxxxxx\n(the rest"), "{text}");
        assert_eq!(text.matches('x').count(), 10);
        let mut to = Vec::new();
        copy_capped(&b"short"[..], &mut to, 10);
        assert_eq!(to, b"short");
    }

    use std::io::Read;

    /// A hub for a server running `script` (`sh -c`), initialized, with
    /// what reads the server.
    fn hub(script: &str) -> (Server, Arc<Hub>, Output) {
        let argv = ["sh", "-c", script].map(String::from).to_vec();
        let tool = Tool::here("fake", argv, &[], Default::default(), vec![]).unwrap();
        let (child, hub, output) = start_server(&tool).unwrap();
        (child, Arc::new(hub), output)
    }

    /// Answers the server's `initialize`, and takes its notification.
    const STARTED: &str = r#"read -r l; printf '{"jsonrpc":"2.0","id":0,"result":{"capabilities":{}}}\n'; read -r l;"#;

    /// A session of `hub`'s: the hub's side kept, the session's given.
    fn session(hub: &Hub, id: u64) -> UnixStream {
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.sessions.lock().unwrap().insert(id, ours);
        theirs
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        theirs
    }

    fn line(stream: &UnixStream) -> String {
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        line
    }

    /// A session that stops reading holds no other's answers: answering
    /// it never waits, and past its unread answers it's ended.
    #[test]
    fn a_session_that_stops_reading_holds_no_others_answers() {
        let (mut child, hub, _output) = hub(&format!("{STARTED} cat >/dev/null"));
        let (stuck, other) = (session(&hub, 1), session(&hub, 2));
        let big = json!({"jsonrpc": "2.0", "id": 1, "result": "x".repeat(64 * 1024)});
        let started = Instant::now();
        for _ in 0..200 {
            hub.answer(1, &big);
        }
        hub.answer(2, &json!({"jsonrpc": "2.0", "id": 9, "result": "yours"}));
        assert!(started.elapsed() < Duration::from_secs(5));
        let answered = line(&other);
        assert!(answered.contains(r#""id":9"#), "{answered}");
        // Ended: what it was sent, then the end.
        let mut rest = Vec::new();
        (&stuck).read_to_end(&mut rest).unwrap();
        assert!(rest.len() < 200 * 64 * 1024);
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A server that sends pings without reading their answers until it's
    /// done never stops the hub reading it: its answers wait for it.
    #[test]
    fn a_server_pinging_with_its_input_full_is_still_read() {
        let script = format!(
            r#"{STARTED} i=0; while [ $i -lt 5000 ]; do printf '{{"jsonrpc":"2.0","id":"p%s","method":"ping"}}\n' $i; i=$((i+1)); done; printf '{{"jsonrpc":"2.0","id":7,"result":"done"}}\n'; cat >/dev/null"#
        );
        let (mut child, hub, output) = hub(&script);
        let asking = session(&hub, 1);
        hub.pending.lock().unwrap().insert(7, (1, json!(41)));
        let reading = hub.clone();
        std::thread::spawn(move || read_server(&reading, output));
        let answered = line(&asking);
        assert!(
            answered.contains(r#""id":41"#) && answered.contains("done"),
            "{answered}"
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}
