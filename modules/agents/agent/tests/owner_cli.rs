//! Model selection persists only after validating the provider's ID.
use link_test_kit::{TempDir, env};

#[cfg(unix)]
#[test]
fn uninstall_stops_the_process_holding_an_agents_foreground_lock() {
    use std::{io::BufRead, process::Stdio, time::Duration};
    if !link_test_kit::gate::program("python3", &["--version"]) {
        return;
    }
    let home = TempDir::new("uninstall-foreground");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let lock = link_files::layout::Hidden::Agents
        .in_home(&home)
        .join("main/foreground.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let mut fixture = env::command("python3", &home);
    fixture
        .args([
            "-I",
            "-c",
            "import fcntl,os,signal,sys; f=open(sys.argv[1],'w'); fcntl.flock(f,fcntl.LOCK_EX); f.write(str(os.getpid())); f.flush(); print('locked',flush=True); signal.pause()",
        ])
        .arg(&lock)
        .stdout(Stdio::piped());
    let mut process = link_test_kit::process::ChildGroup::spawn(&mut fixture).unwrap();
    let mut ready = String::new();
    std::io::BufReader::new(process.take_stdout().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "locked");
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .arg("uninstall")
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    link_test_kit::wait::eventually(
        "uninstall stops the agent in its terminal",
        Duration::from_secs(2),
        || process.try_status().unwrap().is_some(),
    );
    assert!(home.join("Link/Safe/Agents/main/agent.toml").is_file());
}

#[test]
fn an_empty_piped_message_is_refused_with_a_declared_next() {
    let home = TempDir::new("empty-chat-message");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let spec = home.join("Link/Safe/Agents/main/agent.toml");
    let before = std::fs::read(&spec).unwrap();
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .args(["chat", "send"])
        .stdin(std::process::Stdio::null())
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("There's no message to send."), "{said}");
    assert!(said.contains("--message"), "{said}");
    assert!(!said.contains("panicked"), "{said}");
    assert_eq!(std::fs::read(&spec).unwrap(), before);
}

#[tokio::test]
async fn model_status_reaches_the_proxy_without_loading_provider_credentials() {
    let home = TempDir::new("model-status-route");
    let held = link_test_kit::net::Held::loopback();
    let port = held.port();
    let listener = held.listener();
    listener.set_nonblocking(true).unwrap();
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let app = axum::Router::new().route(
        "/{*path}",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({
                "name": "claude-haiku-5-5",
                "endpoint": {"ready": true, "loaded": ["claude-haiku-5-5"]},
            }))
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    link_agent_contract::testing::write_agent(
        &home,
        "main",
        &format!(
            "harness = \"link\"\nsandbox = false\nblock = 1\nport = {port}\n[model]\nprovider = \"anthropic\"\nname = \"claude-haiku-5-5\"\n"
        ),
    );
    // Asked by a person, and with --json.
    let outputs = tokio::task::spawn_blocking(move || {
        [&["model", "status"][..], &["model", "status", "--json"]].map(|args| {
            env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
                .args(args)
                .env("LNK_SERVICES", "off")
                .env("NO_PROXY", "127.0.0.1,localhost")
                .output()
        })
    })
    .await;
    server.abort();
    let [shown, output] = outputs.unwrap().map(Result::unwrap);
    assert!(shown.status.success(), "{shown:?}");
    // A person reads fields, never the JSON.
    let said = String::from_utf8_lossy(&shown.stdout);
    assert!(!said.contains('{'), "{said}");
    for part in ["Ready", "yes", "Loaded", "claude-haiku-5-5"] {
        assert!(said.contains(part), "{part}: {said}");
    }
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ready"], true);
    assert_eq!(value["loaded"][0], "claude-haiku-5-5");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("outside the grant"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn model_status_reaches_an_isolated_proxy_through_its_existing_socket() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let home = TempDir::short("model-api");
    let runtime = link_files::layout::Root::Runtime.default_in(&link_test_kit::env::runtime(&home));
    // Its sockets are keyed by its ID, never its name.
    let id = "m0d3l5tatu5a";
    let socket = link_files::layout::Runtime::AgentNetwork
        .at(&runtime)
        .join(id)
        .join("link/out.sock");
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (mut client, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            client.read_exact(&mut byte).await.unwrap();
            head.push(byte[0]);
            assert!(head.len() < 8192);
        }
        assert!(String::from_utf8_lossy(&head).starts_with("GET /_link/url/"));
        let body = r#"{"name":"qa-model","endpoint":{"ready":true,"loaded":["qa-model"]}}"#;
        client
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    });
    link_agent_contract::testing::write_agent(
        &home,
        "main",
        &format!(
            "id = \"agent:{id}\"\nharness = \"link\"\nsandbox = true\n[model]\nprovider = \"anthropic\"\nname = \"qa-model\"\n"
        ),
    );
    let output = tokio::task::spawn_blocking(move || {
        env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
            .args(["model", "status", "--json"])
            .env("LNK_SERVICES", "off")
            .output()
    })
    .await
    .unwrap()
    .unwrap();
    if !output.status.success() {
        server.abort();
        panic!("{output:?}");
    }
    server.await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ready"], true);
    assert_eq!(value["loaded"][0], "qa-model");
}

