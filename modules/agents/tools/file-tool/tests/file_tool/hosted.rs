//! Link file on a team whose folder an account hosting agents keeps:
//! each call at `teams/<team>/` goes through the agent's files bridge on
//! its own machine, which hands it to the folder's service with the
//! agent's own token; the service's refusal, or the team's file here no
//! longer listing the agent, refuses the very next call.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use link_file_tool::{BridgedTeams, Offer, serve_teamed};
use link_harness_contract::harness::TeamFolders;
use link_sandbox_binding::Keyring;
use link_store_contract::files::{self, RemoteTeam, RemoteTeams};
use link_test_kit::TempDir;
use serde_json::{Value, json};

const OWNER: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";
const TEAM: &str = "t4m9q2x7k1ab";
const TOKEN: &str = "the agent's own token";

/// A call as the stand-in folder service read it.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    authorization: Option<String>,
    body: Value,
}

/// The team's folder service standing in: each call read whole before
/// it's answered, as the service answers: a read `missing`, a write its
/// version, and every call refused once `revoked` (the agent taken off
/// the team there).
struct Service {
    url: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    revoked: Arc<AtomicBool>,
}

fn service() -> Service {
    let listener = link_test_kit::net::Held::loopback().listener();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (seen, revoked) = (
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let (told, refusing) = (seen.clone(), revoked.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut read = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if read.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let (mut length, mut authorization) = (0, None);
            loop {
                let mut header = String::new();
                read.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                let (name, value) = header.split_once(": ").unwrap();
                match name.to_ascii_lowercase().as_str() {
                    "content-length" => length = value.parse().unwrap(),
                    "authorization" => authorization = Some(value.to_string()),
                    _ => {}
                }
            }
            let mut sent = vec![0; length];
            read.read_exact(&mut sent).unwrap();
            let body: Value = serde_json::from_slice(&sent).unwrap();
            let (status, answer) = match (refusing.load(Ordering::SeqCst), body["action"].as_str())
            {
                (true, _) => (
                    403,
                    r#"{"error":"you aren't on that team","why":"refused"}"#,
                ),
                (false, Some("read")) => (200, r#"{"missing":true}"#),
                (false, Some("write")) => (200, r#"{"version":"7"}"#),
                _ => (
                    400,
                    r#"{"error":"not a call this test makes","why":"refused"}"#,
                ),
            };
            told.lock().unwrap().push(Seen {
                line: line.trim_end().into(),
                authorization,
                body,
            });
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    Service { url, seen, revoked }
}

/// Link file's session as the agent's tool runs it: `messages`, each a
/// call, answered through `teams`, every other call run as `/bin/false`
/// (none here is).
fn session(teams: &BridgedTeams, calls: &[(&str, Value)]) -> Vec<Value> {
    let messages: String = calls
        .iter()
        .enumerate()
        .map(|(id, (name, arguments))| {
            let message = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": name, "arguments": arguments}});
            format!("{message}\n")
        })
        .collect();
    let mut replies = Vec::new();
    let apply = ["/bin/false".to_string()];
    serve_teamed(
        &apply,
        teams,
        &Offer::default(),
        messages.as_bytes(),
        &mut replies,
    )
    .unwrap();
    String::from_utf8(replies)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
        .collect()
}

fn refused(result: &Value) -> String {
    assert_eq!(result["isError"], true, "{result}");
    result["content"][0]["text"].as_str().unwrap().to_string()
}

#[test]
fn a_hosted_teams_folder_is_reached_through_the_agents_files_bridge() {
    let dir = TempDir::short("file-hosted");
    let binding = link_store_contract::testing::bound(&dir.join("keys"), "agent0000001");
    let service = service();
    // The agent's bridge, outside its sandbox: the team's folder at its
    // service, held first to what the agent is granted here.
    let remote = RemoteTeam::new(
        &service.url,
        OWNER,
        TEAM,
        Box::new(|| Ok(TOKEN.to_string().into())),
    )
    .unwrap()
    .with_grant(TeamFolders {
        write: vec!["notes".into()],
        read: vec!["sources".into()],
    });
    let on = Arc::new(AtomicBool::new(true));
    let listed = on.clone();
    let homes = Arc::new(RemoteTeams::new(
        binding.identity.clone(),
        BTreeMap::from([("research".to_string(), remote)]),
        Box::new(move |_name: &str| listed.load(Ordering::SeqCst)),
    ));
    let socket = dir.join("team-files.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let keys = Arc::new(Keyring::at(&dir.join("keys")));
    std::thread::spawn(move || files::serve(listener, keys, homes));
    let teams = BridgedTeams {
        bridge: files::Client { socket, binding },
        teams: vec!["research".into()],
    };

    // Written where the agent writes, with its own token, never the
    // sandbox's: the service took the call as the agent's.
    let replies = session(
        &teams,
        &[(
            "write",
            json!({"file_path": "teams/research/notes/a.md", "content": "one\n"}),
        )],
    );
    assert_eq!(
        replies[0]["structuredContent"]["created"], true,
        "{replies:?}"
    );
    let seen = service.seen.lock().unwrap().clone();
    let route = format!("POST /v1/teams/{OWNER}/{TEAM}/files HTTP/1.1");
    assert!(seen.iter().all(|s| s.line == route), "{seen:?}");
    let bearer = format!("Bearer {TOKEN}");
    assert!(
        seen.iter()
            .all(|s| s.authorization.as_deref() == Some(&bearer)),
        "{seen:?}"
    );
    let written = seen.last().unwrap();
    assert_eq!(written.body["action"], "write");
    assert_eq!(written.body["path"], "notes/a.md");
    assert!(written.body.get("team").is_none(), "{written:?}");

    // Where the agent only reads, its write refused here: read, as it
    // may, and never written there.
    let asked = service.seen.lock().unwrap().len();
    let replies = session(
        &teams,
        &[(
            "write",
            json!({"file_path": "teams/research/sources/b.md", "content": "x"}),
        )],
    );
    assert!(
        refused(&replies[0]).contains("you may write only in notes/"),
        "{replies:?}"
    );
    let sent: Vec<Value> = service.seen.lock().unwrap()[asked..]
        .iter()
        .map(|s| s.body["action"].clone())
        .collect();
    assert_eq!(sent, [json!("read")]);

    // Taken off the team there: its very next call is refused, saying so.
    service.revoked.store(true, Ordering::SeqCst);
    let replies = session(
        &teams,
        &[("read", json!({"file_path": "teams/research/notes/a.md"}))],
    );
    assert!(
        refused(&replies[0]).contains("you aren't on that team"),
        "{replies:?}"
    );

    // Taken off the team's file here: refused here, nothing sent.
    service.revoked.store(false, Ordering::SeqCst);
    on.store(false, Ordering::SeqCst);
    let asked = service.seen.lock().unwrap().len();
    let replies = session(
        &teams,
        &[("read", json!({"file_path": "teams/research/notes/a.md"}))],
    );
    assert!(refused(&replies[0]).contains("research"), "{replies:?}");
    assert_eq!(service.seen.lock().unwrap().len(), asked);
}

/// Only a call naming a team of its own kept elsewhere goes through the
/// bridge; the rest of the files folder, and every other team, is run
/// where it is.
#[test]
fn only_its_hosted_teams_calls_go_through_the_bridge() {
    let dir = TempDir::short("file-hosted-routes");
    let binding = link_store_contract::testing::bound(&dir.join("keys"), "agent0000001");
    let teams = BridgedTeams {
        bridge: files::Client {
            socket: dir.join("none.sock"),
            binding,
        },
        teams: vec!["research".into()],
    };
    for (arguments, bridged) in [
        (json!({"file_path": "teams/research/notes/a.md"}), true),
        (json!({"path": "teams/research/notes"}), true),
        (json!({"file_path": "teams/design/notes/a.md"}), false),
        (json!({"file_path": "work/teams/research/a.md"}), false),
        (json!({"pattern": "x"}), false),
    ] {
        assert_eq!(teams.reaches(&arguments), bridged, "{arguments}");
    }
}
