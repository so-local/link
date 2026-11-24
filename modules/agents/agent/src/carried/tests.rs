use std::collections::BTreeMap;

use link_harness_contract::asp::forms::{By, Message, Note, Skill};

use super::*;

fn spec() -> Spec {
    toml::from_str("[instructions]\nskills = \"link\"").unwrap()
}

fn said(by: By, text: &str) -> Message {
    Message {
        by,
        text: text.into(),
    }
}

/// A harness as `lnk agent harness set` meets it: what it exports and takes in
/// over ASP, and what it reads of the agent's own folder at a start.
/// Link Harness keeps its memory in the agent's notes and its skills in
/// the agent's own folder; OpenClaw takes in no sessions.
struct Fake {
    name: &'static str,
    takes_sessions: bool,
    link: bool,
    sessions: BTreeMap<String, Vec<SessionUpdate>>,
    memory: Vec<Note>,
    skills: Vec<Skill>,
    /// What it read at its last start: who else remembered what, and the
    /// agent's skills.
    remembered: Vec<Note>,
    folder_skills: Vec<String>,
}

impl Fake {
    fn new(name: &'static str, takes_sessions: bool, link: bool) -> Self {
        Fake {
            name,
            takes_sessions,
            link,
            sessions: BTreeMap::new(),
            memory: Vec::new(),
            skills: Vec::new(),
            remembered: Vec::new(),
            folder_skills: Vec::new(),
        }
    }

    fn export(&self) -> asp::State {
        let sessions = self
            .sessions
            .iter()
            .map(|(id, updates)| Session {
                evidence: None,
                harness: None,
                session_id: id.clone().into(),
                title: None,
                updates: updates.clone(),
            })
            .collect();
        asp::State {
            sessions: Some(sessions),
            memory: (!self.link).then(|| self.memory.clone()),
            skills: (!self.link).then(|| self.skills.clone()),
            ..asp::State::default()
        }
    }

    fn import(&mut self, state: asp::State) {
        if !self.takes_sessions {
            return;
        }
        // The session it had for a thread stays, the thread the import's.
        for session in state.sessions.into_iter().flatten() {
            let id = session.session_id.0.to_string();
            if let Some(before) = self.sessions.insert(id.clone(), session.updates) {
                self.sessions.insert(format!("{id}#before"), before);
            }
        }
    }

    fn start(&mut self, own: &Path) {
        self.remembered = crate::identity::remembered(own)
            .unwrap()
            .into_iter()
            .filter(|n| n.source.as_deref() != Some(self.name))
            .collect();
        self.folder_skills = crate::skills::list(own)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
    }

    /// A turn on the owner's chat, a memory and a skill, each `n`'s.
    fn work(&mut self, own: &Path, n: usize) {
        let thread = self.sessions.entry("telegram:42".into()).or_default();
        thread.extend(
            Session::of_messages(
                "fake".into(),
                None,
                &[
                    said(By::User, &format!("qa-conversation-{n}")),
                    said(By::Agent, "ok"),
                ],
            )
            .updates,
        );
        let non_text: Vec<SessionUpdate> = serde_json::from_value(serde_json::json!([
            {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": format!("qa-thinking-{n}")}},
            {"sessionUpdate": "tool_call", "toolCallId": format!("c{n}"), "name": "file__read", "title": "Read", "rawInput": {"file_path": "notes.md"}},
            {"sessionUpdate": "tool_call_update", "toolCallId": format!("c{n}"), "status": "completed", "rawOutput": format!("qa-tool-{n}")},
            {"sessionUpdate": "user_message_chunk", "content": {"type": "resource", "resource": {
                "uri": format!("attachment:qa-resource-{n}"), "mimeType": "text/plain", "text": format!("qa-resource-{n}")
            }}}
        ])).unwrap();
        thread.extend(non_text);
        let (memory, skill) = (format!("qa-memory-{n}"), format!("qa-skill-{n}"));
        let text = format!("---\nname: {skill}\ndescription: {skill}\n---\nSay qa.\n");
        match self.link {
            true => {
                let notes = own.join(layout::NOTES);
                let old = std::fs::read_to_string(&notes).unwrap_or_default();
                std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
                std::fs::write(&notes, format!("- {memory}\n{old}")).unwrap();
                let dir = own.join(layout::SKILLS).join(&skill);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("SKILL.md"), text).unwrap();
            }
            false => {
                self.memory.push(Note {
                    path: format!("MEMORY-{n}.md"),
                    text: memory,
                    source: None,
                    deleted: false,
                });
                self.skills.push(Skill {
                    name: skill,
                    files: [("SKILL.md".to_string(), text)].into(),
                });
            }
        }
    }

