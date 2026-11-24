//! A session's write or edit of a file changed since its read: refused,
//! never overwritten unseen, as the files bridge refuses one.

use super::*;

/// A file changed since the session read it is refused when written or
/// edited, never overwritten unseen, as the files bridge refuses one; a file
/// never read is written, and so is one read again.
#[test]
fn a_write_of_a_file_changed_since_its_read_is_refused() {
    let files = files("stale");
    std::fs::write(files.join("work/plan.md"), "one\n").unwrap();
    let apply = apply(&files, &["work"]);
    let messages = |calls: &[(&str, Value)]| -> String {
        calls
            .iter()
            .enumerate()
            .map(|(id, (name, arguments))| {
                format!(
                    "{}\n",
                    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                        "params": {"name": name, "arguments": arguments}})
                )
            })
            .collect()
    };
    let serve = |calls: &[(&str, Value)]| -> Vec<Value> {
        let mut replies = Vec::new();
        link_file_tool::serve(
            &apply,
            &link_file_tool::Offer::default(),
            messages(calls).as_bytes(),
            &mut replies,
        )
        .unwrap();
        String::from_utf8(replies)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
            .collect()
    };
    // Read, then changed by someone else, then written: refused. Its
    // version is the tool's own, never shown to the model.
    let plan = files.join("work/plan.md");
    let changer = format!("{}\n", "x");
    let replies = serve(&[
        ("read", json!({"file_path": "work/plan.md"})),
        // A write between: the session's own, so it's seen.
        (
            "write",
            json!({"file_path": "work/plan.md", "content": "two\n"}),
        ),
    ]);
    assert!(replies[0]["structuredContent"].get("version").is_none());
    assert_eq!(replies[1]["isError"], Value::Null, "{:?}", replies[1]);
    std::fs::write(&plan, "one\n").unwrap();
    let replies = {
        let mut replies = Vec::new();
        let calls = messages(&[("read", json!({"file_path": "work/plan.md"}))]);
        let later = messages(&[
            (
                "write",
                json!({"file_path": "work/plan.md", "content": "mine\n"}),
            ),
            // An edit of it too, though what it replaces is there.
            (
                "edit",
                json!({"file_path": "work/plan.md", "old_string": "x", "new_string": "y"}),
            ),
            ("read", json!({"file_path": "work/plan.md"})),
            (
                "write",
                json!({"file_path": "work/plan.md", "content": "mine\n"}),
            ),
        ]);
        // The file changed after the read, between the session's calls.
        let input = Changing {
            first: calls.into_bytes(),
            then: later.into_bytes(),
            change: Some((plan.clone(), Some(changer.clone()))),
        };
        link_file_tool::serve(
            &apply,
            &link_file_tool::Offer::default(),
            std::io::BufReader::new(input),
            &mut replies,
        )
        .unwrap();
        String::from_utf8(replies)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
            .collect::<Vec<_>>()
    };
    let why = refused(&replies[1]);
    assert!(why.contains("changed since it was read"), "{why}");
    let why = refused(&replies[2]);
    assert!(why.contains("changed since it was read"), "{why}");
    // Read again, it's written.
    assert_eq!(replies[4]["isError"], Value::Null, "{:?}", replies[4]);
    assert_eq!(std::fs::read_to_string(&plan).unwrap(), "mine\n");
}

/// A file removed after its read is refused once, as changed; read again
/// (there's nothing), it's written anew, never refused for the session.
#[test]
fn a_file_removed_since_its_read_is_written_anew() {
    let files = files("removed");
    let plan = files.join("work/plan.md");
    std::fs::write(&plan, "one\n").unwrap();
    let apply = apply(&files, &["work"]);
    let messages = |calls: &[(&str, Value)]| -> Vec<u8> {
        calls
            .iter()
            .enumerate()
            .map(|(id, (name, arguments))| {
                format!(
                    "{}\n",
                    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                        "params": {"name": name, "arguments": arguments}})
                )
            })
            .collect::<String>()
            .into_bytes()
    };
    let input = Changing {
        first: messages(&[("read", json!({"file_path": "work/plan.md"}))]),
        then: messages(&[
            (
                "write",
                json!({"file_path": "work/plan.md", "content": "two\n"}),
            ),
            ("read", json!({"file_path": "work/plan.md"})),
            (
                "write",
                json!({"file_path": "work/plan.md", "content": "two\n"}),
            ),
        ]),
        change: Some((plan.clone(), None)),
    };
    let mut replies = Vec::new();
    link_file_tool::serve(
        &apply,
        &link_file_tool::Offer::default(),
        std::io::BufReader::new(input),
        &mut replies,
    )
    .unwrap();
    let replies: Vec<Value> = String::from_utf8(replies)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
        .collect();
    assert!(refused(&replies[1]).contains("changed since it was read"));
    assert!(refused(&replies[2]).contains("no file"));
    assert_eq!(replies[3]["isError"], Value::Null, "{:?}", replies[3]);
    assert_eq!(std::fs::read_to_string(&plan).unwrap(), "two\n");
}

/// A file read by one spelling of its path and written by another that
/// reaches the same file, as `plan.md` does `Plan.md` where names ignore
/// case (here a second name, a hard link): changed between, the write is
/// refused, its version kept by the file, not only its path as spelled.
#[test]
fn a_file_reached_by_another_spelling_is_refused_when_changed() {
    let files = files("spelled");
    let plan = files.join("work/Plan.md");
    std::fs::write(&plan, "one\n").unwrap();
    std::fs::hard_link(&plan, files.join("work/plan.md")).unwrap();
    let apply = apply(&files, &["work"]);
    let messages = |calls: &[(&str, Value)]| -> Vec<u8> {
        calls
            .iter()
            .enumerate()
            .map(|(id, (name, arguments))| {
                format!(
                    "{}\n",
                    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                        "params": {"name": name, "arguments": arguments}})
                )
            })
            .collect::<String>()
            .into_bytes()
    };
    let input = Changing {
        first: messages(&[("read", json!({"file_path": "work/Plan.md"}))]),
        then: messages(&[(
            "write",
            json!({"file_path": "work/plan.md", "content": "mine\n"}),
        )]),
        change: Some((plan.clone(), Some("theirs\n".into()))),
    };
    let mut replies = Vec::new();
    link_file_tool::serve(
        &apply,
        &link_file_tool::Offer::default(),
        std::io::BufReader::new(input),
        &mut replies,
    )
    .unwrap();
    let replies: Vec<Value> = String::from_utf8(replies)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["result"].clone())
        .collect();
    assert!(replies[0]["structuredContent"].get("file").is_none());
    assert!(refused(&replies[1]).contains("changed since it was read"));
    assert_eq!(std::fs::read_to_string(&plan).unwrap(), "theirs\n");
}

/// Input that gives `first`, then changes a file (`change`: its path and
/// what it's written to hold, or nothing to remove it), then gives
/// `then`: a person editing a file between a session's calls.
struct Changing {
    first: Vec<u8>,
    then: Vec<u8>,
    change: Option<(std::path::PathBuf, Option<String>)>,
}

impl std::io::Read for Changing {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !self.first.is_empty() {
            let n = buf.len().min(self.first.len());
            buf[..n].copy_from_slice(&self.first[..n]);
            self.first.drain(..n);
            return Ok(n);
        }
        match self.change.take() {
            Some((path, Some(text))) => std::fs::write(path, text)?,
            Some((path, None)) => std::fs::remove_file(path)?,
            None => {}
        }
        let n = buf.len().min(self.then.len());
        buf[..n].copy_from_slice(&self.then[..n]);
        self.then.drain(..n);
        Ok(n)
    }
}
