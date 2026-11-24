//! OpenClaw's agent state, for the hub's ASP ([`link_harness_hub::Store`]):
//! who it is and what it remembers, as [`crate::identity`] reads and
//! writes them in its workspace, and the MCP servers its owner added. Its sessions the hub replays through
//! `openclaw acp`'s own `session/list` and `session/load`; none come in,
//! as OpenClaw keeps them behind its gateway, which takes no conversation
//! from outside its own runs. Its settings are Link's.

use std::path::PathBuf;

use link_harness_contract::asp::acp::schema::v1::McpServer;
use link_harness_contract::asp::methods::{Imported, NotTaken};
use link_harness_contract::asp::{AgentState, Kind, State};
use link_harness_contract::harness::identity::{self, Identity};
use serde_json::{Value, json};

/// The kinds it gives and takes.
const KINDS: [Kind; 3] = [Kind::Instructions, Kind::Skills, Kind::Memory];

/// The kinds it gives besides: the MCP servers its owner added, which
/// nothing gives it, as Link's tools reach it from its spec.
const GIVEN: [Kind; 2] = [Kind::Tools, Kind::Schedules];

/// OpenClaw in `home`, the user's.
pub struct OpenClaw {
    pub home: PathBuf,
}

/// What its hub gives and takes, as its contract says: its store's
/// kinds, and its sessions given too, replayed through `openclaw acp`'s
/// own list and load.
pub fn kinds() -> AgentState {
    AgentState {
        export: [Kind::Sessions]
            .into_iter()
            .chain(KINDS)
            .chain(GIVEN)
            .collect(),
        import: [Kind::Sessions].into_iter().chain(KINDS).collect(),
    }
}

impl OpenClaw {
    /// The MCP servers its owner added to `openclaw.json`, as ACP
    /// describes them: those Link added, in `link-tools.json`, left out.
    fn tools(&self) -> anyhow::Result<Vec<McpServer>> {
        let prefix = self.home.join(".openclaw");
        let links = link_harness_contract::harness::tools_added(
            &prefix.join(crate::configuring::TOOLS_RECORD),
        );
        let text = match std::fs::read_to_string(prefix.join("openclaw.json")) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        // One edited by hand into JSON5 is read as none: configure refuses it.
        let config: Value = serde_json::from_str(&text).unwrap_or_default();
        let Some(servers) = config["mcp"]["servers"].as_object() else {
            return Ok(Vec::new());
        };
        Ok(servers
            .iter()
            .filter(|(name, _)| !links.contains(name))
            .filter_map(|(name, server)| serde_json::from_value(described(name, server)?).ok())
            .collect())
    }
}

/// OpenClaw's server `server` as ACP describes it: over HTTP when it has
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

