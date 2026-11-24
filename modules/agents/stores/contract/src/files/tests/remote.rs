//! A team's folder elsewhere: a call naming it is handed to its folder
//! service with the member's own token, and the service's word comes back.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};

const OWNER: &str = "acct-0b7c5e2d4f6a8b9c0d1e2f3a4b5c6d7e";
const TEAM: &str = "t4m9q2x7k1ab";

/// A call as the stand-in service read it: its request line, its
/// `Authorization` header and its body.
#[derive(Debug)]
struct Seen {
    line: String,
    authorization: Option<String>,
    body: serde_json::Value,
}

/// A folder service standing in for a hosted team's: for each call, read
/// whole before it answers, it answers the next of `answers` (a status
/// and a JSON body) and tells what it read.
fn service(answers: Vec<(u16, &'static str)>) -> (String, std::sync::mpsc::Receiver<Seen>) {
    let listener = link_test_kit::net::Held::loopback().listener();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tell, seen) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (status, body) in answers {
            let (stream, _) = listener.accept().unwrap();
            let mut read = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            read.read_line(&mut line).unwrap();
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
            let _ = tell.send(Seen {
                line: line.trim_end().into(),
                authorization,
                body: serde_json::from_slice(&sent).unwrap(),
            });
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (url, seen)
}

/// Agents whose every team is answered by the folder service at `url`,
/// counting what was asked of the folder here (never).
struct Elsewhere {
    url: String,
    asked_here: AtomicUsize,
}

impl Homes for Elsewhere {
    fn home(&self, _who: &Identity) -> anyhow::Result<Home> {
        anyhow::bail!("no Home here")
    }
    fn team(&self, _: &Identity, _: &TeamRef) -> anyhow::Result<Option<TeamFolder>> {
        self.asked_here.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
    fn remote_team(
        &self,
        _who: &Identity,
        _team: &TeamRef,
    ) -> anyhow::Result<Option<Arc<RemoteTeam>>> {
        let token = || Ok(link_secretive::Secret::from("member-token".to_string()));
        Ok(Some(Arc::new(RemoteTeam::new(
            &self.url,
            OWNER,
            TEAM,
            Box::new(token),
        )?)))
    }
}

/// The member's call reaches the service on the team's route, with its
/// own token and no team of the caller's naming; the service's answer,
/// its refusal and its conflict come back to the member as the bridge's
/// own would; nothing of the folder is reached here.
#[test]
fn a_team_elsewhere_is_answered_by_its_service_with_the_members_token() {
    let dir = TempDir::short("files-remote");
    let ours = crate::testing::bound(&dir.join("keys"), "ours00000001");
    let (url, seen) = service(vec![
        (200, r#"{"version":"7"}"#),
        (
            403,
            r#"{"error":"you may write only in notes/: drafts/a.md isn't","why":"refused"}"#,
        ),
        (409, r#"{"error":"notes/a.md changed","why":"conflict"}"#),
        (200, r#"{"files":[]}"#),
    ]);
    let homes = Arc::new(Elsewhere {
        url,
        asked_here: AtomicUsize::new(0),
    });
    let (socket, _) = served(&dir, homes.clone());
    let client = Client {
        socket,
        binding: ours,
    };
    let research = TeamRef::parse("research").unwrap();

    let written = client.write_in(Some(&research), "notes/a.md", b"one", Expect::Absent);
    assert_eq!(written.unwrap(), "7");
    let call = seen.recv().unwrap();
    assert_eq!(
        call.line,
        format!("POST /v1/teams/{OWNER}/{TEAM}/files HTTP/1.1")
    );
    assert_eq!(call.authorization.as_deref(), Some("Bearer member-token"));
    assert_eq!(call.body["action"], "write");
    assert_eq!(call.body["path"], "notes/a.md");
    assert!(call.body.get("team").is_none(), "{:?}", call.body);

    let refused = client
        .write_in(Some(&research), "drafts/a.md", b"x", Expect::Absent)
        .unwrap_err();
    assert!(
        format!("{refused:#}").contains("you may write only in notes/"),
        "{refused:#}"
    );
    seen.recv().unwrap();

    let stale = client.write_in(Some(&research), "notes/a.md", b"two", Expect::Absent);
    assert!(stale.is_err_and(|e| is_conflict(&e)));
    seen.recv().unwrap();

    assert!(client.list_in(Some(&research), "").unwrap().is_empty());
    assert_eq!(seen.recv().unwrap().body["action"], "list");
    assert_eq!(homes.asked_here.load(Ordering::SeqCst), 0);
}

/// The route and grant a member's token is made for are the team's own.
#[test]
fn a_members_token_is_for_its_teams_route() {
    assert_eq!(
        team_route(OWNER, TEAM),
        format!("/v1/teams/{OWNER}/{TEAM}/files")
    );
    assert_eq!(member_grants(TEAM), "team.member@team:t4m9q2x7k1ab");
    let token = || Ok(link_secretive::Secret::from(String::new()));
    assert!(RemoteTeam::new("ftp://x", OWNER, TEAM, Box::new(token)).is_err());
    assert!(RemoteTeam::new("https://x", "me@box", TEAM, Box::new(token)).is_err());
}