    fn holds(&self) -> String {
        let mut all: Vec<String> = Vec::new();
        all.push(serde_json::to_string(&self.sessions).unwrap());
        all.extend(
            self.memory
                .iter()
                .chain(&self.remembered)
                .map(|n| n.text.clone()),
        );
        all.extend(self.skills.iter().map(|s| s.name.clone()));
        all.extend(self.folder_skills.iter().cloned());
        all.join("\n")
    }
}

/// `lnk agent harness set`: out of `from`, into `to`, `to` started.
fn switch(own: &Path, from: &Fake, to: &mut Fake) {
    left(&spec(), own, from.name, &from.export()).unwrap();
    to.import(elsewhere(own, to.name).unwrap());
    to.start(own);
}

/// Hermes, OpenClaw, Link Harness, OpenClaw, Hermes: a conversation, a
/// memory and a skill added at each, every one present at the end.
#[test]
fn nothing_is_lost_on_a_round_trip() {
    let own = link_test_kit::TempDir::new("carried-round-trip");
    let mut hermes = Fake::new("hermes", true, false);
    let mut openclaw = Fake::new("openclaw", false, false);
    let mut link = Fake::new("link", true, true);
    hermes.start(&own);
    hermes.work(&own, 1);
    switch(&own, &hermes, &mut openclaw);
    openclaw.work(&own, 2);
    switch(&own, &openclaw, &mut link);
    link.work(&own, 3);
    switch(&own, &link, &mut openclaw);
    openclaw.work(&own, 4);
    switch(&own, &openclaw, &mut hermes);
    hermes.work(&own, 5);
    let held = hermes.holds();
    for n in 1..=5 {
        for what in [
            "conversation",
            "memory",
            "skill",
            "thinking",
            "tool",
            "resource",
        ] {
            let one = format!("qa-{what}-{n}");
            assert!(held.contains(&one), "{one} is lost: {held}");
        }
    }
    // Its chat follows the conversation it left last: OpenClaw's.
    let chat = Session {
        evidence: None,
        harness: None,
        session_id: "telegram:42".into(),
        title: None,
        updates: hermes.sessions["telegram:42"].clone(),
    }
    .messages();
    assert_eq!(chat[0].text, "qa-conversation-2");
    assert_eq!(chat[2].text, "qa-conversation-4");
}

#[test]
fn a_conversation_one_holds_already_isn_t_imported_again() {
    let own = link_test_kit::TempDir::new("carried-held");
    let mut hermes = Fake::new("hermes", true, false);
    let mut link = Fake::new("link", true, true);
    hermes.work(&own, 1);
    switch(&own, &hermes, &mut link);
    assert_eq!(
        Session {
            evidence: None,
            harness: None,
            session_id: "telegram:42".into(),
            title: None,
            updates: link.sessions["telegram:42"].clone()
        }
        .messages()
        .len(),
        2
    );
    // Back, nothing new: nothing to take in.
    let state = link.export();
    left(&spec(), &own, "link", &state).unwrap();
    assert_eq!(elsewhere(&own, "hermes").unwrap().sessions, Some(vec![]));
    // Went on there: the whole of it, on its thread.
    link.work(&own, 2);
    left(&spec(), &own, "link", &link.export()).unwrap();
    let carried = elsewhere(&own, "hermes").unwrap().sessions.unwrap();
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].session_id.0.as_ref(), "telegram:42");
    assert_eq!(carried[0].messages().len(), 4);
    // Named for the harness it came from, so a copy beside one is too.
    assert_eq!(carried[0].harness.as_deref(), Some("link"));
}

