//! Hermes's agent state, for the hub's ASP ([`link_harness_hub::Store`]):
//! its sessions through its own `SessionDB` (`sessions.py`, run by its
//! Python), who it is and what it remembers as [`crate::identity`] reads
//! and writes them, and the MCP servers its owner added to its
//! `config.yaml`. Its settings are Link's, which `configure` writes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use link_format::WithinExt;
use link_harness_contract::asp::acp::schema::v1::{McpServer, SessionId};
use link_harness_contract::asp::methods::{Imported, NotTaken};
use link_harness_contract::asp::{AgentState, Kind, State};
use link_harness_contract::harness::identity;
use serde_json::{Value, json};
use serde_norway::Value as Yaml;
mod sessions;

/// Where, in the user's home, Link archives the sessions it imported.
const IMPORTS: &str = ".hermes/link-imports.json";

/// The kinds it gives and takes: all but settings.
const KINDS: [Kind; 5] = [
    Kind::Sessions,
    Kind::Instructions,
    Kind::Skills,
    Kind::Tools,
    Kind::Memory,
];

/// Hermes in `home` (the user's), its sessions working in `cwd`.
pub struct Hermes {
    pub home: PathBuf,
    pub programs: PathBuf,
    pub cwd: String,
}

/// What it gives and takes, as its contract says and its hub
/// advertises.
pub fn kinds() -> AgentState {
    AgentState {
        export: KINDS.into_iter().chain([Kind::Schedules]).collect(),
        import: KINDS.to_vec(),
    }
}

pub fn content() -> Vec<link_harness_contract::harness::switching::SessionContent> {
    use link_harness_contract::harness::switching::SessionContent;
    vec![SessionContent::Tools, SessionContent::Images]
}

impl link_harness_hub::Store for Hermes {
    fn kinds(&self) -> AgentState {
        kinds()
    }

    fn export(&self, kinds: &[Kind]) -> anyhow::Result<State> {
        let mut state = crate::identity::export(&self.home)?;
        let asked = |kind| kinds.contains(&kind);
        if !asked(Kind::Instructions) {
            state.instructions = None;
        }
        if !asked(Kind::Skills) {
            state.skills = None;
        }
        if !asked(Kind::Memory) {
            state.memory = None;
        }
        if asked(Kind::Tools) {
            state.tools = Some(self.tools()?);
        }
        if asked(Kind::Sessions) {
            let sessions: Vec<sessions::Exported> =
                serde_json::from_slice(&self.sessions("export", b"")?)
                    .within("reading Hermes's sessions")?;
            let archive = self.archive()?;
            let mut portable = Vec::new();
            for native in sessions {
                if let Some(saved) = archive.get(&native.id) {
                    if let Some(restored) = saved.restore(&native) {
                        portable.push(restored);
                        continue;
                    }
                    // Native history diverged: keep both complete branches.
                    let mut old = saved.session.clone();
                    old.session_id = SessionId::new(format!("{}~imported", native.id));
                    portable.push(old);
                }
                portable.push(sessions::exported(&native));
            }
            state.sessions = Some(portable);
        }
        if asked(Kind::Schedules) {
            state.schedules = Some(crate::schedules::export(&self.home)?);
        }
        Ok(state)
    }

