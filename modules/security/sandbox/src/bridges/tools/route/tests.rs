use super::*;
use crate::bridges::tools::{Ask, save};

#[test]
fn a_question_shows_no_escapes_and_the_servers_word_as_its_own() {
    let arguments = json!({"repo": "me/x\x1b]52;c;evil\x07", "pad": "y".repeat(2000)});
    let asked = question(
        "main-otter",
        "gh",
        "delete_repo\x1b[2J",
        &arguments,
        &json!({"destructiveHint": true}),
    );
    let text = asked.text();
    assert!(!text.contains('\x1b') && !text.contains('\x07'), "{text}");
    assert!(
        text.contains("run tool gh's delete_repo") && text.contains("may delete"),
        "{text}"
    );
    assert!(text.len() > 2_000, "full arguments remain reviewable");
    let read_only = question(
        "s",
        "gh",
        "list",
        &json!({}),
        &json!({"readOnlyHint": true}),
    );
    assert!(read_only.text().contains("its own word"));
}

/// A tool `name` in the sandboxes folder of a new installation folder (its
/// server's `$HERE`), its server `script` (sh), and the sandbox's tools
/// naming it.
fn tool_running(name: &str, script: &str) -> (&'static Tools, link_test_kit::TempDir) {
    let root = link_test_kit::TempDir::new(&format!("tools-{name}"));
    let dir = link_files::layout::Hidden::Sandboxes.at(&root);
    std::fs::create_dir_all(&dir).unwrap();
    let tool = Tool {
        name: name.into(),
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        env: [
            ("PATH".to_string(), "/usr/bin:/bin".to_string().into()),
            ("HERE".to_string(), dir.display().to_string().into()),
        ]
        .into(),
        ask: Ask::None,
        allow: vec![],
        ask_if: Vec::new(),
        ask_if_hint: None,
        shared: false,
    };
    save(&dir, &tool).unwrap();
    let tools = Box::leak(Box::new(Tools::new("t", dir, vec![name.into()])));
    (tools, root)
}

/// `message` posted to tool `name` (in `session`), served on one end
/// of a pipe; the other end is the client's.
fn post(
    tools: &'static Tools,
    name: &str,
    session: Option<&str>,
    message: &Value,
) -> (
    tokio::io::DuplexStream,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    (client, served(server, tools, name, session, message))
}

/// [`post`], over a Unix socket, as the proxy is reached on Linux.
#[cfg(target_os = "linux")]
fn post_unix(
    tools: &'static Tools,
    name: &str,
    session: Option<&str>,
    message: &Value,
) -> (
    tokio::net::UnixStream,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (client, server) = tokio::net::UnixStream::pair().unwrap();
    (client, served(server, tools, name, session, message))
}

/// `message` posted to tool `name` (in `session`), served on `server`.
fn served<S>(
    server: S,
    tools: &'static Tools,
    name: &str,
    session: Option<&str>,
    message: &Value,
) -> tokio::task::JoinHandle<anyhow::Result<()>>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    link_sandbox_route::Unsaid<S>: Hangup,
{
    let body = serde_json::to_vec(message).unwrap();
    let mut head = format!(
        "POST {}{name} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n",
        link_sandbox_client::routes::TOOLS,
        body.len()
    );
    if let Some(id) = session {
        head.push_str(&format!("Mcp-Session-Id: {id}\r\n"));
    }
    head.push_str("\r\n");
    tokio::spawn(async move {
        let server = link_sandbox_route::Unsaid(server);
        serve(server, head.as_bytes(), body, tools, &Authority::Run).await
    })
}

/// A server answering each request after a moment.
#[cfg(target_os = "linux")]
const SLOW_ANSWERS: &str = r#"while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}]*\).*/\1/p')
  if [ -n "$id" ]; then
    sleep 0.5
    printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
  fi
done"#;

/// An agent calls a tool its route named as its proxy started only
/// while it's granted it at the call: taken back since, it's refused
/// before its server starts.
#[tokio::test]
async fn a_tool_taken_back_since_is_refused() {
    let (tools, _dir) = tool_running("taken-back", "exit 1");
    let now = |granted: &[&str]| {
        Authority::Agent(link_sandbox_client::grants::Current {
            tools: granted.iter().map(|t| t.to_string()).collect(),
            browser: None,
        })
    };
    let body = serde_json::to_vec(&initialize()).unwrap();
    let head = format!(
        "POST {}taken-back HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n",
        link_sandbox_client::routes::TOOLS,
        body.len()
    );
    for (authority, refused) in [(now(&[]), true), (now(&["other"]), true)] {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let (head, body) = (head.clone(), body.clone());
        let serving = tokio::spawn(async move {
            let server = link_sandbox_route::Unsaid(server);
            serve(server, head.as_bytes(), body, tools, &authority).await
        });
        let mut answer = String::new();
        client.read_to_string(&mut answer).await.unwrap();
        let served = serving.await.unwrap();
        assert_eq!(served.is_err(), refused, "{answer}");
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
        assert!(answer.contains("granted now"), "{answer}");
    }
}

