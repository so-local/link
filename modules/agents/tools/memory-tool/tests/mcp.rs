//! `lnk-memory-tool serve` as an MCP client drives it over stdio: a
//! message a line, notifications unanswered, a tool's error a result
//! the model reads.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn it_answers_mcp_over_stdio() {
    let root = link_test_kit::TempDir::new("mcp");
    std::fs::create_dir_all(root.join("chats/t1")).unwrap();
    std::fs::create_dir_all(root.join("memory")).unwrap();
    std::fs::write(
        root.join("memory/recipe.md"),
        "# Soup\n\nLeeks and potatoes.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("chats/t1/conversation.md"),
        "**You:** What soup was it?\n\n**Agent:** Leek and potato.\n",
    )
    .unwrap();

    let mut server = Command::new(env!("CARGO_BIN_EXE_lnk-memory-tool"))
        .arg("serve")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = server.stdin.take().unwrap();
    let mut output = BufReader::new(server.stdout.take().unwrap());
    let mut ask = |input: &mut std::process::ChildStdin, message: Value| -> Value {
        writeln!(input, "{message}").unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let call = |id: u64, name: &str, arguments: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
               "params": {"name": name, "arguments": arguments}})
    };

    let init = ask(
        &mut input,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {},
                   "clientInfo": {"name": "t", "version": "1"}}}),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "link-memory");

    // A notification gets no answer: the next line is tools/list's.
    writeln!(
        input,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    let tools = ask(
        &mut input,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    assert_eq!(tools["id"], 2, "{tools}");
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["links", "list", "read", "search"]);
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["annotations"]["readOnlyHint"] == true)
    );

    // Read in the background: answered at once, saying what isn't
    // indexed yet, until all of it is.
    let found =
        link_test_kit::wait::eventually("the folder indexed", Duration::from_secs(10), || {
            let found = ask(
                &mut input,
                call(3, "search", json!({"query": "leek potato"})),
            );
            found["result"]["structuredContent"]["indexing"]
                .is_null()
                .then_some(found)
        });
    let results = &found["result"]["structuredContent"]["results"];
    assert_eq!(results.as_array().unwrap().len(), 2, "{found}");
    let kinds: Vec<&str> = results
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"file") && kinds.contains(&"conversation"));
    // The same, as text, for a client that reads only that, fenced as
    // third-party text: a search's, from its query.
    let text = found["result"]["content"][0]["text"].as_str().unwrap();
    let fenced = link_untrusted::fenced(text).expect("the results fenced");
    assert_eq!(fenced.kind, "search", "{text}");
    let shown = serde_json::from_str::<Value>(fenced.text).unwrap();
    let structured = &found["result"]["structuredContent"];
    assert_eq!(
        shown["results"][0]["path"],
        structured["results"][0]["path"]
    );
    // What the documents say is the fenced text's alone: the structured
    // content gives where, never what.
    assert!(shown["results"][0]["snippet"].is_string(), "{shown}");
    for said in ["title", "snippet"] {
        assert!(structured["results"][0].get(said).is_none(), "{structured}");
    }

    let read = ask(
        &mut input,
        call(4, "read", json!({"path": "memory/recipe.md", "from": 3})),
    );
    assert_eq!(crate::document(&read)["text"], "Leeks and potatoes.");
    assert!(read["result"]["structuredContent"].get("text").is_none());

    // Written since it started: the next call a second after the last
    // sees it.
    std::fs::write(root.join("memory/later.md"), "Bread on Sunday.\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let listed = ask(&mut input, call(5, "list", json!({"kind": "file"})));
    assert_eq!(
        listed["result"]["structuredContent"]["total"], 2,
        "{listed}"
    );

    // A folder and a time, as a date or as seconds: what's in it and
    // changed after, and with a time, what was removed.
    let under = ask(
        &mut input,
        call(
            9,
            "search",
            json!({"query": "leek potato", "under": "chats"}),
        ),
    );
    let results = under["result"]["structuredContent"]["results"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(results.len(), 1, "{under}");
    assert_eq!(results[0]["kind"], "conversation");
    let future = ask(&mut input, call(10, "list", json!({"since": "2999-01-01"})));
    let listing = &future["result"]["structuredContent"];
    assert_eq!(listing["total"], 0, "{future}");
    assert_eq!(listing["removed"], json!([]));
    let everything = ask(&mut input, call(11, "list", json!({"since": "1970-01-01"})));
    let listing = &everything["result"]["structuredContent"];
    assert_eq!(listing["total"], 3, "{everything}");
    assert!(
        listing["documents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["change"] == "new")
    );
    let bad_time = ask(
        &mut input,
        call(12, "search", json!({"query": "soup", "since": "last week"})),
    );
    assert_eq!(bad_time["result"]["isError"], true, "{bad_time}");

    // Page through a search: one result a call, each call from the
    // last's `next`, every result once.
    let (mut next, mut paged) = (None::<String>, Vec::new());
    for id in 20..30 {
        let page = ask(
            &mut input,
            call(
                id,
                "search",
                json!({"query": "leek potato soup", "limit": 1, "page": next}),
            ),
        );
        let reply = &page["result"]["structuredContent"];
        paged.push(reply["results"][0]["path"].as_str().unwrap().to_string());
        match reply["next"].as_str() {
            Some(page) => next = Some(page.to_string()),
            None => break,
        }
    }
    paged.sort();
    assert_eq!(paged, ["chats/t1/conversation.md", "memory/recipe.md"]);
    // A size in tokens: whole results while they fit, at least one.
    let small = ask(
        &mut input,
        call(13, "search", json!({"query": "leek potato", "tokens": 1})),
    );
    let reply = &small["result"]["structuredContent"];
    assert_eq!(reply["results"].as_array().unwrap().len(), 1, "{small}");
    assert!(reply["next"].is_string(), "{small}");
    let read = ask(
        &mut input,
        call(14, "read", json!({"path": "memory/recipe.md", "tokens": 3})),
    );
    let reply = &read["result"]["structuredContent"];
    assert_eq!(crate::document(&read)["text"], "# Soup", "{read}");
    assert_eq!(reply["next"], 2);

    let huge = ask(
        &mut input,
        call(
            15,
            "read",
            json!({"path": "memory/recipe.md", "from": 3, "lines": u64::MAX}),
        ),
    );
    assert_eq!(
        crate::document(&huge)["text"],
        "Leeks and potatoes.",
        "{huge}"
    );

    let missing = ask(
        &mut input,
        call(6, "read", json!({"path": "../etc/passwd"})),
    );
    assert_eq!(missing["result"]["isError"], true, "{missing}");
    let bad = ask(
        &mut input,
        call(7, "search", json!({"query": "x", "kind": "email"})),
    );
    assert_eq!(bad["result"]["isError"], true, "{bad}");
    let unknown = ask(&mut input, call(8, "delete", json!({})));
    assert_eq!(unknown["error"]["code"], -32602);
    assert_eq!(
        ask(&mut input, json!("not a message"))["error"]["code"],
        -32600
    );

    drop(input);
    assert!(server.wait().unwrap().success());
}

/// Long titles and many documents or links: `list` and `links` stay
/// within the size a call asks for, or about 6,000 tokens, and say
/// `cut`; a title is at most 200 characters.
#[test]
fn every_answer_fits_its_size() {
    use link_memory_tool::{Index, mcp};
    let root = link_test_kit::TempDir::new("mcp-sizes");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    let heading = "h".repeat(100_000);
    for n in 0..300 {
        std::fs::write(
            root.join(format!("memory/note{n}.md")),
            format!("# {heading}\n"),
        )
        .unwrap();
    }
    let links: String = (0..900)
        .map(|n| format!("[[{n}-{}]]\n", "x".repeat(400)))
        .collect();
    std::fs::write(root.join("memory/links.md"), links).unwrap();

    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let call = |name: &str, arguments: Value| -> (usize, Value) {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": arguments}});
        let reply = mcp::answer(&index, &message.to_string()).unwrap();
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .len();
        let shown =
            link_untrusted::unfenced(reply["result"]["content"][0]["text"].as_str().unwrap());
        (text, serde_json::from_str::<Value>(&shown).unwrap())
    };
    let (size, listed) = call("list", json!({"limit": 500}));
    assert!(size < 6000 * 4 + 1000, "{size} bytes");
    assert_eq!(listed["cut"], true);
    assert_eq!(listed["total"], 301);
    let title = listed["documents"][0]["title"].as_str().unwrap();
    assert!(title.chars().count() <= 200, "{}", title.len());
    let (size, links) = call("links", json!({"path": "memory/links.md"}));
    assert!(size < 6000 * 4 + 1000, "{size} bytes");
    assert_eq!(links["cut"], true);
    let (size, _) = call("list", json!({"limit": 500, "tokens": 500}));
    assert!(size < 500 * 4 + 1000, "{size} bytes");
}

/// `links` paged by `next` shows every link once: a list cut stops the
/// answer there, so a smaller link after it never runs `next` past one
/// not yet shown.
#[test]
fn links_paged_show_every_link() {
    use link_memory_tool::{Index, mcp};
    let root = link_test_kit::TempDir::new("mcp-links-paged");
    std::fs::create_dir_all(root.join("memory")).unwrap();
    let write =
        |name: &str, text: &str| std::fs::write(root.join("memory").join(name), text).unwrap();
    write("a.md", "# a\n[[b]]\n[[c]]\n");
    write("b.md", "# b\n");
    write("c.md", &format!("# {}\n", "c".repeat(200)));
    write("d.md", "# d\n[[a]]\n");
    let mut index = Index::open(&root, None).unwrap();
    index.refresh().unwrap();
    let mut seen = Vec::new();
    let mut skip = 0;
    for _ in 0..5 {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "links", "arguments":
                {"path": "memory/a.md", "tokens": 30, "skip": skip}}});
        let reply = mcp::answer(&index, &message.to_string()).unwrap();
        let links = &reply["result"]["structuredContent"];
        for list in ["to", "from"] {
            for linked in links[list].as_array().unwrap() {
                seen.push(format!("{list} {}", linked["path"].as_str().unwrap()));
            }
        }
        match links["next"].as_u64() {
            Some(next) => skip = next as usize,
            None => break,
        }
    }
    assert_eq!(
        seen,
        ["to memory/b.md", "to memory/c.md", "from memory/d.md"]
    );
    // No call asks for more than an answer holds.
    let message = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "links", "arguments": {"path": "memory/a.md", "tokens": 1_000_000}}});
    let reply = mcp::answer(&index, &message.to_string()).unwrap();
    assert_eq!(reply["result"]["isError"], true, "{reply}");
}

