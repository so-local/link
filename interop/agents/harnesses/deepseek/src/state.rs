//! DeepSeek Harness's agent state, for the hub's ASP ([`link_harness_hub::Store`]):
//! its retained session generations backfilled without changing native
//! logs, native format upgrades and runtime reconstruction remaining
//! DSH's own; and who it is, as [`crate::identity`] reads and writes it.

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use link_harness_contract::{
    asp::{
        AgentState, Kind, State,
        forms::Session,
        methods::{Imported, NotTaken},
    },
    harness::transcript,
};
use serde_json::{Value, json};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Layout {
    pub home: String,
    max_bytes: u64,
    current_version: u64,
    max_depth: usize,
    pub instructions: String,
    pub skills: String,
}
pub(crate) static LAYOUT: std::sync::LazyLock<Layout> = std::sync::LazyLock::new(|| {
    toml::from_str(include_str!("native.toml")).expect("checked native session layout")
});

pub struct DeepSeek {
    pub home: PathBuf,
}

/// The kinds it gives and takes of its own, besides its sessions.
const KINDS: [Kind; 2] = [Kind::Instructions, Kind::Skills];

/// What its hub gives and takes, as its contract says: its sessions read
/// from its logs and its identity both ways. Sessions come in through the
/// hub's portable continuation, never into its native logs.
pub fn kinds() -> AgentState {
    AgentState {
        export: [Kind::Sessions].into_iter().chain(KINDS).collect(),
        import: [Kind::Sessions].into_iter().chain(KINDS).collect(),
    }
}

impl link_harness_hub::Store for DeepSeek {
    fn kinds(&self) -> AgentState {
        AgentState {
            export: kinds().export,
            import: KINDS.to_vec(),
        }
    }
    fn export(&self, kinds: &[Kind]) -> anyhow::Result<State> {
        let mut state = crate::identity::export(&self.home)?;
        if !kinds.contains(&Kind::Instructions) {
            state.instructions = None;
        }
        if !kinds.contains(&Kind::Skills) {
            state.skills = None;
        }
        if !kinds.contains(&Kind::Sessions) {
            return Ok(state);
        }
        let mut sessions = Vec::new();
        let mut omissions = Vec::new();
        for path in logs(&self.home.join(&LAYOUT.home))? {
            let meta = std::fs::symlink_metadata(&path)?;
            ensure!(
                meta.is_file() && meta.len() <= LAYOUT.max_bytes,
                "native session log is not a supported regular file"
            );
            let bytes = std::fs::read(&path)?;
            let reader: Box<dyn Read + '_> = if path.extension().is_some_and(|s| s == "zstd") {
                Box::new(zstd::stream::read::Decoder::new(&bytes[..])?)
            } else {
                Box::new(&bytes[..])
            };
            let mut raw = Vec::new();
            reader.take(LAYOUT.max_bytes + 1).read_to_end(&mut raw)?;
            ensure!(
                raw.len() as u64 <= LAYOUT.max_bytes,
                "native history exceeds the supported decode limit; originals are retained"
            );
            ensure!(
                raw.ends_with(b"\n"),
                "native history has an incomplete record; originals are retained"
            );
            let records = std::str::from_utf8(&raw)?
                .lines()
                .map(serde_json::from_str::<Value>)
                .collect::<Result<Vec<_>, _>>()?;
            let header = records.first().context("native session has no header")?;
            ensure!(
                header["type"] == "session",
                "native session has an unsupported header"
            );
            let id = header["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .context("native session has no stable identity")?;
            let version = header["version"]
                .as_u64()
                .context("native session has no format version")?;
            let current = version == LAYOUT.current_version;
            let source = if current {
                id.to_string()
            } else {
                format!("archive:{id}:v{version}")
            };
            let mut session = Session {
                evidence: None,
                session_id: source.clone().into(),
                title: None,
                updates: Vec::new(),
                harness: None,
            };
            for (index, record) in records.iter().enumerate() {
                session.updates.push(transcript::original(
                    "deepseek",
                    &source,
                    &index.to_string(),
                    record.clone(),
                ));
                if current {
                    project(&mut session, record)?;
                }
            }
            if !current {
                omissions.push(NotTaken {kind:Kind::Sessions,reason:format!(
                "DeepSeek session {id} format {version} is retained as raw history; only format {} has a readable projection, and no native format is rewritten",LAYOUT.current_version)});
            }
            sessions.push(session);
        }
        if !sessions.is_empty() {
            omissions.push(NotTaken {kind:Kind::Sessions,reason:
            "DeepSeek native execution, provider streams and surface rewrites remain archival; portable continuation uses exposed text/tool history and never runs historical tools".into()});
        }
        state.sessions = Some(sessions);
        state.omissions = omissions;
        Ok(state)
    }
    fn import(&self, state: State) -> anyhow::Result<Imported> {
        let mut imported = Imported::default();
        crate::identity::import(&self.home, &state, &mut imported)?;
        Ok(imported)
    }
}