/// A sandbox whose grants couldn't be learned is refused with the
/// keeper's own reason and the steps that help, never as a tool it
/// isn't granted: that reason is what reaches the agent's log.
#[tokio::test]
async fn an_unanswered_grant_says_why() {
    let head = format!(
        "POST {}task HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n",
        link_sandbox_client::routes::TOOLS
    );
    let (mut client, server) = tokio::io::duplex(64 * 1024);
    let serving = tokio::spawn(async move {
        let server = link_sandbox_route::Unsaid(server);
        unanswered(server, head.as_bytes(), "the agent module couldn't answer").await
    });
    let mut answer = String::new();
    client.read_to_string(&mut answer).await.unwrap();
    assert!(serving.await.unwrap().is_err());
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert!(answer.contains("task wasn't checked"), "{answer}");
    assert!(
        answer.contains("the agent module couldn't answer"),
        "{answer}"
    );
    assert!(answer.contains("lnk upgrade"), "{answer}");
    assert!(!answer.contains("granted now"), "{answer}");
}

/// A client that shuts only its writing after its request (a
/// half-close) hasn't left: its start and its call are answered.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_client_that_only_shuts_its_writing_is_answered() {
    let (tools, _dir) = tool_running("half-closed", SLOW_ANSWERS);
    let (mut client, serving) = post_unix(tools, "half-closed", None, &initialize());
    client.shutdown().await.unwrap();
    let mut answer = String::new();
    client.read_to_string(&mut answer).await.unwrap();
    serving.await.unwrap().unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    let id = answer
        .lines()
        .find_map(|l| l.strip_prefix("Mcp-Session-Id: "))
        .expect("a session")
        .trim()
        .to_string();
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let (mut client, serving) = post_unix(tools, "half-closed", Some(&id), &list);
    client.shutdown().await.unwrap();
    let mut answer = String::new();
    client.read_to_string(&mut answer).await.unwrap();
    serving.await.unwrap().unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(answer.contains(r#""id":2"#), "{answer}");
    tools.end(&id);
}

/// A client that closes its socket after shutting its writing has
/// left: its call is let go.
#[cfg(target_os = "linux")]
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_half_closed_client_that_then_closes_is_let_go() {
    let script = r#"while IFS= read -r line; do
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}]*\).*/\1/p')
      case "$line" in *'"method":"initialize"'*)
        printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
      esac
    done"#;
    let (tools, _dir) = tool_running("never-answers-unix", script);
    let (mut client, serving) = post_unix(tools, "never-answers-unix", None, &initialize());
    let mut answer = String::new();
    client.read_to_string(&mut answer).await.unwrap();
    serving.await.unwrap().unwrap();
    let id = answer
        .lines()
        .find_map(|l| l.strip_prefix("Mcp-Session-Id: "))
        .expect("a session")
        .trim()
        .to_string();
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let (mut client, serving) = post_unix(tools, "never-answers-unix", Some(&id), &list);
    client.shutdown().await.unwrap();
    let session = tools.session(&id, "never-answers-unix").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while session.pending.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline, "never asked");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Still waited for, half-closed.
    tokio::time::sleep(link_sandbox_route::io::HANGUP_CHECK * 2).await;
    assert!(!serving.is_finished());
    drop(client);
    let ended = tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("let go once the client left")
        .unwrap();
    assert!(ended.is_err());
    assert!(session.pending.lock().unwrap().is_empty());
    tools.end(&id);
}

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "t", "version": "1"}
    }})
}

/// One sandbox can't starve another's tools: one whose every start
/// is taken still leaves another sandbox's tools to start.
#[tokio::test]
async fn a_sandboxs_starts_are_its_own() {
    let answers = r#"read -r line; printf '{"jsonrpc":"2.0","id":1,"result":{}}\n'; exec sleep 60"#;
    let (busy, _busy_root) = tool_running("busy", answers);
    let (other, _other_root) = tool_running("other", answers);
    let _taken = busy
        .starting
        .acquire_many(crate::settings::get().tool_starts as u32)
        .await
        .unwrap();
    let tool = load(&other.dir, "other").unwrap();
    let started = tokio::time::timeout(Duration::from_secs(10), start(other, &tool, &initialize()))
        .await
        .expect("another sandbox's start never waits on this one's");
    assert!(started.is_ok());
}

