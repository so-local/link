use super::*;

fn otter() -> Vec<Trace> {
    vec![
        Trace::new(Kind::Home, "~/.otter"),
        Trace::new(Kind::Config, "~/.otter/otter.json"),
        Trace::new(Kind::Secrets, "~/.otter/.env"),
        Trace::new(Kind::Login, "~/.otter/auth.json"),
        Trace::new(Kind::Program, "~/.local/bin/otter"),
        Trace::new(Kind::Service, "~/.config/systemd/user/otter.service"),
        Trace::new(Kind::Lock, "/tmp/otter-{uid}"),
    ]
}

/// OpenClaw's shape: agents sharing a home, each with its folder and
/// its workspace.
fn claw() -> Vec<Trace> {
    let mut traces = otter();
    traces.push(Trace::new(Kind::Agent, "~/.otter/agents/{agent}"));
    traces.push(Trace::new(Kind::Agent, "~/.otter/workspace{-agent}"));
    traces
}

/// Hermes's shape: profiles, each a whole home of its own.
fn profiled() -> Vec<Trace> {
    let mut traces = otter();
    traces.push(Trace::new(Kind::Profile, "~/.otter/profiles/{agent}"));
    traces
}

fn install(home: &Path) {
    for dir in [".otter/state", ".local/bin", ".config/systemd/user"] {
        std::fs::create_dir_all(home.join(dir)).unwrap();
    }
    std::fs::write(home.join(".otter/otter.json"), "{}").unwrap();
    std::fs::write(
        home.join(".otter/.env"),
        "# keys\nexport OTTER_KEY=\"k-1\"\nBOT_TOKEN='t-2'\n\nnot a line\nTELEGRAM_BOT_TOKEN=1:b\n",
    )
    .unwrap();
    std::fs::write(home.join(".otter/auth.json"), "{}").unwrap();
    std::fs::write(home.join(".otter/state/memory.md"), "remember").unwrap();
    std::fs::write(home.join(".local/bin/otter"), "").unwrap();
    std::fs::write(home.join(".config/systemd/user/otter.service"), "").unwrap();
}

fn rel(p: &str) -> Relative {
    Relative::new(p).unwrap()
}

/// An install is found by its home, each of its places there with its
/// kind, relative to the home folder; the plan stops its service before
/// copying the home once (its settings inside it go with it), adds each
/// secret of its `.env`, and says what stays behind.
#[test]
fn an_install_is_found_by_its_list_and_planned_step_by_step() {
    let home = link_test_kit::TempDir::new("migrating");
    assert_eq!(find("otter", &otter(), &home, "501"), None);
    install(&home);
    let found = find("otter", &otter(), &home, "501").unwrap();
    assert_eq!(found.agents.len(), 1);
    let own = found.own();
    assert_eq!(own.places.len(), 6);
    let steps = plan(own, &home, true).unwrap();
    assert_eq!(
        steps,
        [
            Step::Agent,
            Step::Service {
                at: rel(".config/systemd/user/otter.service"),
            },
            Step::Copy {
                from: rel(".otter"),
                to: rel(".otter"),
                leaving: vec![],
            },
            Step::Secret {
                name: "OTTER_KEY".into(),
            },
            Step::Secret {
                name: "BOT_TOKEN".into(),
            },
        ]
    );
    let words = words("otter", own, &steps, "main", &home.join("H"), &home).join("\n");
    for said in [
        "Making the agent main",
        "Leaving OTTER_KEY in the original's settings",
        "Stopping its own service",
        "Its telegram bot goes to Link's connection",
        "auth.json, can't move",
        ".local/bin/otter stays where it is",
    ] {
        assert!(words.contains(said), "{said}: {words}");
    }
    let values: Vec<String> = env_file(&home.join(".otter/.env"))
        .unwrap()
        .into_iter()
        .map(|(_, v)| v.expose_owned())
        .collect();
    assert_eq!(values, ["k-1", "t-2", "1:b"]);
    // The bot's token goes to its connection, never a variable of the harness's.
    assert_eq!(connections(own, &home), ["telegram"]);
    let tokens = tokens_for("telegram", own, &home);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, "LNK_TELEGRAM_TOKEN");
}

