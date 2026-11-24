//! Read-only backfill of native SQLite windows and retained JSONL archives.
//! No native routing identity is inferred from a display title or a file name.

use std::{io::Read, path::Path};

use anyhow::{Context, ensure};
use link_harness_contract::{
    asp::{Kind, State, forms::Session, methods::NotTaken},
    harness::transcript,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

#[derive(serde::Deserialize)]
struct Layout {
    agents: String,
    database: String,
    sessions: String,
    max_bytes: u64,
}
static LAYOUT: std::sync::LazyLock<Layout> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("native.toml")).expect("checked native layout")
});

pub fn export(home: &Path) -> anyhow::Result<State> {
    let mut sessions = Vec::new();
    let mut omissions = Vec::new();
    for agent in folders(&home.join(&LAYOUT.agents))? {
        let native = agent.join(&LAYOUT.sessions);
        let database = agent.join(&LAYOUT.database);
        if database.try_exists()? {
            let meta = std::fs::symlink_metadata(&database)?;
            ensure!(
                meta.is_file(),
                "native session database is not a regular file"
            );
            let db = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            db.execute_batch("BEGIN")?;
            let windows = db.prepare("SELECT w.session_id, w.session_key, n.current_session_id FROM session_windows w LEFT JOIN session_nodes n ON n.session_key = w.session_key ORDER BY w.created_at, w.session_id")?
                .query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?)))?
                .collect::<Result<Vec<_>,_>>()?;
            let compressed = db
                .prepare("PRAGMA table_info(transcript_events)")?
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "event_zstd");
            for (id, key, current) in windows {
                let raw = if compressed {
                    "SELECT event_json, event_zstd, event_utf8_bytes FROM transcript_events WHERE session_id = ?1 ORDER BY seq"
                } else {
                    "SELECT event_json, NULL, NULL FROM transcript_events WHERE session_id = ?1 ORDER BY seq"
                };
                let mut events = Vec::new();
                let mut query = db.prepare(raw)?;
                let mut rows = query.query([&id])?;
                while let Some(row) = rows.next()? {
                    let text: Option<String> = row.get(0)?;
                    let bytes = match text {
                        Some(text) => text.into_bytes(),
                        None => {
                            let bytes: Vec<u8> = row.get(1)?;
                            let decoded = decode(&bytes, true)?;
                            ensure!(
                                Some(decoded.len() as i64) == row.get::<_, Option<i64>>(2)?,
                                "native transcript length mismatch"
                            );
                            decoded
                        }
                    };
                    events.push(
                        serde_json::from_slice(&bytes)
                            .context("reading native transcript event")?,
                    );
                }
                // Cold and reset archives are read separately below. An empty
                // hot table never replaces their retained history.
                if !events.is_empty() {
                    let native_id = crate::workarounds::acp_session_ids::native_id(&key);
                    let name = if Some(&id) == current.as_ref() {
                        native_id
                    } else {
                        format!("archive:{key}:{id}")
                    };
                    sessions.push(session(&name, &events)?);
                }
            }
            // Retained archive blobs include reset/deletion histories. Cold
            // file references are checked through the native archive reader.
            for table in [
                "session_transcript_archives",
                "session_transcript_cold_archives",
            ] {
                let exists: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    [table],
                    |r| r.get(0),
                )?;
                if !exists {
                    continue;
                }
                let sql = if table == "session_transcript_archives" {
                    "SELECT session_id, archive_name, archive_blob, encoding, archive_sha256 FROM session_transcript_archives ORDER BY created_at"
                } else {
                    "SELECT session_id, archive_name, archive_blob, 'zstd', archive_sha256 FROM session_transcript_cold_archives ORDER BY archived_at"
                };
                let mut query = db.prepare(sql)?;
                let mut rows = query.query([])?;
                while let Some(row) = rows.next()? {
                    let id: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    let blob: Option<Vec<u8>> = row.get(2)?;
                    let encoding: String = row.get(3)?;
                    let digest: String = row.get(4)?;
                    let blob = match blob {
                        Some(blob) => blob,
                        None => {
                            ensure!(
                                Path::new(&name).components().count() == 1
                                    && !name.starts_with('.'),
                                "invalid cold archive name"
                            );
                            let file = native.join("cold").join(&name);
                            regular(&file)?;
                            std::fs::read(file)?
                        }
                    };
                    ensure!(
                        link_digest::sha256(&blob) == digest,
                        "native archive checksum mismatch"
                    );
                    ensure!(
                        matches!(encoding.as_str(), "identity" | "zstd"),
                        "unsupported native archive encoding"
                    );
                    let raw = decode(&blob, encoding == "zstd")?;
                    let events = lines(&raw)?;
                    sessions.push(session(&format!("archive:{id}:{name}"), &events)?);
                }
            }
            db.execute_batch("ROLLBACK")?;
        }
        // Old JSONL transcripts, including pre-compaction and reset copies,
        // remain archives even after SQLite has become the active store.
        for file in files(&native)? {
            let name = file
                .file_name()
                .and_then(|s| s.to_str())
                .context("invalid native archive name")?;
            if name.ends_with(".jsonl") || name.contains(".jsonl.") {
                regular(&file)?;
                let bytes = std::fs::read(&file)?;
                let compressed = bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]);
                let raw = decode(&bytes, compressed)?;
                let events = lines(&raw)?;
                let header = events.first().context("empty native transcript")?;
                let id = header["id"]
                    .as_str()
                    .or_else(|| header["sessionId"].as_str())
                    .context("native transcript has no source session identity")?;
                sessions.push(session(&format!("archive:{id}:{name}"), &events)?);
            }
        }
    }
    if !sessions.is_empty() {
        omissions.push(NotTaken {kind:Kind::Sessions,reason:
        "Native routing, active-window selection and runtime-only state remain in the OpenClaw home; raw records and readable text/tool history are archived separately and never replay tool effects".into()});
    }
    Ok(State {
        sessions: Some(sessions),
        omissions,
        ..Default::default()
    })
}