/// What the agent's spec says of its memory (`LNK_MEMORY_SPEC`) is how
/// it's served: its embedding model, and the most a search gives.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn the_agents_spec_says_how_it_is_served() {
    let command = |spec: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lnk-memory-tool"));
        command.args(["launch", "show", "/f", "/s"]);
        match spec {
            Some(spec) => command.env(link_memory_tool_contract::SPEC_ENV, spec),
            None => command.env_remove(link_memory_tool_contract::SPEC_ENV),
        };
        command.output().unwrap()
    };
    let served = |spec: Option<&str>| {
        let output = command(spec);
        assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let nomic = served(Some(r#"{"model": "nomic-embed-text", "tokens": 900}"#));
    assert_eq!(nomic["model"], "nomic-embed-text");
    let argv = nomic["argv"].to_string();
    assert!(argv.contains(r#""--embed","nomic-embed-text""#), "{argv}");
    assert!(argv.contains(r#""--tokens","900""#), "{argv}");
    // Without it, the engine's own.
    let own = served(None);
    assert_eq!(own["model"], "qwen3-embedding");
    assert!(!own["argv"].to_string().contains("--tokens"));
    // `none`: by words only, no model served, nothing reached.
    let words = served(Some(r#"{"model": "none"}"#));
    assert_eq!(words["model"], Value::Null);
    assert_eq!(words["models"], Value::Null);
    assert!(!words["argv"].to_string().contains("--embed"));
    // Nonsense is refused, never taken as the engine's own.
    for spec in [
        r#"{"model": "a b; rm"}"#,
        "not json",
        r#"{"tokens": 0}"#,
        r#"{"tokens": 6001}"#,
    ] {
        let output = command(Some(spec));
        assert!(!output.status.success(), "{spec}");
        assert!(output.stdout.is_empty(), "{spec}");
    }
}