/// Agents sharing a home each become one of Link's: the install's own
/// copies the home without the others' folders; another copies it
/// without any agent's, then its own folders where the install's own
/// agent's are. Only one plan stops the service, and a secret two share
/// is added once.
#[test]
fn agents_sharing_a_home_are_each_planned() {
    let home = link_test_kit::TempDir::new("claw");
    install(&home);
    for dir in [
        "agents/main/sessions",
        "agents/work/sessions",
        "workspace",
        "workspace-work",
    ] {
        std::fs::create_dir_all(home.join(".otter").join(dir)).unwrap();
    }
    let found = find("otter", &claw(), &home, "501").unwrap();
    let names: Vec<_> = found.agents.iter().map(|a| a.name.as_deref()).collect();
    assert_eq!(names, [None, Some("work")]);
    let own = plan(&found.agents[0], &home, true).unwrap();
    let work = plan(&found.agents[1], &home, false).unwrap();
    assert!(matches!(&own[2], Step::Copy { leaving, .. }
        if *leaving == [rel(".otter/agents/work"), rel(".otter/workspace-work")]));
    assert_eq!(
        work[..4],
        [
            Step::Agent,
            Step::Copy {
                from: rel(".otter"),
                to: rel(".otter"),
                leaving: vec![
                    rel(".otter/agents/work"),
                    rel(".otter/workspace-work"),
                    rel(".otter/agents/main"),
                    rel(".otter/workspace"),
                ],
            },
            Step::Copy {
                from: rel(".otter/agents/work"),
                to: rel(".otter/agents/main"),
                leaving: vec![],
            },
            Step::Copy {
                from: rel(".otter/workspace-work"),
                to: rel(".otter/workspace"),
                leaving: vec![],
            },
        ]
    );
    assert!(!work.iter().any(|s| matches!(s, Step::Service { .. })));
    let said = words(
        "otter",
        &found.agents[1],
        &work,
        "work",
        &home.join("H"),
        &home,
    )
    .join("\n");
    assert!(
        said.contains("Leaving OTTER_KEY in the original's settings"),
        "{said}"
    );
    // Its folders counted as folders: two agents' worth, never four agents.
    assert!(said.contains("but 4 folders of its other agents"), "{said}");
}

/// A profile is a whole home of its own: copied as the agent's home, its
/// own secrets read from it; the install's own leaves it out.
#[test]
fn a_profile_is_an_agent_of_its_own() {
    let home = link_test_kit::TempDir::new("profiles");
    install(&home);
    let kit = home.join(".otter/profiles/kit");
    std::fs::create_dir_all(&kit).unwrap();
    std::fs::write(kit.join("otter.json"), "{}").unwrap();
    std::fs::write(kit.join(".env"), "KIT_KEY=k\n").unwrap();
    // Its own secrets are read even when the install's own has none.
    std::fs::remove_file(home.join(".otter/.env")).unwrap();
    let found = find("otter", &profiled(), &home, "501").unwrap();
    assert_eq!(found.agents[0].leaving, [rel(".otter/profiles/kit")]);
    let kit = &found.agents[1];
    assert_eq!(kit.name.as_deref(), Some("kit"));
    let steps = plan(kit, &home, false).unwrap();
    assert_eq!(
        steps,
        [
            Step::Agent,
            Step::Copy {
                from: rel(".otter/profiles/kit"),
                to: rel(".otter"),
                leaving: vec![],
            },
            Step::Secret {
                name: "KIT_KEY".into(),
            },
        ]
    );
}