#[test]
fn a_harness_s_kept_state_is_refused_under_another_s_name() {
    let own = link_test_kit::TempDir::new("carried-name");
    assert!(keep(&own, "../hermes", &asp::State::default()).is_err());
    keep(&own, "hermes", &asp::State::default()).unwrap();
    keep(&own, "link", &asp::State::default()).unwrap();
    keep(&own, "hermes", &asp::State::default()).unwrap();
    let names: Vec<String> = kept(&own)
        .unwrap()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["link", "hermes"]);
}

#[test]
fn equal_text_does_not_hide_non_text_divergence_or_another_thread() {
    use link_harness_contract::asp::acp::schema::v1::{
        ContentBlock, ContentChunk, SessionUpdate, TextContent,
    };
    let own = link_test_kit::TempDir::new("carried-non-text");
    let session = |id: &str, thought: &str| Session {
        evidence: None,
        harness: None,
        session_id: id.to_string().into(),
        title: None,
        updates: vec![
            SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("Same text"),
            ))),
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new(thought),
            ))),
        ],
    };
    for (name, state) in [
        (
            "hermes",
            asp::State {
                sessions: Some(vec![session("link:main", "One branch")]),
                ..asp::State::default()
            },
        ),
        (
            "link",
            asp::State {
                sessions: Some(vec![session("link:main", "Other branch")]),
                ..asp::State::default()
            },
        ),
        (
            "codex",
            asp::State {
                sessions: Some(vec![session("link:other", "One branch")]),
                ..asp::State::default()
            },
        ),
    ] {
        keep(&own, name, &state).unwrap();
    }
    let carried = elsewhere(&own, "openclaw").unwrap().sessions.unwrap();
    assert_eq!(carried.len(), 3);
    let from_link = elsewhere(&own, "link").unwrap().sessions.unwrap();
    assert_eq!(from_link.len(), 2);
    assert!(
        from_link
            .iter()
            .any(|session| &*session.session_id.0 == "link:other")
    );
    assert!(
        from_link
            .iter()
            .any(|item| item.updates == session("link:main", "One branch").updates)
    );
}

/// A version coming to a harness that doesn't go on from the one it
/// holds goes beside it, named for its harness; one that goes on from
/// it, its words wrapped, keeps the thread.
#[test]
fn a_version_that_parted_goes_beside() {
    let session = |texts: &[&str]| Session {
        evidence: None,
        harness: Some("openclaw".into()),
        session_id: "telegram:42".into(),
        title: None,
        updates: texts
            .iter()
            .map(|text| {
                serde_json::from_value(serde_json::json!({"sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": text}}))
                .unwrap()
            })
            .collect(),
    };
    let state = |texts: &[&str]| asp::State {
        sessions: Some(vec![session(texts)]),
        ..asp::State::default()
    };
    let held = state(&["remember  qa-1"]);
    let mut went_on = state(&["remember  qa-1", "more"]);
    assert!(beside(&held, &mut went_on).is_empty());
    assert_eq!(went_on, state(&["remember  qa-1", "more"]));
    let mut ambiguous = state(&["[Telegram] remember qa-1", "more"]);
    assert!(
        !beside(&held, &mut ambiguous).is_empty(),
        "matching words cannot prove an identical history"
    );
    let mut parted = state(&["hello on openclaw"]);
    let named = beside(&held, &mut parted);
    assert_eq!(
        named,
        [(
            "telegram:42".into(),
            "telegram:42~openclaw".into(),
            "openclaw".into()
        )]
    );
    assert_eq!(
        parted.sessions.unwrap()[0].session_id.0.as_ref(),
        "telegram:42~openclaw"
    );
}
