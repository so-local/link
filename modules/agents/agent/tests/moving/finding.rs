//! Where a move goes: the box a chat reaches, the box a cloud offers or
//! starts, and the route a box's agents take.

use super::*;

/// `lnk agent chat send` goes where the agent is: for one on a box, through
/// `lnk box run`, answered by the box's own `lnk`. There, the fake
/// harness, which answers no endpoint, is refused, saying what to do.
#[test]
fn a_chat_goes_to_the_box_the_agent_is_on() {
    let world = World::new("chat");
    an_agent_here(&world);
    let output = world.lnk_agent(&["chat", "send", "--message", "Hello"]);
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        said.contains("fake speaks its own connections, and has no endpoint")
            && said.contains("lnk agent harness set link"),
        "{said}"
    );
    ok(&world.lnk_agent(&["move", "--on", "b1", "--no-start"]));
    let output = world.lnk_agent(&["chat", "send", "--chat", "qa", "--message", "Hello"]);
    assert!(!output.status.success());
    let there = String::from_utf8_lossy(&output.stderr).to_string();
    // The message went on stdin, never on a command line there.
    let ran = std::fs::read_to_string(world.root.join("ran-there")).unwrap();
    let chat = ran.lines().last().unwrap();
    assert_eq!(chat, "box run b1 -- agent -a main chat send --chat qa");
    assert!(!ran.contains("Hello"), "{ran}");
    // Here, it would say where the agent runs: the box answered.
    assert!(
        there.contains("fake speaks its own connections, and has no endpoint")
            && !there.contains("runs on your box"),
        "{there}"
    );
}

/// A harness with no endpoint is refused by `lnk agent chat send` while it
/// runs too: its plan holds an empty endpoint, never an address.
#[test]
fn a_chat_with_a_running_harness_that_has_no_endpoint_names_its_connections() {
    let world = World::new("chat-running");
    an_agent_here(&world);
    write(
        &world.here.join(".config/lnk/agents/main/bridge.json"),
        r#"{"endpoint": "", "key": "k", "connections": ["telegram"]}"#,
    );
    let output = world.lnk_agent(&["chat", "send", "--message", "Hello"]);
    assert!(!output.status.success());
    let said = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        said.contains("fake speaks its own connections, and has no endpoint")
            && !said.contains("loopback"),
        "{said}"
    );
}

