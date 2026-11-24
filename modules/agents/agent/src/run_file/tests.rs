/// A programmer's whole files folder, `.`, reaches the file tool as a name,
/// never as an argument `lnk sandbox tool add` reads as a path.
#[test]
fn the_folders_it_writes_reach_the_file_tool_as_names() {
    let whole = Sandbox::parse("schema = 1\n[files]\nwrite = [\".\", \"instructions\"]\n").unwrap();
    let (whole, names) = write_names(&whole);
    assert!(whole);
    assert_eq!(names, ["instructions"]);
    assert!(
        names
            .iter()
            .all(|arg| arg != "." && !arg.starts_with("./") && !arg.starts_with("../")),
        "{names:?}"
    );
}

use super::*;

#[test]
fn a_managed_name_does_not_authorize_changed_or_custom_file_processes() {
    let root = link_test_kit::TempDir::new("file-read-origin");
    let program = root.join("lnk-file-tool");
    std::fs::write(&program, "native").unwrap();
    let argv = vec![program.display().to_string(), "serve".into()];
    let mut kept = crate::defined::Tool::new(argv.clone(), &program);
    assert!(current_read_fingerprint(&kept, &program, &argv).is_some());
    assert!(current_read_fingerprint(&kept, &program, &["/custom/file".into()]).is_none());
    kept.modified = None;
    assert!(current_read_fingerprint(&kept, &program, &argv).is_none());
    kept.modified = Some(u64::MAX);
    assert!(current_read_fingerprint(&kept, &program, &argv).is_none());
    kept.modified = crate::defined::modified(&program);
    kept.lnk = "other-build".into();
    assert!(current_read_fingerprint(&kept, &program, &argv).is_none());
}

/// The flags reading the agent's own folder's instructions, skills
/// and conversations, at `agent`: `file`'s.
fn own_read(agent: &str) -> Vec<String> {
    ["instructions", "skills", "chats"]
        .iter()
        .flat_map(|name| ["--files.read".to_string(), format!("{agent}/{name}")])
        .collect()
}

/// [`own_read`] for `run` and `code`, conversations hidden: each `run`
/// call reads its own alone, by its server.
fn own_read_hiding_chats(agent: &str) -> Vec<String> {
    let mut flags: Vec<String> = ["instructions", "skills"]
        .iter()
        .flat_map(|name| ["--files.read".to_string(), format!("{agent}/{name}")])
        .collect();
    flags.extend(["--files.hide".to_string(), format!("{agent}/chats")]);
    flags
}

/// Every conversation's folder is hidden from `run` and `code`, even
/// where the sandbox file grants the whole of Home: a
/// command reads another conversation through neither. `file` reads it,
/// its server holding each call to its own conversation.
#[test]
fn run_and_code_reach_no_conversation_s_folder() {
    let folder =
        link_agent_contract::folder::Folder::new(Path::new("/home/u/Link/Agents/main"), None);
    let teams = settings::TeamReach::default();
    let chats = "/home/u/Link/Agents/main/agent/chats";
    for text in ["schema = 1\n", "schema = 1\n[files]\nwrite = [\".\"]\n"] {
        let sandbox = Sandbox::parse(text).unwrap();
        let reaches = |reach: Reach| {
            let flags = sandbox_flags(&sandbox, &folder, &teams, reach);
            let opened = flags.windows(2).any(|w| {
                ["--files.read", "--files.write"].contains(&w[0].as_str()) && w[1] == chats
            });
            let hidden = flags
                .windows(2)
                .any(|w| w[0] == "--files.hide" && w[1] == chats);
            (opened, hidden)
        };
        assert_eq!(reaches(Reach::Files), (true, false), "{text}");
        assert_eq!(reaches(Reach::Folders), (false, true), "{text}");
        assert_eq!(
            reaches(Reach::Network { upstream: None }),
            (false, true),
            "{text}"
        );
    }
}