#[test]
fn restart_without_an_existing_agent_does_not_create_one() {
    let home = TempDir::new("restart-missing");
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .args(["restart"])
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("There is no agent yet"),
        "{output:?}"
    );
    assert!(!home.join("Link/Safe/Agents/main/agent.toml").exists());
}

#[test]
fn tool_states_are_on_or_off_and_only_file_starts_on() {
    let home = TempDir::new("tool-states");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .args(["tool", "list"])
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let said = String::from_utf8_lossy(&output.stdout);
    assert!(
        !said.contains("comes with") && !said.contains("not on"),
        "{said}"
    );
    // File starts on, so asked to, it changes its files, instructions
    // and skills; every other tool is off until added.
    for (name, state) in [
        ("scheduler", "off"),
        ("run", "off"),
        ("file", "on"),
        ("code", "off"),
        ("browser", "off"),
    ] {
        let row = said
            .lines()
            .find(|line| line.split_whitespace().next() == Some(name))
            .unwrap();
        assert_eq!(row.split_whitespace().nth(1), Some(state), "{row}");
    }
}

#[test]
fn built_in_tools_are_enabled_and_removed_explicitly_without_changing_file_grants() {
    let home = TempDir::new("explicit-tools");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let spec = home.join("Link/Safe/Agents/main/agent.toml");
    let server = home.join("test-language-server");
    link_test_kit::write_program(&server, "#!/bin/sh\nexit 0\n").unwrap();
    let server_name = toml::Value::String(server.display().to_string()).to_string();
    let mut behavior = link_agent_spec::link();
    behavior.extends = Some(link_agent_spec::Extends::None);
    behavior.command_response = Some("Harness".into());
    // File starts on in Link's spec: switched off here, so adding it is
    // seen.
    behavior.tool.get_mut("file").unwrap().enabled = Some(false);
    behavior.tool.get_mut("file").unwrap().find = Some(false);
    let configured = link_agent_spec::parse(&format!("schema = 1\n[tool.code.servers.test]\ncommand = [{server_name}]\nfiles = [\"txt\"]\nroots = [\"project.toml\"]\n")).unwrap();
    behavior.tool.get_mut("code").unwrap().servers = configured.tool("code").servers;
    let written = link_agent::settings::configuration::with_behavior(
        &std::fs::read_to_string(&spec).unwrap(),
        &link_spec::print_spec(&behavior).unwrap(),
    )
    .unwrap();
    // A comment of yours on file's switch, wherever in its table the
    // switch is written.
    let file = written.find("[behavior.tool.file]\n").unwrap();
    let switch = file + written[file..].find("\nenabled = false\n").unwrap() + 1;
    let line = "enabled = false";
    let written = format!(
        "{}{line} # My switch{}",
        &written[..switch],
        &written[switch + line.len()..]
    );
    assert!(written.contains("enabled = false # My switch"));
    std::fs::write(&spec, format!("# Keep my instructions\n{written}")).unwrap();
    let grants = home.join("Link/Safe/Agents/main/sandbox.toml");
    std::fs::create_dir_all(grants.parent().unwrap()).unwrap();
    let original_grants = format!(
        "schema = 1\n[files]\nwrite = [\"work\"]\n[commands]\nallow = [\"sh\", {server_name}]\n"
    );
    std::fs::write(&grants, &original_grants).unwrap();
    let run = |args: &[&str]| {
        let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
            .args(args)
            .env("LNK_SERVICES", "off")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let state = |tool: &str| {
        let listed = run(&["tool", "list"]);
        listed
            .lines()
            .find(|line| line.split_whitespace().next() == Some(tool))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string()
    };
    for tool in ["scheduler", "run", "file", "code"] {
        assert_eq!(state(tool), "off");
        run(&["tool", "add", tool]);
        assert_eq!(state(tool), "on");
        let enabled = std::fs::read(&spec).unwrap();
        run(&["tool", "add", tool]);
        assert_eq!(std::fs::read(&spec).unwrap(), enabled);
        let parsed: toml::Table = std::fs::read_to_string(&spec).unwrap().parse().unwrap();
        assert_eq!(
            parsed["behavior"]["tool"][tool]["enabled"].as_bool(),
            Some(true)
        );
        run(&["tool", "remove", tool]);
        assert_eq!(state(tool), "off");
        let disabled = std::fs::read(&spec).unwrap();
        run(&["tool", "remove", tool]);
        assert_eq!(std::fs::read(&spec).unwrap(), disabled);
    }
    let kept = std::fs::read_to_string(&spec).unwrap();
    assert!(kept.contains("# Keep my instructions"));
    assert!(kept.contains("# My switch"));
    let parsed: toml::Table = kept.parse().unwrap();
    assert_eq!(
        parsed["behavior"]["tool"]["file"]["find"].as_bool(),
        Some(false)
    );
    assert_eq!(
        parsed["behavior"]["command_response"].as_str(),
        Some("Harness")
    );
    assert_eq!(std::fs::read_to_string(&grants).unwrap(), original_grants);
}

#[test]
fn invalid_spec_is_not_overwritten_when_enabling_a_tool() {
    let home = TempDir::new("tool-invalid-spec");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let path = home.join("Link/Safe/Agents/main/agent.toml");
    let original = "[behavior.tool.file]\nenabled = \"invalid\"\n";
    std::fs::write(&path, original).unwrap();
    let output = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
        .args(["tool", "add", "file"])
        .env("LNK_SERVICES", "off")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[test]
fn missing_configuration_files_are_recreated_with_closed_grants_and_the_same_identity() {
    let home = TempDir::new("recreate-agent-config");
    let core = home.join("lnk");
    link_test_kit::write_core(&core, "#!/bin/sh\nprintf '[]\\n'\n").unwrap();
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let run = |args: &[&str]| {
        env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
            .args(args)
            .env("LNK_EXE", &core)
            .env("LNK_SERVICES", "off")
            .env("ANTHROPIC_API_KEY", "sk-ant-test-recreate-config")
            .output()
            .unwrap()
    };
    let selected = run(&["model", "set", "claude-haiku-5-5"]);
    assert!(selected.status.success(), "{selected:?}");
    let directory = home.join("Link/Safe/Agents/main");
    let path = directory.join("agent.toml");
    let before: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let credentials = std::fs::read(home.join(".lnk/safe/secrets/secrets.toml")).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(directory.join("sandbox.toml")).unwrap();
    let status = run(&["show"]);
    // Status reports the existing, stopped agent even though its model choice was deleted.
    assert_eq!(status.status.code(), Some(1), "{status:?}");
    assert!(!String::from_utf8_lossy(&status.stderr).contains("There is no agent yet"));
    let restored: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(restored.get("id"), before.get("id"));
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("extends =")
    );
    let grants = link_agent::sandbox_file::Sandbox::parse(
        &std::fs::read_to_string(directory.join("sandbox.toml")).unwrap(),
    )
    .unwrap();
    assert_eq!(grants, link_agent::sandbox_file::Sandbox::none());
    assert_eq!(
        std::fs::read(home.join(".lnk/safe/secrets/secrets.toml")).unwrap(),
        credentials
    );
    assert!(String::from_utf8_lossy(&status.stdout).contains("Sandbox file"));
}

#[test]
fn a_tool_cannot_expand_a_closed_sandbox_without_confirmation() {
    let home = TempDir::new("tool-grant-confirmation");
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let config = home.join("Link/Safe/Agents/main/agent.toml");
    let path = home.join("Link/Safe/Agents/main/sandbox.toml");
    let closed = link_agent::sandbox_file::Sandbox::none()
        .text("main")
        .unwrap();
    std::fs::write(&path, &closed).unwrap();
    let before = std::fs::read(&config).unwrap();
    let run = |yes: bool| {
        let mut command = env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home);
        command
            .args(["tool", "add", "file"])
            .env("LNK_SERVICES", "off");
        if yes {
            command.arg("--yes");
        }
        command.output().unwrap()
    };
    let refused = run(false);
    assert!(!refused.status.success(), "{refused:?}");
    assert_eq!(std::fs::read(&config).unwrap(), before);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), closed);
    let approved = run(true);
    assert!(approved.status.success(), "{approved:?}");
    let sandbox =
        link_agent::sandbox_file::Sandbox::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(sandbox.folders().write, ["work", "instructions", "skills"]);
    assert!(sandbox.spec.internet.is_none() && sandbox.commands().is_none());
    let actual: link_agent::settings::Settings =
        link_files::saved::parse(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(actual.behavior.unwrap().tool("file").enabled, Some(true));
}

