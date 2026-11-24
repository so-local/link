//! OpenClaw's `openclaw acp`, from 2026.9.6 through 2026.11.7, confuses a session's ACP id with
//! its gateway key, `acp-bridge:<id>`:
//!
//! - `session/resume` of a session an earlier `openclaw acp` made binds
//!   it to a new, empty gateway session, keyed by the ACP id.
//!   `session/load` reads the key from its ledger and finds it.
//! - `session/list` names each session by its gateway key,
//!   `agent:<agent>:acp-bridge:<id>`, not the id `session/new` gave it,
//!   so a client can't find its sessions there.
//!
//! Between the hub and `openclaw acp`, its `initialize` answer loses
//! `resume`, so the hub takes a session up by `session/load`, and a
//! `session/list` answer names each session by its id again. Reported
//! upstream as "session/resume after a restart binds a new, empty
//! gateway session"; this goes once the pinned release fixes both.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// What comes before an ACP session's id in its gateway key, after the
/// agent's part (`agent:main:`) where there is one.
const KEY_PREFIX: &str = "acp-bridge:";

/// Translate only OpenClaw's defined ACP key namespace. A group or account
/// containing similar text must retain its complete opaque identity.
pub(crate) fn native_id(key: &str) -> String {
    let candidate = key
        .strip_prefix("agent:")
        .and_then(|rest| rest.split_once(':'))
        .map_or(key, |(_, native)| native);
    candidate
        .strip_prefix(KEY_PREFIX)
        .unwrap_or(key)
        .to_string()
}

/// The pipes' buffer between the hub and the translation, in bytes.
const BUFFER: usize = 1 << 16;

/// `start`, its pipes translated.
pub(crate) fn translated(start: link_harness_hub::Start) -> link_harness_hub::Start {
    Box::new(move || {
        let harness = start()?;
        let (hub_side, to_harness) = tokio::io::duplex(BUFFER);
        let (from_hub_read, from_hub_write) = tokio::io::split(to_harness);
        let listing = Arc::new(Mutex::new(HashSet::new()));
        let asked = listing.clone();
        let mut write = harness.write;
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(from_hub_read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                noted(&line, &asked);
                if write
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                let _ = write.flush().await;
            }
        });
        let mut back = from_hub_write;
        let read = harness.read;
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = answered(&line, &listing);
                if back
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
                let _ = back.flush().await;
            }
        });
        let (read, write) = tokio::io::split(hub_side);
        Ok(link_harness_hub::Pipes {
            read: Box::new(read),
            write: Box::new(write),
            child: harness.child,
        })
    })
}

/// A line from the hub: a `session/list` request's id noted, so its
/// answer is translated.
fn noted(line: &str, listing: &Mutex<HashSet<String>>) {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if message["method"] == "session/list"
        && let Some(id) = message.get("id")
    {
        listing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string());
    }
}

/// A line from `openclaw acp`, as the hub reads it: an `initialize`
/// answer without `resume`, a `session/list` answer with each session's
/// id; any other line as it was.
fn answered(line: &str, listing: &Mutex<HashSet<String>>) -> String {
    let Ok(mut message) = serde_json::from_str::<Value>(line) else {
        return line.to_string();
    };
    let listed = message.get("id").is_some_and(|id| {
        listing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id.to_string())
    });
    let Some(result) = message.get_mut("result") else {
        return line.to_string();
    };
    let mut changed = false;
    if let Some(caps) = result
        .pointer_mut("/agentCapabilities/sessionCapabilities")
        .and_then(Value::as_object_mut)
    {
        changed |= caps.remove("resume").is_some();
    }
    if listed {
        for session in result["sessions"].as_array_mut().into_iter().flatten() {
            let Some(id) = session["sessionId"].as_str() else {
                continue;
            };
            let native = native_id(id);
            if native != id {
                session["sessionId"] = Value::from(native);
                changed = true;
            }
        }
    }
    match changed {
        true => message.to_string(),
        false => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `initialize`'s answer loses `resume`, so the hub loads instead;
    /// nothing else of it changes.
    #[test]
    fn resume_is_never_offered() {
        let listing = Mutex::default();
        let init = json!({"jsonrpc": "2.0", "id": 0, "result": {"agentCapabilities": {
            "loadSession": true,
            "sessionCapabilities": {"resume": {}, "list": {}}
        }}});
        let read: Value = serde_json::from_str(&answered(&init.to_string(), &listing)).unwrap();
        assert_eq!(
            read["result"]["agentCapabilities"],
            json!({"loadSession": true, "sessionCapabilities": {"list": {}}})
        );
    }

    /// A `session/list` answer names each session by its id; another
    /// answer naming one by a key is left as it was.
    #[test]
    fn a_list_names_each_session_by_its_id() {
        let listing = Mutex::default();
        noted(
            &json!({"jsonrpc": "2.0", "id": 7, "method": "session/list", "params": {}}).to_string(),
            &listing,
        );
        let answer = json!({"jsonrpc": "2.0", "id": 7, "result": {"sessions": [
            {"sessionId": "acp-bridge:4cb67d25", "cwd": "/a"},
            {"sessionId": "agent:main:acp-bridge:f2e4a2ed", "cwd": "/a"},
            {"sessionId": "agent:main:telegram", "cwd": "/a"}
        ]}});
        let read: Value = serde_json::from_str(&answered(&answer.to_string(), &listing)).unwrap();
        assert_eq!(read["result"]["sessions"][0]["sessionId"], "4cb67d25");
        assert_eq!(read["result"]["sessions"][1]["sessionId"], "f2e4a2ed");
        assert_eq!(
            read["result"]["sessions"][2]["sessionId"],
            "agent:main:telegram"
        );
        let other = json!({"jsonrpc": "2.0", "id": 8, "result": {"sessions": [
            {"sessionId": "acp-bridge:4cb67d25"}
        ]}})
        .to_string();
        assert_eq!(answered(&other, &listing), other);
    }

    /// A line that isn't JSON, or changes nothing, goes on byte for byte.
    #[test]
    fn other_lines_are_untouched() {
        let listing = Mutex::default();
        for line in [
            "not json",
            r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#,
        ] {
            assert_eq!(answered(line, &listing), line);
        }
    }
}