#[test]
fn a_cloud_with_one_box_moves_the_agent_there() {
    let world = World::new("cloud-one");
    an_agent_here(&world);
    ok(&world.lnk_agent(&["move", "--cloud", "aws", "--no-start"]));
    let there =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(!there.contains("sk-ant-test"), "{there}");
    assert_eq!(
        crate::common::kept_key(
            &world.there,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
    assert!(!world.root.join("started").exists());
}

/// A box in a cloud account found only by its tags, which anyone who can
/// tag machines in a shared account can plant, isn't taken for the
/// account's name: without a terminal to ask on, the move says to name
/// it. By its name, it moves there.
#[test]
fn a_box_found_only_by_its_tags_is_never_taken_unasked() {
    let world = World::new("cloud-found");
    an_agent_here(&world);
    write(&world.root.join("planted"), "");
    let refused = world.lnk_agent(&["move", "--cloud", "azure", "--no-start"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("c1 is a box this computer didn't make"),
        "{said}"
    );
    assert!(
        said.contains("lnk agent move --on c1 --agent main"),
        "{said}"
    );
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(!world.root.join("started").exists());
    ok(&world.lnk_agent(&["move", "--on", "c1", "--no-start"]));
    let there =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(!there.contains("sk-ant-test"), "{there}");
    assert_eq!(
        crate::common::kept_key(
            &world.there,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
}

#[test]
fn a_cloud_with_several_boxes_asks_which() {
    let world = World::new("cloud-several");
    an_agent_here(&world);
    let refused = world.lnk_agent(&["move", "--cloud", "gcp", "--no-start"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("2 boxes in gcp: b2, old"), "{said}");
    // Neither a box nor a cloud: refused too.
    let refused = world.lnk_agent(&["move", "--on", "nowhere", "--no-start"]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("no box named"), "{said}");
    // Nothing moved, nothing started.
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());
    assert!(!world.root.join("started").exists());
}

#[test]
fn a_cloud_with_no_box_starts_one_and_moves_there() {
    let world = World::new("cloud-none");
    an_agent_here(&world);
    let said = ok(&world.lnk_agent(&["move", "--cloud", "azure", "--no-start"]));
    assert!(
        said.contains("You have no box in azure: lnk box create azure makes one"),
        "{said}"
    );
    // A box is billed: without --yes, lnk box create asks before making it.
    assert_eq!(
        std::fs::read_to_string(world.root.join("started")).unwrap(),
        "box create --cloud azure\n"
    );
    let there =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(!there.contains("sk-ant-test"), "{there}");
    assert_eq!(
        crate::common::kept_key(
            &world.there,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
    assert!(said.contains("azure-1"), "{said}");
}

/// `--image` always starts a new box, from the image, in a cloud account,
/// even one with a box of yours already, and the agent moves there.
#[test]
fn an_image_starts_a_new_box_and_the_agent_moves_there() {
    let world = World::new("image");
    an_agent_here(&world);
    // A box isn't a cloud account: nothing started, nothing moved.
    let refused = world.lnk_agent(&["move", "--on", "b1", "--image", "web-ready", "--no-start"]);
    assert!(!refused.status.success());
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("--image starts a new box from your image, in a cloud account"),
        "{said}"
    );
    assert!(!world.root.join("started-aws").exists());
    assert!(world.here.join("Link/Safe/Agents/main/agent.toml").exists());

    let said = ok(&world.lnk_agent(&[
        "move",
        "--cloud",
        "aws",
        "--image",
        "web-ready",
        "--no-start",
        "--yes",
    ]));
    assert!(
        said.contains("A box in aws from your image web-ready"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(world.root.join("started-aws")).unwrap(),
        "box create --cloud aws --image web-ready --yes\n",
        "--yes to the move is its yes to the box"
    );
    assert!(said.contains("aws-2"), "{said}");
    let there =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(!there.contains("sk-ant-test"), "{there}");
    assert_eq!(
        crate::common::kept_key(
            &world.there,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-ant-test")
    );
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_local_model_needs_a_key_to_move_to_a_box() {
    let world = World::new("key");
    let here = &world.here;
    crate::common::write_agent(
        here,
        "main",
        &format!(
            "id = \"agent:{ID}\"\nharness = \"fake\"\nfiles = \"{}\"\n\n[model]\nprovider = \"ollama\"\napi_key = \"\"\n\
             name = \"qwen3:8b\"\nbase_url = \"http://localhost:9\"\n",
            world.files(here).display()
        ),
    );
    let refused = world.lnk_agent(&["move", "--on", "b1", "--no-start"]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("ANTHROPIC_API_KEY"));
    // Nothing moved, and it's still here.
    assert!(
        !world
            .there
            .join("Link/Safe/Agents/main/agent.toml")
            .exists()
    );
    assert!(here.join("Link/Safe/Agents/main/agent.toml").exists());

    let moved = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args(["move", "--on", "b1", "--no-start"])
        .env_clear()
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(here))
        .env("HOME", here)
        .env("XDG_RUNTIME_DIR", link_test_kit::env::made_runtime(here))
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .env("OPENROUTER_API_KEY", "sk-or-box-key")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    ok(&moved);
    let there =
        std::fs::read_to_string(world.there.join("Link/Safe/Agents/main/agent.toml")).unwrap();
    assert!(
        !there.contains("sk-or-box-key") && there.contains("openrouter"),
        "{there}"
    );
    assert_eq!(
        crate::common::kept_key(
            &world.there,
            "main",
            &link_agent_identity::own::OwnSecret::Model.to_use().unwrap()
        )
        .as_deref(),
        Some("sk-or-box-key")
    );
}

#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn a_new_box_routes_its_agents_through_the_proxy_unless_routed_already() {
    let world = World::new("route");
    let config = world.here.join(".lnk");
    let settings = world.here.join("Link/Safe/Agents/main/agent.toml");
    // A new box: the machine's default, and no agent made for it.
    ok(&world.lnk_agent(&["route", "--direct", "--default"]));
    let defaults = std::fs::read_to_string(
        link_files::layout::Home::AgentDefaults.at(&world.here.join("Link")),
    )
    .unwrap();
    assert!(defaults.contains("upstream = \"direct\""), "{defaults}");
    assert!(!settings.exists());
    assert!(!link_files::layout::Hidden::Agents.at(&config).exists());
    // An agent made here starts with it (its start then needs the sandbox,
    // which this one doesn't have: what's checked is what it was given).
    let files = world.here.join("Link/Agents/side/home");
    let new = Command::new(env!("CARGO_BIN_EXE_lnk-agent"))
        .args([
            "create",
            "side",
            "--harness",
            "fake",
            "--foreground",
            "--no-sandbox",
            "--run.files",
        ])
        .arg(&files)
        .env_clear()
        .env("LNK_SERVICES", "off")
        .envs(link_test_kit::keychain::isolated(&world.here))
        .env("HOME", &world.here)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.here),
        )
        .env("PATH", format!("{}:/usr/bin:/bin", world.bin.display()))
        .env("LNK_EXE", world.bin.join("lnk"))
        .env("ANTHROPIC_API_KEY", "sk-ant-test")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let side = std::fs::read_to_string(world.here.join("Link/Safe/Agents/side/agent.toml"))
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&new.stderr)));
    assert!(side.contains("upstream = \"direct\""), "{side}");
    // A route set already (this machine's exit) is never replaced by the default.
    crate::common::write_agent(
        &world.here,
        "main",
        "upstream = \"socks5://127.0.0.1:1080\"\n",
    );
    ok(&world.lnk_agent(&["route", "--direct", "--default"]));
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("socks5://127.0.0.1:1080"), "{text}");
    // Asked for plainly (`lnk agent route --direct`), it is.
    ok(&world.lnk_agent(&["-a", "main", "route", "--direct"]));
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(text.contains("upstream = \"direct\""), "{text}");
}
