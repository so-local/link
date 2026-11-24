//! Real HTTP pages exceed the generic client cap, but stay catalogue-bounded.
use super::*;
use link_test_kit::{TempDir, env, gate, net::Held, process::ChildGroup};

fn server(pages: usize, complete: bool) -> (TempDir, ChildGroup, String) {
    let mut tokens = vec!["next".to_string(); pages];
    if complete && let Some(last) = tokens.last_mut() {
        last.clear();
    }
    server_tokens(tokens, 2 << 20)
}

fn server_tokens(tokens: Vec<String>, padding: usize) -> (TempDir, ChildGroup, String) {
    let dir = TempDir::short("gcp-catalog");
    let held = Held::loopback();
    let url = format!("http://{}/skus", held.address());
    let script = dir.join("catalog.py");
    link_test_kit::write_program(&script, r#"#!/usr/bin/env python3
import json, os, socket, sys, urllib.parse
listener = socket.socket(fileno=int(os.environ['LISTEN']))
tokens = json.loads(sys.argv[1])
for number, token in enumerate(tokens):
    connection, _ = listener.accept()
    with connection:
        request = connection.makefile('rb')
        line = request.readline()
        if number:
            assert ('pageToken=' + urllib.parse.quote(tokens[number-1])).encode() in line, line
        while request.readline() != b'\r\n':
            pass
        body = json.dumps({'skus': [{'page': number}], 'nextPageToken': token,
                           'padding': 'x' * int(sys.argv[2])}).encode()
        try:
            connection.sendall(b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: '
                               + str(len(body)).encode() + b'\r\nConnection: close\r\n\r\n' + body)
        except (BrokenPipeError, ConnectionResetError):
            pass
"#).unwrap();
    let mut command = env::command("python3", &dir);
    command
        .arg(&script)
        .arg(serde_json::to_string(&tokens).unwrap())
        .arg(padding.to_string());
    held.hand_to(&mut command, "LISTEN");
    let child = ChildGroup::spawn(&mut command).unwrap();
    (dir, child, url)
}

#[test]
fn large_catalog_pages_are_bounded_and_only_complete_catalogs_succeed() {
    if !gate::program("python3", &["--version"]) {
        return;
    }
    let client = Client::builder().outside().build().unwrap();
    let token = Secret::new("catalog-fixture".into());
    for (pages, complete, bytes, succeeds) in [
        (2, true, settings().page_bytes, true),
        (1, false, settings().page_bytes, false),
        (1, true, link_fetch::MAX_ANSWER, false),
    ] {
        let (_dir, _child, api) = server(pages, complete);
        let selected = Settings {
            api,
            pages,
            page_size: 5000,
            page_bytes: bytes,
            request_seconds: 10,
        };
        let result = read_with(&selected, &client, &token);
        assert_eq!(result.is_ok(), succeeds, "{result:?}");
        if succeeds {
            assert_eq!(result.unwrap().len(), pages);
        }
    }
}

#[test]
fn a_catalog_cannot_revisit_an_earlier_page_before_terminating() {
    if !gate::program("python3", &["--version"]) {
        return;
    }
    let (_dir, _child, api) = server_tokens(["A", "B", "A", ""].map(String::from).into(), 0);
    let selected = Settings {
        api,
        pages: 4,
        page_size: 5000,
        page_bytes: settings().page_bytes,
        request_seconds: 10,
    };
    let client = Client::builder().outside().build().unwrap();
    let error = read_with(&selected, &client, &Secret::new("catalog-fixture".into())).unwrap_err();
    assert!(
        error.to_string().contains("repeated its page token"),
        "{error:#}"
    );
}