impl link_harness_hub::Store for OpenClaw {
    fn kinds(&self) -> AgentState {
        AgentState {
            export: [Kind::Sessions]
                .into_iter()
                .chain(KINDS)
                .chain(GIVEN)
                .collect(),
            import: KINDS.to_vec(),
        }
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
        if asked(Kind::Schedules) {
            state.schedules = Some(crate::schedules::export(&self.home)?);
        }
        if asked(Kind::Sessions) {
            let native = crate::native::export(&self.home)?;
            state.sessions = native.sessions;
            state.omissions.extend(native.omissions);
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
        let workspace = self.home.join(".openclaw/workspace");
        if let Some(text) = &state.instructions {
            // The persona and owner in `SOUL.md`, the rest in `AGENTS.md`,
            // as `configure` writes the spec's.
            let who = Identity::of_agents_md(text);
            if let Some(soul) = who.who() {
                identity::write_outside(&workspace.join("SOUL.md"), &soul)?;
            }
            if let Some(standing) = &who.standing {
                identity::write_outside(&workspace.join("AGENTS.md"), standing)?;
            }
            imported.taken.push(Kind::Instructions);
        }
        if let Some(skills) = &state.skills {
            for skill in skills {
                identity::write_skill(&workspace.join("skills"), skill)?;
            }
            imported.taken.push(Kind::Skills);
        }
        if let Some(notes) = &state.memory {
            crate::identity::remember(&self.home, notes)?;
            imported.taken.push(Kind::Memory);
        }
        let not_taken = [
            (
                Kind::Sessions,
                state.sessions.is_some(),
                "OpenClaw keeps conversations behind its gateway, which takes none from elsewhere",
            ),
            (
                Kind::Tools,
                state.tools.is_some(),
                "OpenClaw's own MCP servers aren't carried yet",
            ),
            (
                Kind::Settings,
                state.settings.is_some(),
                "OpenClaw takes its settings from Link's",
            ),
        ];
        for (kind, given, reason) in not_taken {
            if given {
                imported.not_taken.push(NotTaken {
                    kind,
                    reason: reason.into(),
                });
            }
        }
        Ok(imported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use link_harness_contract::asp;
    use link_harness_hub::Store;

    /// The servers its owner added go out as its tools; Link's own don't.
    #[test]
    fn its_owner_s_servers_are_its_tools() {
        let home = link_test_kit::TempDir::new("openclaw-tools");
        let prefix = home.join(".openclaw");
        std::fs::create_dir_all(&prefix).unwrap();
        let config = json!({"mcp": {"servers": {
            "gmail": {"command": "gmail-mcp", "args": ["--read"], "env": {"GMAIL_USER": "maya"}},
            "memory": {"url": "http://127.0.0.1:9/mcp", "transport": "streamable-http"}
        }}});
        std::fs::write(prefix.join("openclaw.json"), config.to_string()).unwrap();
        std::fs::write(
            prefix.join(crate::configuring::TOOLS_RECORD),
            r#"{"tools":["memory"],"schema":1}"#,
        )
        .unwrap();
        let openclaw = OpenClaw {
            home: home.to_path_buf(),
        };
        let tools = openclaw.export(&[Kind::Tools]).unwrap().tools.unwrap();
        let wire = serde_json::to_value(&tools).unwrap();
        assert_eq!(
            wire,
            json!([{"name": "gmail", "command": "gmail-mcp", "args": ["--read"],
                "env": [{"name": "GMAIL_USER", "value": "maya"}]}])
        );
        assert!(openclaw.export(&[Kind::Skills]).unwrap().tools.is_none());
    }

    #[test]
    fn who_it_is_comes_in_where_configure_writes_the_spec_s() {
        let home = link_test_kit::TempDir::new("openclaw-state");
        let openclaw = OpenClaw {
            home: home.to_path_buf(),
        };
        let workspace = home.join(".openclaw/workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let soul = identity::with_block("Old voice.\n", Some("From Link."));
        std::fs::write(workspace.join("SOUL.md"), soul).unwrap();
        let who = Identity {
            persona: Some("A careful voice.".into()),
            standing: Some("Answer briefly.".into()),
            ..Identity::default()
        };
        let skill = asp::forms::Skill {
            name: "notes".into(),
            files: [(
                "SKILL.md".to_string(),
                "---\nname: notes\ndescription: Keeps notes.\n---\n# Notes\n".to_string(),
            )]
            .into(),
        };
        let state = State {
            instructions: who.all(),
            skills: Some(vec![skill]),
            sessions: Some(Vec::new()),
            ..State::default()
        };
        let imported = openclaw.import(state).unwrap();
        assert_eq!(imported.taken, [Kind::Instructions, Kind::Skills]);
        assert_eq!(imported.not_taken[0].kind, Kind::Sessions);
        let soul = std::fs::read_to_string(workspace.join("SOUL.md")).unwrap();
        assert!(
            soul.contains("A careful voice.") && soul.contains("From Link."),
            "{soul}"
        );
        assert!(!soul.contains("Old voice."), "{soul}");
        let agents = std::fs::read_to_string(workspace.join("AGENTS.md")).unwrap();
        assert!(agents.contains("Answer briefly."), "{agents}");

        let out = openclaw.export(&KINDS).unwrap();
        let back = Identity::of_agents_md(&out.instructions.unwrap());
        assert_eq!(back.persona, who.persona);
        assert_eq!(back.standing, who.standing);
        assert_eq!(out.skills.unwrap()[0].name, "notes");
        assert_eq!(
            openclaw.export(&[Kind::Skills]).unwrap().kinds(),
            [Kind::Skills]
        );
    }
}