/// A copy lands whole, links as links, each file checked, other agents'
/// folders left out; copied again, it replaces the first.
#[test]
fn a_copy_is_whole_and_checked() {
    let home = link_test_kit::TempDir::new("copied");
    install(&home);
    std::os::unix::fs::symlink("state/memory.md", home.join(".otter/latest")).unwrap();
    std::fs::create_dir_all(home.join(".otter/profiles/kit")).unwrap();
    let to = home.join(".lnk/data/agents/main/harnesses/otter/.otter");
    let leaving = [home.join(".otter/profiles/kit")];
    copy_checked(&home.join(".otter"), &to, &leaving).unwrap();
    std::fs::write(to.join("stale"), "").unwrap();
    copy_checked(&home.join(".otter"), &to, &leaving).unwrap();
    assert!(!to.join("stale").exists());
    assert!(to.join("profiles").is_dir() && !to.join("profiles/kit").exists());
    assert_eq!(
        std::fs::read_to_string(to.join("state/memory.md")).unwrap(),
        "remember"
    );
    assert_eq!(
        std::fs::read_link(to.join("latest")).unwrap(),
        Path::new("state/memory.md")
    );
    assert!(
        home.join(".otter/state/memory.md").exists(),
        "the original stays"
    );
}

/// A database is copied whole by SQLite, what's only in its write-ahead
/// log with it and the log itself never copied; every text file and
/// every database of the copy then names where it is, whole names only.
#[test]
fn a_copys_databases_and_sessions_name_where_they_are() {
    let home = link_test_kit::TempDir::new("sessions");
    install(&home);
    let was = home.join(".otter");
    let portable =
        link_files::state::path(&was, link_files::state::Area::Context).join("original.json");
    std::fs::create_dir_all(portable.parent().unwrap()).unwrap();
    let original = format!("{{\"evidence\":\"{}/state/memory.md\"}}", was.display());
    std::fs::write(&portable, &original).unwrap();
    std::fs::write(
        was.join("state/one.jsonl"),
        format!(
            "{{\"read\":\"{0}/state/memory.md\",\"old\":\"{0}-old\"}}\n",
            was.display()
        ),
    )
    .unwrap();
    let live = rusqlite::Connection::open(was.join("state.db")).unwrap();
    live.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT);
         INSERT INTO messages (content) VALUES ('wrote {}/state/memory.md');",
        was.display()
    ))
    .unwrap();
    assert!(was.join("state.db-wal").exists());
    let to = home.join(".lnk/data/agents/main/harnesses/otter/.otter");
    copy_checked(&was, &to, &[]).unwrap();
    assert!(!to.join("state.db-wal").exists());
    assert_eq!(rewrite(&to, &[(was.clone(), to.clone())]).unwrap(), 2);
    assert_eq!(
        std::fs::read_to_string(
            link_files::state::path(&to, link_files::state::Area::Context).join("original.json")
        )
        .unwrap(),
        original
    );
    let session = std::fs::read_to_string(to.join("state/one.jsonl")).unwrap();
    assert!(
        session.contains(&format!("\"{}/state/memory.md\"", to.display())),
        "{session}"
    );
    assert!(
        session.contains(&format!("\"{}-old\"", was.display())),
        "{session}"
    );
    let copied = rusqlite::Connection::open(to.join("state.db")).unwrap();
    let content: String = copied
        .query_row("SELECT content FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(content, format!("wrote {}/state/memory.md", to.display()));
    drop(live);
}

/// A database nothing holds open is copied and left as it was: no log
/// or shared memory of SQLite's left beside it in the original.
#[test]
fn a_closed_database_is_left_with_nothing_beside_it() {
    let home = link_test_kit::TempDir::new("closed");
    install(&home);
    let was = home.join(".otter");
    rusqlite::Connection::open(was.join("state.db"))
        .unwrap()
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT);
             INSERT INTO messages (content) VALUES ('kept');",
        )
        .unwrap();
    let before = std::fs::read(was.join("state.db")).unwrap();
    copy_checked(&was, &home.join("copy"), &[]).unwrap();
    assert!(!was.join("state.db-wal").exists());
    assert!(!was.join("state.db-shm").exists());
    assert_eq!(std::fs::read(was.join("state.db")).unwrap(), before);
}