/// An agent working in a repository: its own sandbox file, its Home the
/// repository. Each `run` call writes the repository, reads its own
/// folder's instructions, skills and conversations, reaches the
/// file's host, port and tool, and no more; each `file` call the same
/// folders, no network.
#[test]
fn an_agent_in_a_repository_is_each_calls_sandbox() {
    let parse = |text: &str| Sandbox::parse(&format!("schema = 1\n{text}")).unwrap();
    let sandbox = parse(
        "[files]\nwrite = [\".\"]\n[internet]\nhosts = [\"pypi.org\"]\n\
         tcp = [\"github.com:22\"]\nper_minute = 60\n[localhost]\nports = [5432]\n\
         [tools]\nnames = [\"gh\"]\n[commands]\nallow = [\"sh\", \"python3\"]\n",
    );
    let files = Path::new("/home/u/app");
    let teams = settings::TeamReach::default();
    let agent = "/home/u/Link/Agents/main/agent";
    let folder = link_agent_contract::folder::Folder::new(
        Path::new("/home/u/Link/Agents/main"),
        Some(files),
    );
    let file = sandbox_flags(&sandbox, &folder, &teams, Reach::Files);
    let mut wanted = vec!["--files.write".to_string(), "/home/u/app".to_string()];
    wanted.extend(own_read(agent));
    assert_eq!(file, wanted);
    let run = sandbox_flags(&sandbox, &folder, &teams, Reach::Network { upstream: None });
    let mut own = vec!["--files.write".to_string(), "/home/u/app".to_string()];
    own.extend(own_read_hiding_chats(agent));
    assert_eq!(run[..own.len()], own);
    assert_eq!(
        run[own.len()..],
        [
            "--internet.hosts",
            "pypi.org",
            "--internet.per_minute",
            "60",
            "--internet.tcp",
            "github.com:22",
            "--localhost.ports",
            "5432"
        ]
    );
    assert_eq!(sandbox.commands().unwrap().allow, ["sh", "python3"]);
    // A file that writes nothing of it: the repository read, nothing
    // written, no network, no program.
    let none = Sandbox::none();
    let mut wanted = vec!["--files.read".to_string(), "/home/u/app".to_string()];
    wanted.extend(own_read_hiding_chats(agent));
    assert_eq!(
        sandbox_flags(&none, &folder, &teams, Reach::Network { upstream: None }),
        wanted
    );
    assert!(none.commands().is_none());
}