    fn import(&self, state: State) -> anyhow::Result<Imported> {
        let mut imported = Imported::default();
        if state.schedules.is_some() {
            imported.not_taken.push(NotTaken {
                kind: Kind::Schedules,
                reason: "Link owns portable schedules; native cron is never written on import"
                    .into(),
            });
        }
        let mut contract = link_harness_contract::harness::Contract::new(true);
        contract.session_content = content();
        let omitted = contract.session_omissions(&state);
        if !omitted.is_empty() {
            imported.not_taken.push(NotTaken {
                kind: Kind::Sessions,
                reason: format!(
                    "{} stay in the portable archive and are not used in native context",
                    omitted
                        .into_iter()
                        .map(|kind| kind.words())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
        if state.settings.as_ref().is_some_and(|s| !s.is_empty()) {
            imported.not_taken.push(NotTaken {
                kind: Kind::Settings,
                reason: "Hermes takes its settings from Link's".into(),
            });
        }
        if let Some(text) = &state.instructions {
            identity::write_outside(&self.home.join(".hermes/SOUL.md"), text)?;
            imported.taken.push(Kind::Instructions);
        }
        if let Some(skills) = &state.skills {
            let root = self.home.join(".hermes/skills");
            for skill in skills {
                identity::write_skill(&root, skill)?;
            }
            imported.taken.push(Kind::Skills);
        }
        if let Some(servers) = &state.tools {
            self.take_tools(servers)?;
            imported.taken.push(Kind::Tools);
        }
        if let Some(notes) = &state.memory {
            crate::identity::remember(&self.home, notes)?;
            imported.taken.push(Kind::Memory);
        }
        if let Some(sessions) = &state.sessions {
            let projected: Vec<_> = sessions.iter().map(sessions::projected).collect();
            let asked: Vec<Value> = sessions
                .iter()
                .zip(&projected)
                .map(|(session, messages)| {
                    json!({"id": session.session_id.0, "title": session.title,
                    "cwd": self.cwd, "messages": messages, "portable": session})
                })
                .collect();
            let made = self.import_sessions(&asked)?;
            let mut archive = self.archive()?;
            for (session, projected) in sessions.iter().zip(projected) {
                if let Some(id) = made.get(&*session.session_id.0) {
                    archive.insert(
                        id.clone(),
                        sessions::Archive {
                            session: session.clone(),
                            projected,
                        },
                    );
                }
            }
            link_files::write_private(
                &self.home.join(IMPORTS),
                &link_files::saved::print_json(&sessions::Imports { sessions: archive })?,
            )?;
            imported.sessions = made
                .into_iter()
                .map(|(old, new)| (SessionId::new(old), SessionId::new(new)))
                .collect();
            imported.taken.push(Kind::Sessions);
        }
        Ok(imported)
    }
}

impl Hermes {
    fn archive(&self) -> anyhow::Result<BTreeMap<String, sessions::Archive>> {
        let file = self.home.join(IMPORTS);
        match std::fs::read(file) {
            Ok(bytes) => Ok(link_files::saved::parse_json::<sessions::Imports>(&bytes)
                .within("reading Hermes's portable session archive")?
                .sessions),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// `sessions.py <command>` run by Hermes's own Python, with its
    /// packages, in its code, `input` on its stdin: what it printed.
    /// Hermes's launcher (`.hermes/bin/hermes`) runs a module of its code
    /// with them (`--run-module`), and its package manager's
    /// `pm._venv_entry` runs code given to it (`-c`) within that: there is
    /// no Python of Hermes's to run directly, its packages being in an
    /// environment its launcher picks as it starts.
    /// The sessions Hermes made of `asked`, each by the thread it was
    /// asked for.
    fn import_sessions(&self, asked: &[Value]) -> anyhow::Result<BTreeMap<String, String>> {
        let made = self.sessions("import", &serde_json::to_vec(asked)?)?;
        serde_json::from_slice(&made).within("reading the sessions Hermes made")
    }

    fn sessions(&self, command: &str, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        use std::io::Write;
        let hermes = self.home.join(".hermes");
        let payload = crate::workarounds::payload(&self.programs)?;
        let code = payload.code.clone();
        let mut command_line = payload.command(&hermes);
        let mut child = command_line
            .args(["--run-module", "pm._venv_entry"])
            .arg(&code)
            .arg("-c")
            .arg(include_str!("sessions.py"))
            .arg(command)
            .current_dir(&code)
            .env("HERMES_HOME", &hermes)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .within("running Hermes's Python")?;
        child
            .stdin
            .take()
            .within("opening the stdin of Hermes's Python")?
            .write_all(input)?;
        let out = child.wait_with_output()?;
        anyhow::ensure!(
            out.status.success(),
            "Hermes's sessions {command} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(out.stdout)
    }

    fn config(&self) -> anyhow::Result<Yaml> {
        let path = self.home.join(".hermes/config.yaml");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        Ok(match text.trim() {
            "" => Yaml::Mapping(Default::default()),
            _ => serde_norway::from_str(&text).within(format!("reading {}", path.display()))?,
        })
    }

    /// Link's own tools, which `configure` adds and isn't the owner's.
    fn links(&self) -> Vec<String> {
        link_harness_contract::harness::tools_added(&self.home.join(".hermes/link-tools.json"))
    }

    /// The MCP servers its owner added, as ACP describes them.
    fn tools(&self) -> anyhow::Result<Vec<McpServer>> {
        let config = self.config()?;
        let links = self.links();
        let Some(servers) = config["mcp_servers"].as_mapping() else {
            return Ok(Vec::new());
        };
        Ok(servers
            .iter()
            .filter_map(|(name, server)| {
                let name = name.as_str()?;
                if links.iter().any(|link| link == name) {
                    return None;
                }
                let server: Value = serde_json::to_value(server).ok()?;
                serde_json::from_value(described(name, &server)?).ok()
            })
            .collect())
    }

    /// `servers` added to its `config.yaml`, each under its name: one it
    /// has by that name stays as it is.
    fn take_tools(&self, servers: &[McpServer]) -> anyhow::Result<()> {
        let mut config = self.config()?;
        let root = config
            .as_mapping_mut()
            .within("reading Hermes's config.yaml as a mapping")?;
        let key = Yaml::from("mcp_servers");
        if !root.get(&key).is_some_and(Yaml::is_mapping) {
            root.insert(key.clone(), Yaml::Mapping(Default::default()));
        }
        let mcp = root
            .get_mut(&key)
            .and_then(Yaml::as_mapping_mut)
            .expect("made a mapping above");
        for server in servers {
            let wire = serde_json::to_value(server)?;
            let Some(name) = wire["name"].as_str() else {
                continue;
            };
            if mcp.contains_key(name) {
                continue;
            }
            mcp.insert(name.into(), serde_norway::to_value(hermes_form(&wire))?);
        }
        let path = self.home.join(".hermes/config.yaml");
        link_files::write_private(&path, serde_norway::to_string(&config)?.as_bytes())?;
        Ok(())
    }
}

/// Hermes's server `server` as ACP describes it: over HTTP when it has
/// a `url`, else a command.
fn described(name: &str, server: &Value) -> Option<Value> {
    let pairs = |map: &Value| -> Vec<Value> {
        map.as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some(json!({"name": k, "value": v.as_str()?})))
            .collect()
    };
    if let Some(url) = server["url"].as_str() {
        return Some(json!({"type": "http", "name": name, "url": url,
            "headers": pairs(&server["headers"])}));
    }
    Some(json!({"name": name, "command": server["command"].as_str()?,
        "args": server["args"].as_array().cloned().unwrap_or_default(),
        "env": pairs(&server["env"])}))
}

/// ACP's server `wire` in Hermes's form.
fn hermes_form(wire: &Value) -> Value {
    let map = |list: &Value| -> serde_json::Map<String, Value> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p["name"].as_str()?.to_string(), p["value"].clone())))
            .collect()
    };
    match wire["url"].as_str() {
        Some(url) => json!({"url": url, "headers": map(&wire["headers"])}),
        None => json!({"command": wire["command"], "args": wire["args"], "env": map(&wire["env"])}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_harness_contract::asp;
    use link_harness_contract::asp::forms::{By, Message, Session};
    use link_harness_hub::Store;

    /// Hermes's launcher, standing in: run as `sessions` runs it
    /// (`--run-module pm._venv_entry <code> -c <sessions.py> <command>`),
    /// `export` prints one session, and `import` keeps what it was given
    /// and makes one.
    const LAUNCHER: &str = "#!/bin/sh\n\
        [ \"$1 $2 $4\" = \"--run-module pm._venv_entry -c\" ] || exit 2\n\
        case \"$6\" in\n\
        export) echo '[{\"id\": \"20261020_a1\", \"title\": \"Plans\", \"messages\": \
        [{\"role\": \"user\", \"content\": \"hello\"}, {\"role\": \"assistant\", \"content\": \"hi\"}]}]' ;;\n\
        import) cat > \"$HERMES_HOME/asked.json\"; echo '{\"telegram:42\": \"20261020_b2\"}' ;;\n\
        esac\n";

    fn hermes(name: &str) -> (link_test_kit::TempDir, Hermes) {
        let home = link_test_kit::TempDir::new(name);
        let programs = link_files::layout::Hidden::HermesPrograms.in_home(&home);
        crate::workarounds::tests::complete_payload(&programs);
        let launcher =
            crate::installing::launcher_in(&programs.join(&crate::workarounds::CONFIG.code));
        std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        link_test_kit::write_program(&launcher, LAUNCHER).unwrap();
        std::fs::create_dir_all(home.join(".hermes")).unwrap();
        let hermes = Hermes {
            home: home.to_path_buf(),
            programs,
            cwd: "/files".into(),
        };
        (home, hermes)
    }

    /// `sessions.py import` run twice by Python against a stand-in of
    /// Hermes's SessionDB: what it wrote, and what each run printed.
    fn imported_twice(name: &str, asked: &Value) -> Option<(Value, Vec<Value>)> {
        if !link_test_kit::gate::program("python3", &["--version"]) {
            return None;
        }
        let home = link_test_kit::TempDir::new(name);
        let rows = home.join("rows.json");
        let asked_file = home.join("asked.json");
        std::fs::write(&asked_file, serde_json::to_vec(asked).unwrap()).unwrap();
        let script = [
            include_str!("state/sessions-test.py"),
            include_str!("sessions.py"),
        ]
        .concat();
        let printed = (0..2)
            .map(|_| {
                let mut command = link_test_kit::env::command("python3", &home);
                command
                    .args(["-I", "-c", &script, "import"])
                    .env("SESSIONS_TEST_ROWS", &rows)
                    .env("SESSIONS_TEST_ASKED", &asked_file);
                let output = link_test_kit::Deadline::new(
                    "Hermes's sessions import",
                    std::time::Duration::from_secs(30),
                )
                .output(&mut command);
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                serde_json::from_slice(&output.stdout).unwrap()
            })
            .collect();
        let rows = serde_json::from_slice(&std::fs::read(&rows).unwrap()).unwrap();
        Some((rows, printed))
    }

    #[test]
    fn an_imported_session_is_one_acps_resume_adopts_once() {
        let asked = json!([
            {"id": "link:main", "title": "Plans", "cwd": "/files", "portable": {"n": 1},
             "messages": [{"role": "user", "content": "hello"}]},
            {"id": "link:other", "title": "Plans", "cwd": "/files", "portable": {"n": 2},
             "messages": [{"role": "user", "content": "hi"}]}
        ]);
        let Some((rows, printed)) = imported_twice("hermes-sessions-import", &asked) else {
            return;
        };
        assert_eq!(printed[0], printed[1], "a second import is the first's");
        let rows = rows.as_object().unwrap();
        assert_eq!(rows.len(), 2, "{rows:?}");
        let main = printed[0]["link:main"].as_str().unwrap();
        let row = &rows[main];
        // Hermes adopts a plain `acp` row on resume, never one with provenance.
        assert_eq!(row["source"], "acp");
        assert!(row.get("origin_json").is_none(), "{row}");
        assert_eq!(row["cwd"], "/files");
        assert_eq!(row["messages"], asked[0]["messages"]);
        // An id of the shape `hermes acp` mints: a version 4 UUID.
        assert_eq!(main.len(), 36);
        assert_eq!(&main[14..15], "4", "{main}");
        let other = printed[0]["link:other"].as_str().unwrap();
        assert_eq!(rows[other]["title"], format!("Plans ({})", &other[24..]));
    }

    #[test]
    fn its_sessions_persona_and_owners_servers_are_exported() {
        let (home, hermes) = hermes("hermes-state-export");
        let soul = identity::with_block("Their own voice.\n", Some("From Link."));
        std::fs::write(home.join(".hermes/SOUL.md"), soul).unwrap();
        std::fs::write(
            home.join(".hermes/config.yaml"),
            "mcp_servers:\n  mine:\n    command: notes\n    args: [--fast]\n  \
             link-tools:\n    url: http://127.0.0.1:9/mcp\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".hermes/link-tools.json"),
            r#"{"tools":["link-tools"],"schema":1}"#,
        )
        .unwrap();
        let state = hermes.export(&KINDS).unwrap();
        let sessions = state.sessions.unwrap();
        assert_eq!(&*sessions[0].session_id.0, "20261020_a1");
        assert_eq!(sessions[0].title.as_deref(), Some("Plans"));
        assert_eq!(sessions[0].messages()[1].text, "hi");
        assert!(state.instructions.unwrap().contains("Their own voice."));
        let tools = serde_json::to_value(state.tools.unwrap()).unwrap();
        assert_eq!(
            tools,
            json!([{"name": "mine", "command": "notes", "args": ["--fast"], "env": []}])
        );
        let only = hermes.export(&[Kind::Memory]).unwrap();
        assert_eq!(only.kinds(), [Kind::Memory]);
    }

    #[test]
    fn what_comes_in_is_written_into_its_own_files() {
        let (home, hermes) = hermes("hermes-state-import");
        let soul = identity::with_block("Old voice.\n", Some("From Link."));
        std::fs::write(home.join(".hermes/SOUL.md"), soul).unwrap();
        let conversation = [
            Message {
                by: By::User,
                text: "hello".into(),
            },
            Message {
                by: By::Agent,
                text: "hi".into(),
            },
        ];
        let session = Session::of_messages(SessionId::new("telegram:42"), None, &conversation);
        let server: McpServer = serde_json::from_value(json!({
            "type": "http", "name": "web", "url": "https://mcp.example/", "headers": []
        }))
        .unwrap();
        let skill = asp::forms::Skill {
            name: "notes".into(),
            files: [("SKILL.md".to_string(), "# Notes\n".to_string())].into(),
        };
        let state = State {
            sessions: Some(vec![session]),
            instructions: Some("New voice.\n".into()),
            skills: Some(vec![skill.clone()]),
            tools: Some(vec![server]),
            settings: Some([("mode".to_string(), json!("ask"))].into()),
            ..State::default()
        };
        let imported = hermes.import(state).unwrap();
        assert_eq!(
            imported.sessions[&SessionId::new("telegram:42")],
            SessionId::new("20261020_b2")
        );
        assert_eq!(imported.not_taken[0].kind, Kind::Settings);
        let asked: Value =
            serde_json::from_slice(&std::fs::read(home.join(".hermes/asked.json")).unwrap())
                .unwrap();
        assert_eq!(
            asked[0]["messages"],
            json!([
                {"role": "user", "content": "hello"}, {"role": "assistant", "content": "hi"}
            ])
        );
        assert_eq!(asked[0]["portable"]["sessionId"], "telegram:42");
        let soul = std::fs::read_to_string(home.join(".hermes/SOUL.md")).unwrap();
        assert_eq!(
            identity::outside_block(&soul).as_deref(),
            Some("New voice.")
        );
        assert!(soul.contains("From Link."), "{soul}");
        let written = home.join(".hermes/skills/notes/SKILL.md");
        assert_eq!(std::fs::read_to_string(written).unwrap(), "# Notes\n");
        let config = std::fs::read_to_string(home.join(".hermes/config.yaml")).unwrap();
        assert!(config.contains("url: https://mcp.example/"), "{config}");

        let escaping = asp::forms::Skill {
            files: [("../SOUL.md".to_string(), "taken".to_string())].into(),
            ..skill
        };
        let state = State {
            skills: Some(vec![escaping]),
            ..State::default()
        };
        assert!(hermes.import(state).is_err());
    }
}