#[test]
fn model_set_persists_an_exact_id_and_refuses_a_typo_without_changes() {
    let root = TempDir::new("owner-model");
    let home = root.join("home");
    let core = root.join("lnk");
    link_test_kit::write_core(&core, "#!/bin/sh\nprintf '[]\\n'\n").unwrap();
    link_agent_contract::testing::write_agent(&home, "main", "harness = \"link\"\n");
    let path = home.join("Link/Safe/Agents/main/agent.toml");
    let initial = std::fs::read(&path).unwrap();
    let run = |id: &str| {
        env::command(env!("CARGO_BIN_EXE_lnk-agent"), &home)
            .args(["model", "set", id])
            .env("LNK_EXE", &core)
            .env("LNK_SERVICES", "off")
            .env("ANTHROPIC_API_KEY", "sk-ant-test-owner-model")
            .output()
            .unwrap()
    };
    let bad = run("claude-haiku-5.5");
    assert_eq!(bad.status.code(), Some(2), "{bad:?}");
    assert!(String::from_utf8_lossy(&bad.stderr).contains("claude-haiku-5-5"));
    assert_eq!(std::fs::read(&path).unwrap(), initial);
    let good = run("claude-haiku-5-5");
    assert!(good.status.success(), "{good:?}");
    let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(saved["model"]["name"].as_str(), Some("claude-haiku-5-5"));
    let after = std::fs::read(&path).unwrap();
    assert_eq!(run("claude-haiku-5.5").status.code(), Some(2));
    assert_eq!(std::fs::read(&path).unwrap(), after);
}