fn canonical(name: &str) -> bool {
    let name = name.strip_suffix(".zstd").unwrap_or(name);
    if name == "session.jsonl" {
        return true;
    }
    name.strip_prefix("session.v")
        .and_then(|s| s.strip_suffix(".jsonl"))
        .is_some_and(|v| !v.starts_with('0') && v.parse::<u64>().is_ok())
}

fn logs(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut stack = vec![(root.to_path_buf(), 0)];
    let mut logs = Vec::new();
    while let Some((folder, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name();
            if kind.is_dir() && depth < LAYOUT.max_depth {
                stack.push((entry.path(), depth + 1));
            } else if name.to_str().is_some_and(canonical) {
                ensure!(kind.is_file(), "native session log is not a regular file");
                logs.push(entry.path());
            }
        }
    }
    logs.sort();
    Ok(logs)
}

fn project(session: &mut Session, raw: &Value) -> anyhow::Result<()> {
    let kind = raw["type"].as_str().unwrap_or_default();
    let data = &raw["data"];
    let record = raw["seq"].to_string();
    let message = data.get("message").unwrap_or(data);
    if matches!(kind, "user/message" | "assistant/message") {
        let text = transcript::text_content(&message["content"]);
        if !text.is_empty() {
            session
                .updates
                .push(transcript::text(kind == "user/message", &text, &record));
        }
    }
    if kind == "tool/call" {
        let arguments = data["arguments"]
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or_else(|| data["arguments"].clone());
        session.updates.push(serde_json::from_value(
            json!({"sessionUpdate":"tool_call","toolCallId":data["callId"],
            "title":data["name"],"name":data["name"],"rawInput":arguments}),
        )?);
    }
    if kind == "tool/result" {
        let id = message
            .get("toolCallId")
            .or_else(|| message.get("callId"))
            .context("native tool result has no call identity")?;
        session.updates.push(serde_json::from_value(json!({"sessionUpdate":"tool_call_update","toolCallId":id,
            "status":if message["isError"]==true {"failed"} else {"completed"},"rawOutput":message["content"]}))?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_harness_hub::Store;

    #[test]
    fn native_backfill_keeps_generations_tools_and_unknown_extensions_without_rewriting_sources() {
        let home = link_test_kit::TempDir::new("deepseek-native-backfill");
        let folder = home.join(&LAYOUT.home).join("sessions/project/session");
        std::fs::create_dir_all(&folder).unwrap();
        let records = [
            json!({"type":"session","version":4,"id":"native-one","createdAt":1,"isSeeded":false,"delegationDepth":0}),
            json!({"type":"user/message","seq":0,"time":1,"data":{"content":"Correction: green; invoice still needs checking"},"surfaceOp":"append","extra":"retained"}),
            json!({"type":"tool/call","seq":1,"time":2,"data":{"callId":"c1","name":"file__read","arguments":"{}"}}),
            json!({"type":"tool/result","seq":2,"time":3,"data":{"message":{"toolCallId":"c1","content":"invoice pending"}},"surfaceOp":"append"}),
        ];
        let bytes = records
            .iter()
            .map(|r| format!("{r}\n"))
            .collect::<String>()
            .into_bytes();
        let compressed = zstd::stream::encode_all(&bytes[..], 0).unwrap();
        let path = folder.join("session.v4.jsonl.zstd");
        std::fs::write(&path, &compressed).unwrap();
        let old = folder.join("session.v3.jsonl");
        std::fs::write(
            &old,
            "{\"type\":\"session\",\"version\":3,\"id\":\"native-one\"}\n",
        )
        .unwrap();
        let state = DeepSeek {
            home: home.to_path_buf(),
        }
        .export(&[Kind::Sessions])
        .unwrap();
        let sessions = state.sessions.unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|s| {
            s.messages()
                .iter()
                .any(|m| m.text.contains("invoice still needs checking"))
        }));
        let raw = serde_json::to_string(&sessions).unwrap();
        assert!(
            raw.contains("retained")
                && raw.contains("invoice pending")
                && raw.contains("tool_call_update")
        );
        assert!(!state.omissions.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), compressed);
    }
}