fn regular(path: &Path) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && meta.len() <= LAYOUT.max_bytes,
        "native archive is not a supported regular file"
    );
    Ok(())
}

fn entries(path: &Path, directory: bool) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "native archive path is a link");
        if kind.is_dir() == directory && (kind.is_dir() || kind.is_file()) {
            found.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}
fn folders(path: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    entries(path, true)
}
fn files(path: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    entries(path, false)
}

fn decode(bytes: &[u8], compressed: bool) -> anyhow::Result<Vec<u8>> {
    let reader: Box<dyn Read + '_> = if compressed {
        Box::new(zstd::stream::read::Decoder::new(bytes)?)
    } else {
        Box::new(bytes)
    };
    let mut raw = Vec::new();
    reader.take(LAYOUT.max_bytes + 1).read_to_end(&mut raw)?;
    ensure!(
        raw.len() as u64 <= LAYOUT.max_bytes,
        "native archive exceeds the supported decode limit"
    );
    Ok(raw)
}
fn lines(bytes: &[u8]) -> anyhow::Result<Vec<Value>> {
    ensure!(
        bytes.ends_with(b"\n"),
        "native transcript ends in an incomplete record; originals are retained"
    );
    std::str::from_utf8(bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

fn session(id: &str, events: &[Value]) -> anyhow::Result<Session> {
    let mut updates = Vec::new();
    for (i, raw) in events.iter().enumerate() {
        let original = raw.clone();
        let raw = if raw["kind"] == "event" {
            serde_json::from_str::<Value>(
                raw["row"]["event_json"]
                    .as_str()
                    .context("missing cold archive event")?,
            )?
        } else {
            raw.clone()
        };
        let record = raw["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| i.to_string());
        updates.push(transcript::original("openclaw", id, &record, original));
        let message = raw.get("message").unwrap_or(&raw);
        let role = message["role"].as_str().unwrap_or_default();
        let text = transcript::text_content(&message["content"]);
        if matches!(role, "user" | "assistant") && !text.is_empty() {
            updates.push(transcript::text(role == "user", &text, &record));
        }
        if role == "assistant" {
            for block in message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["type"] == "toolCall")
            {
                updates.push(serde_json::from_value(
                    json!({"sessionUpdate":"tool_call","toolCallId":block["id"],
                    "title":block["name"],"name":block["name"],"rawInput":block["arguments"]}),
                )?);
            }
        }
        if role == "toolResult" {
            updates.push(serde_json::from_value(json!({"sessionUpdate":"tool_call_update","toolCallId":message["toolCallId"],
                "status":if message["isError"]==true {"failed"} else {"completed"},"rawOutput":message["content"]}))?);
        }
    }
    Ok(Session {
        evidence: None,
        session_id: id.to_string().into(),
        title: None,
        updates,
        harness: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_backfill_preserves_compacted_windows_raw_extensions_and_tool_results() {
        let home = link_test_kit::TempDir::new("openclaw-backfill");
        let database = home
            .join(&LAYOUT.agents)
            .join("main")
            .join(&LAYOUT.database);
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        let db = Connection::open(&database).unwrap();
        db.execute_batch("CREATE TABLE session_nodes(session_key TEXT,current_session_id TEXT);
            CREATE TABLE session_windows(session_id TEXT,session_key TEXT,created_at INTEGER);
            CREATE TABLE transcript_events(session_id TEXT,seq INTEGER,event_json TEXT,event_zstd BLOB,event_utf8_bytes INTEGER);
            INSERT INTO session_nodes VALUES('agent:main:telegram:account:group:42:topic:1','current');
            INSERT INTO session_windows VALUES('old','agent:main:telegram:account:group:42:topic:1',1);
            INSERT INTO session_windows VALUES('current','agent:main:telegram:account:group:42:topic:1',2);
            INSERT INTO session_windows VALUES('inactive','agent:main:telegram:second:group:42:topic:1',3);").unwrap();
        let user=json!({"id":"r1","message":{"role":"user","content":"Correction: use green; check the invoice next"},"unknown":{"kept":true}}).to_string();
        db.execute(
            "INSERT INTO transcript_events VALUES('old',1,?1,NULL,NULL)",
            [&user],
        )
        .unwrap();
        let tool=json!({"id":"r2","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"invoice pending"}]}}).to_string();
        let compressed = zstd::stream::encode_all(tool.as_bytes(), 0).unwrap();
        db.execute(
            "INSERT INTO transcript_events VALUES('current',1,NULL,?1,?2)",
            rusqlite::params![compressed, tool.len() as i64],
        )
        .unwrap();
        db.execute(
            "INSERT INTO transcript_events VALUES('inactive',1,?1,NULL,NULL)",
            [&user],
        )
        .unwrap();
        let state = export(&home).unwrap();
        let sessions = state.sessions.unwrap();
        assert_eq!(sessions.len(), 3);
        assert!(sessions.iter().any(|s| {
            s.messages()
                .iter()
                .any(|m| m.text.contains("check the invoice next"))
        }));
        let raw = serde_json::to_string(&sessions).unwrap();
        assert!(
            raw.contains("unknown")
                && raw.contains("invoice pending")
                && raw.contains("tool_call_update")
        );
        assert_eq!(export(&home).unwrap().sessions.unwrap(), sessions);
        assert!(!state.omissions.is_empty());
        let envelope = json!({"kind":"event","unknownHeader":true,
            "row":{"event_json":user,"seq":7}});
        let cold = session("cold", std::slice::from_ref(&envelope)).unwrap();
        assert_eq!(transcript::source(&cold.updates[0]).unwrap().1, &envelope);
    }
}