/// A database whose log still holds writes, as a harness that stopped
/// without closing leaves it, is copied with them and left as it was:
/// its writes stay in its log, never folded into it.
#[test]
fn a_database_with_writes_in_its_log_is_left_as_it_was() {
    let home = link_test_kit::TempDir::new("crashed");
    install(&home);
    let was = home.join(".otter");
    // Written elsewhere and never closed, then the database and its log
    // put in place: a harness that stopped without closing, with nothing
    // of this process holding them.
    let elsewhere = home.join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    let live = rusqlite::Connection::open(elsewhere.join("state.db")).unwrap();
    live.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA wal_autocheckpoint = 0;
         CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT);
         INSERT INTO messages (content) VALUES ('only in the log');",
    )
    .unwrap();
    for name in ["state.db", "state.db-wal"] {
        std::fs::copy(elsewhere.join(name), was.join(name)).unwrap();
    }
    let files = |name: &str| std::fs::read(was.join(name)).unwrap();
    let before = (files("state.db"), files("state.db-wal"));
    let to = home.join("copy");
    copy_checked(&was, &to, &[]).unwrap();
    assert_eq!((files("state.db"), files("state.db-wal")), before);
    let content: String = rusqlite::Connection::open(to.join("state.db"))
        .unwrap()
        .query_row("SELECT content FROM messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(content, "only in the log");
    drop(live);
}

/// A home kept elsewhere and linked into place is copied through its
/// link: the copy is its folder, whole, never a link left dangling, and
/// what it leaves is left by either name.
#[test]
fn a_linked_home_is_copied_through_its_link() {
    let home = link_test_kit::TempDir::new("linked");
    let real = home.join("dotfiles/otter");
    std::fs::create_dir_all(real.join("agents/work")).unwrap();
    std::fs::write(real.join("MEMORY.md"), "remember the whale").unwrap();
    std::fs::write(real.join("agents/work/one.jsonl"), "work's").unwrap();
    std::os::unix::fs::symlink("dotfiles/otter", home.join(".otter")).unwrap();
    let to = home.join("H/.otter");
    copy_checked(
        &home.join(".otter"),
        &to,
        &[home.join(".otter/agents/work")],
    )
    .unwrap();
    assert!(!to.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(
        std::fs::read_to_string(to.join("MEMORY.md")).unwrap(),
        "remember the whale"
    );
    assert!(!to.join("agents/work").exists());
}

/// A hosted model's key is each agent's own model key, never a variable
/// of its harness's; any other secret is left in the original for its
/// owner to give sealed, out of every agent's copy.
#[test]
fn a_hosted_models_key_is_each_agents_model_key() {
    let home = link_test_kit::TempDir::new("keys");
    install(&home);
    std::fs::write(
        home.join(".otter/.env"),
        "ANTHROPIC_API_KEY=sk-ant-k1\nOPENAI_API_KEY=not-a-key\nSEARCH_KEY=s-1\n",
    )
    .unwrap();
    for dir in ["agents/main", "agents/work", "workspace", "workspace-work"] {
        std::fs::create_dir_all(home.join(".otter").join(dir)).unwrap();
    }
    let found = find("otter", &claw(), &home, "501").unwrap();
    let own = plan(&found.agents[0], &home, true).unwrap();
    let work = plan(&found.agents[1], &home, false).unwrap();
    let secrets = |steps: &[Step]| -> Vec<Step> {
        steps
            .iter()
            .filter(|s| matches!(s, Step::Secret { .. } | Step::Model { .. }))
            .cloned()
            .collect()
    };
    let model = Step::Model {
        name: "ANTHROPIC_API_KEY".into(),
    };
    // A provider's variable that holds no key of its is a secret like any.
    let named = |name: &str| Step::Secret { name: name.into() };
    assert_eq!(
        secrets(&own),
        [model.clone(), named("OPENAI_API_KEY"), named("SEARCH_KEY")]
    );
    assert_eq!(secrets(&work), secrets(&own));
    let said = words(
        "otter",
        &found.agents[0],
        &own,
        "main",
        &home.join("H"),
        &home,
    )
    .join("\n");
    for line in [
        "Making ANTHROPIC_API_KEY main's model key",
        "lnk secret add SEARCH_KEY --to agent:main --host <its host>",
        "taken out of the copy's settings",
    ] {
        assert!(said.contains(line), "{line}: {said}");
    }
}