/// A sandbox file granting a folder's dot files (`write_dot_files`)
/// grants them to each call too, so `run`'s git commits in the
/// repository; without it they stay read only to every call.
#[test]
fn dot_files_granted_reach_each_call() {
    let folder = link_agent_contract::folder::Folder::new(
        Path::new("/home/u/Link/Agents/programmer"),
        Some(Path::new("/home/u/repo")),
    );
    let teams = settings::TeamReach::default();
    let dots = |text: &str| {
        let flags = sandbox_flags(
            &Sandbox::parse(&format!("schema = 1\n{text}")).unwrap(),
            &folder,
            &teams,
            Reach::Network { upstream: None },
        );
        flags
            .windows(2)
            .filter(|w| w[0] == "--files.write_dot_files")
            .map(|w| w[1].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        dots("[files]\nwrite = [\".\"]\nwrite_dot_files = [\".\"]\n"),
        ["/home/u/repo"]
    );
    assert_eq!(
        dots("[files]\nwrite = [\"work\"]\nwrite_dot_files = [\"work\"]\n"),
        ["/home/u/repo/work"]
    );
    assert!(dots("[files]\nwrite = [\".\"]\n").is_empty());
}

/// `file` never writes dot files, whatever the sandbox file grants
/// `run` and `code`: its sandbox leaves `.git/hooks` read only.
#[test]
fn the_file_tool_writes_no_dot_files() {
    let folder = link_agent_contract::folder::Folder::new(
        Path::new("/home/u/Link/Agents/programmer"),
        Some(Path::new("/home/u/repo")),
    );
    let teams = settings::TeamReach::default();
    let sandbox =
        Sandbox::parse("schema = 1\n[files]\nwrite = [\".\"]\nwrite_dot_files = [\".\"]\n")
            .unwrap();
    let dots = |reach: Reach| {
        sandbox_flags(&sandbox, &folder, &teams, reach)
            .iter()
            .any(|flag| flag == "--files.write_dot_files")
    };
    assert!(!dots(Reach::Files));
    assert!(dots(Reach::Folders));
    assert!(dots(Reach::Network { upstream: None }));
}

/// Code's servers: those its `[commands]` allow and this machine has;
/// each other said why, an empty command none at all.
#[test]
fn code_starts_the_servers_its_commands_allow_and_this_machine_has() {
    let server = |program: &str| CodeServer {
        command: Some(match program {
            "" => Vec::new(),
            p => vec![p.to_string(), "--stdio".into()],
        }),
        files: Some(vec!["x".into()]),
        roots: None,
        settings: Default::default(),
    };
    let servers = BTreeMap::from([
        ("shell".to_string(), server("sh")),
        ("missing".to_string(), server("no-such-server-here")),
        ("refused".to_string(), server("cat")),
        ("off".to_string(), server("")),
    ]);
    let allow = ["sh", "no-such-server-here"].map(String::from);
    let (usable, unavailable) = code_servers(&servers, &allow, sandbox_file::PATH);
    assert_eq!(usable.keys().collect::<Vec<_>>(), ["shell"]);
    assert_eq!(unavailable["missing"], "this machine doesn't have it");
    assert!(unavailable["refused"].contains("[commands] doesn't allow it"));
    assert!(!unavailable.contains_key("off"));
    let allow = ["sh", "cat"].map(String::from);
    let (usable, unavailable) = code_servers(&servers, &allow, sandbox_file::PATH);
    assert_eq!(usable.len(), 2);
    assert_eq!(unavailable.len(), 1);
}

/// A server named by its absolute path is the program `[commands]`
/// names on `PATH`, and the other way round: the files are compared,
/// never their spellings; another program stays refused.
#[test]
fn a_servers_absolute_path_is_the_program_its_commands_name() {
    let dir = link_test_kit::TempDir::new("code-server-path");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["lsp", "other"] {
        link_test_kit::programs::write_program(&bin.join(name), "#!/bin/sh\n").unwrap();
    }
    let path = bin.display().to_string();
    let absolute = bin.join("lsp").display().to_string();
    let other = bin.join("other").display().to_string();
    let usable = |program: &str, allow: &str| {
        let servers = BTreeMap::from([(
            "lang".to_string(),
            CodeServer {
                command: Some(vec![program.to_string()]),
                files: Some(vec!["x".into()]),
                roots: None,
                settings: Default::default(),
            },
        )]);
        code_servers(&servers, &[allow.to_string()], &path)
            .0
            .contains_key("lang")
    };
    assert!(usable(&absolute, "lsp"));
    assert!(usable("lsp", &absolute));
    assert!(usable(&absolute, &absolute));
    assert!(!usable(&absolute, "other"));
    assert!(!usable("lsp", &other));
}

/// File's sandbox reads Home and its own folder's instructions,
/// skills and conversations, and writes what `[files]` says, nothing
/// more; run's adds what the file grants.
/// A routed agent's `run` goes out as its start does: through its
/// route, behind its proxy, the network as it is and the LAN reaching
/// nothing past it; the hosts its file grants, and no other. Unrouted
/// (`direct`), as its file says.
#[test]
fn a_routed_agents_run_reaches_only_what_its_route_carries() {
    let folder =
        link_agent_contract::folder::Folder::new(Path::new("/home/me/Link/Agents/main"), None);
    let teams = settings::TeamReach::default();
    let sandbox = Sandbox::parse(
        "schema = 1\n[internet]\nhosts = [\"pypi.org\"]\n[lan]\nhosts = [\"nas.local\"]\n",
    )
    .unwrap();
    let exit = "socks5://127.0.0.1:1080";
    let direct = sandbox_flags(&sandbox, &folder, &teams, Reach::Network { upstream: None });
    let routed = sandbox_flags(
        &sandbox,
        &folder,
        &teams,
        Reach::Network {
            upstream: Some(exit),
        },
    );
    assert!(
        direct.windows(2).any(|w| w == ["--lan.hosts", "nas.local"]),
        "{direct:?}"
    );
    assert!(!direct.iter().any(|f| f == "--upstream"), "{direct:?}");
    assert!(
        routed.windows(2).any(|w| w == ["--upstream", exit]),
        "{routed:?}"
    );
    assert!(
        routed
            .windows(2)
            .any(|w| w == ["--internet.hosts", "pypi.org"]),
        "{routed:?}"
    );
    assert!(
        !routed
            .iter()
            .any(|f| f == "--lan.hosts" || f == "nas.local"),
        "{routed:?}"
    );
    // The network as it is, granted: every host, but through the route.
    let host = Sandbox::parse("schema = 1\n[network]\nmode = \"host\"\n").unwrap();
    let direct = sandbox_flags(&host, &folder, &teams, Reach::Network { upstream: None });
    let routed = sandbox_flags(
        &host,
        &folder,
        &teams,
        Reach::Network {
            upstream: Some(exit),
        },
    );
    assert!(
        direct.windows(2).any(|w| w == ["--network.mode", "host"]),
        "{direct:?}"
    );
    assert!(!routed.iter().any(|f| f == "--network.mode"), "{routed:?}");
    assert!(
        routed.windows(2).any(|w| w == ["--internet.hosts", "*"]),
        "{routed:?}"
    );
    assert!(
        routed.windows(2).any(|w| w == ["--upstream", exit]),
        "{routed:?}"
    );
}

#[test]
fn each_call_is_sandboxed_as_the_file_says() {
    let folder =
        link_agent_contract::folder::Folder::new(Path::new("/home/me/Link/Agents/main"), None);
    let files = &folder;
    let agent = "/home/me/Link/Agents/main/agent";
    let none_of_teams = settings::TeamReach::default();
    let sandbox = Sandbox::link();
    let file = sandbox_flags(&sandbox, files, &none_of_teams, Reach::Files);
    // Link's: its work, instructions and skills written, its
    // conversations read.
    let wanted: Vec<String> = [
        "--files.read",
        "/home/me/Link/Agents/main/home",
        "--files.write",
        "/home/me/Link/Agents/main/home/work",
        "--files.write",
        "/home/me/Link/Agents/main/agent/instructions",
        "--files.write",
        "/home/me/Link/Agents/main/agent/skills",
        "--files.read",
        "/home/me/Link/Agents/main/agent/chats",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(file, wanted);
    let run = sandbox_flags(
        &sandbox,
        files,
        &none_of_teams,
        Reach::Network { upstream: None },
    );
    let mut own: Vec<String> = file[..8].to_vec();
    own.extend(["--files.hide".to_string(), format!("{agent}/chats")]);
    assert_eq!(run[..own.len()], own);
    assert_eq!(run[own.len()..], ["--internet.hosts", "*"]);
    assert!(
        !run.windows(2)
            .any(|w| w[0] == "--files.write" && w[1].ends_with("chats"))
    );
    // Its instructions, granted: written where they're kept.
    let mine =
        Sandbox::parse("schema = 1\n[files]\nwrite = [\"work\", \"instructions\"]\n").unwrap();
    let flags = sandbox_flags(&mine, files, &none_of_teams, Reach::Files);
    assert!(
        flags
            .windows(2)
            .any(|w| w == ["--files.write", &format!("{agent}/instructions")]),
        "{flags:?}"
    );
    assert!(
        !flags
            .windows(2)
            .any(|w| w == ["--files.read", &format!("{agent}/instructions")])
    );
    let narrow = Sandbox::parse(
        "schema = 1\n[files]\nwrite = [\"work\"]\nread = [\"~\"]\n[internet]\nhosts = [\"pypi.org\"]\n\
         tcp = [\"github.com:22\"]\nper_minute = 60\n[tools]\nnames = [\"gh\"]\n",
    )
    .unwrap();
    let run = sandbox_flags(
        &narrow,
        files,
        &none_of_teams,
        Reach::Network { upstream: None },
    );
    for pair in [
        ["--internet.hosts", "pypi.org"],
        ["--internet.tcp", "github.com:22"],
        ["--internet.per_minute", "60"],
        ["--files.read", "~"],
    ] {
        assert!(run.windows(2).any(|w| w == pair), "{pair:?} in {run:?}");
    }
    // Its tools are its harness's alone: never a program's it runs.
    assert!(!run.iter().any(|f| f.starts_with("--tools.")), "{run:?}");
    assert!(!run.iter().any(|f| f == "*"));
    // Nothing at all: the folder read, no network.
    let none = sandbox_flags(
        &Sandbox::none(),
        files,
        &none_of_teams,
        Reach::Network { upstream: None },
    );
    let mut wanted = vec![
        "--files.read".to_string(),
        "/home/me/Link/Agents/main/home".to_string(),
    ];
    wanted.extend(own_read_hiding_chats(agent));
    assert_eq!(none, wanted);
    // A team's folders: what it's granted, and every team hidden.
    let teams = settings::TeamReach {
        write: vec!["/home/me/Link/Teams/research/shared/drafts".into()],
        read: vec!["/home/me/Link/Teams/research/shared/sources".into()],
        hide: vec!["/home/me/Link/Teams".into()],
        teams: Vec::new(),
        hosted: Vec::new(),
    };
    let file = sandbox_flags(&Sandbox::none(), files, &teams, Reach::Files);
    let mut wanted = vec![
        "--files.read".to_string(),
        "/home/me/Link/Agents/main/home".to_string(),
    ];
    wanted.extend(own_read(agent));
    wanted.extend(
        [
            "--files.write",
            "/home/me/Link/Teams/research/shared/drafts",
            "--files.read",
            "/home/me/Link/Teams/research/shared/sources",
            "--files.hide",
            "/home/me/Link/Teams",
        ]
        .map(String::from),
    );
    assert_eq!(file, wanted);
}

/// A command stops a tenth before its tool's timeout, at least 2 s
/// before it and never more than half; one above
/// `[tools] timeout`, where its namespace stops waiting while the
/// command runs on, is refused, saying which to raise.
#[test]
fn a_tool_timeout_above_every_calls_is_refused() {
    let some = |text: &str| Some(text.parse::<link_actions::value::Duration>().unwrap());
    assert_eq!(
        command_ms_of("run", some("2m"), some("5m")).unwrap(),
        108_000
    );
    assert_eq!(
        command_ms_of("run", some("5m"), some("5m")).unwrap(),
        270_000
    );
    assert_eq!(command_ms_of("code", some("5m"), None).unwrap(), 270_000);
    // A short one keeps the least back, which covers its sandbox's start
    // and its output's drain; a shorter one, half.
    assert_eq!(command_ms_of("run", some("10s"), None).unwrap(), 8_000);
    assert_eq!(command_ms_of("run", some("3s"), None).unwrap(), 1_500);
    let refused = command_ms_of("run", some("10m"), some("5m")).unwrap_err();
    assert!(
        refused.to_string().contains("[tool.run] timeout")
            && refused.to_string().contains("raise [tools] timeout"),
        "{refused}"
    );
    assert!(command_ms_of("run", None, None).is_err());
}

/// `file`'s server is given the grants its `apply` is: the folders it
/// writes (`--write`), all of Home (`--whole`), and each team's folders
/// written and read (`--team-write`, `--team-read`), which its
/// instructions say to the model.
#[test]
fn file_s_server_says_the_grants_its_apply_holds() {
    let folder =
        link_agent_contract::folder::Folder::new(Path::new("/home/u/Link/Agents/main"), None);
    let teams = settings::TeamReach {
        teams: vec![(
            "crew".into(),
            "/home/u/Link/Teams/crew/shared".into(),
            link_harness_contract::harness::TeamFolders {
                write: vec!["drafts".into()],
                read: vec![".".into()],
            },
        )],
        ..Default::default()
    };
    let sandbox =
        Sandbox::parse("schema = 1\n[files]\nwrite = [\".\", \"instructions\"]\n").unwrap();
    let mut serve = link_file_tool_contract::cli::FileToolServeArgs {
        no_find: false,
        no_diffs: false,
        team_bridge: Vec::new(),
        socket: None,
        r#as: None,
        account: None,
        keyring: None,
        command: Vec::new(),
        write: Vec::new(),
        whole: false,
        team_write: Vec::new(),
        team_read: Vec::new(),
    };
    let apply = file_grants(&sandbox, &teams, &folder.home, &folder, &mut serve);
    assert!(apply.whole && serve.whole);
    assert_eq!(serve.write, ["instructions"]);
    assert_eq!(serve.write, apply.write);
    assert_eq!(serve.team_write, ["crew/drafts"]);
    assert_eq!(serve.team_read, ["crew/."]);
    assert_eq!(
        (&serve.team_write, &serve.team_read),
        (&apply.team_write, &apply.team_read)
    );
    let argv = serve.argv();
    for flag in ["--whole", "--write", "--team-write", "--team-read"] {
        assert!(argv.iter().any(|a| a == flag), "{flag} in {argv:?}");
    }
}

/// A `run` command and a `code` server run with the developer tools when
/// the sandbox file grants them on this system, as its harness does:
/// `[commands.macos]` grants them on a Mac alone.
#[test]
fn the_developer_tools_reach_run_and_code_where_granted() {
    let granted = Sandbox::parse(
        "schema = 1\n[commands]\nallow = [\"*\"]\n[commands.macos]\ndeveloper_tools = true\n",
    )
    .unwrap();
    let flag = ["--commands.developer_tools".to_string()];
    match cfg!(target_os = "macos") {
        true => assert_eq!(developer_tools_flags(&granted), flag),
        false => assert!(developer_tools_flags(&granted).is_empty()),
    }
    let off = Sandbox::parse("schema = 1\n[commands]\nallow = [\"*\"]\n").unwrap();
    assert!(developer_tools_flags(&off).is_empty());
}