/// A start its client gave up on (it closed its connection) is given
/// up too: the server is stopped, and no session is kept.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_start_whose_client_left_is_given_up() {
    let (tools, _root) = tool_running("slow-start", "echo $$ > \"$HERE/pid\"; exec sleep 60");
    let (client, serving) = post(tools, "slow-start", None, &initialize());
    let pid_file = tools.dir.join("pid");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    // The shell makes the file before it writes its pid: wait for both.
    let read_pid = || {
        std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok())
    };
    while read_pid().is_none() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let pid = read_pid().expect("the server wrote its pid");
    drop(client);
    let ended = tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("given up once the client left")
        .unwrap();
    assert!(ended.is_err());
    assert!(tools.sessions.lock().unwrap().is_empty());
    // Its server is stopped (gone, or a zombie not yet waited for).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let state = link_secretive::process::command("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&state.stdout).trim().to_string();
        if state.is_empty() || state.starts_with('Z') {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "still running: {state}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A call that never answers holds its connection only until the
/// client leaves; the session stays for its next call.
#[tokio::test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
async fn a_call_whose_client_left_is_let_go() {
    let script = r#"while IFS= read -r line; do
      id=$(printf '%s' "$line" | sed -n 's/.*"id":\([^,}]*\).*/\1/p')
      case "$line" in *'"method":"initialize"'*)
        printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
      esac
    done"#;
    let (tools, _dir) = tool_running("never-answers", script);
    let (mut client, serving) = post(tools, "never-answers", None, &initialize());
    let mut answer = String::new();
    client.read_to_string(&mut answer).await.unwrap();
    serving.await.unwrap().unwrap();
    let id = answer
        .lines()
        .find_map(|l| l.strip_prefix("Mcp-Session-Id: "))
        .expect("a session")
        .trim()
        .to_string();
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let (client, serving) = post(tools, "never-answers", Some(&id), &list);
    let session = tools.session(&id, "never-answers").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while session.pending.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline, "never asked");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(client);
    let ended = tokio::time::timeout(Duration::from_secs(5), serving)
        .await
        .expect("let go once the client left")
        .unwrap();
    assert!(ended.is_err());
    assert!(session.pending.lock().unwrap().is_empty());
    assert!(tools.session(&id, "never-answers").is_some());
    tools.end(&id);
}

/// A sandbox with more tools than `tool_sessions` keeps a session of
/// each: a new session ends only its own tool's least recently called
/// one, and only once that tool has the most.
#[test]
fn a_new_session_ends_only_its_own_tools_oldest() {
    let now = std::time::Instant::now();
    let at = |seconds: u64| now + Duration::from_secs(seconds);
    let tools = [
        "memory", "desktop", "browser", "cron", "file", "link", "question", "task", "tool",
    ];
    let kept: Vec<(&str, std::time::Instant)> =
        tools.iter().zip(0..).map(|(t, s)| (*t, at(s))).collect();
    assert_eq!(to_end(&kept, "web", 8), None, "a tenth tool ends none");
    let mut kept = kept;
    kept.push(("web", at(20)));
    kept.push(("web", at(10)));
    assert_eq!(to_end(&kept, "web", 3), None);
    assert_eq!(
        to_end(&kept, "web", 2),
        Some(10),
        "its least recently called"
    );
    assert_eq!(to_end(&kept, "memory", 1), Some(0));
}

#[test]
fn heads_name_a_tool_and_carry_a_session() {
    let head = parse(b"POST /_link/mcp/gh HTTP/1.1\r\nHost: x\r\nContent-Length: 12\r\nMcp-Session-Id: abc\r\n\r\n").unwrap();
    assert_eq!(
        (head.tool.as_str(), head.length, head.session.as_deref()),
        ("gh", 12, Some("abc"))
    );
    for bad in [
        &b"POST /_link/mcp/../x HTTP/1.1\r\n\r\n"[..],
        b"POST /_link/mcp/ HTTP/1.1\r\n\r\n",
        b"POST /_link/mcp/gh HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        b"POST /_link/mcp/gh HTTP/1.1\nX: y\r\n\r\n",
    ] {
        assert!(parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
    }
}
