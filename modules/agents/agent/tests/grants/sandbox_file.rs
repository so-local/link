//! What the sandbox keeps: its grants, spec and start unchanged from
//! inside.

use super::*;

/// Every file and link under `dir`, by its path, with its contents (a
/// link's, where it points): what decides what an agent may do and how it
/// starts. Not what running writes: logs, sockets, and the proxy's
/// traffic files.
fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut all = std::collections::BTreeMap::new();
    let mut left = vec![dir.to_path_buf()];
    while let Some(at) = left.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // What runs writes as it runs, and the sandbox's checks that
            // passed here, which Link keeps as it wraps.
            let running = path.extension().is_some_and(|e| e == "log")
                || path
                    .parent()
                    .is_some_and(|p| p.ends_with("sandboxes/traffic"))
                || path.ends_with("sandboxes/bwrap-version");
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => left.push(path),
                Ok(meta) if meta.is_symlink() => {
                    let to = std::fs::read_link(&path).unwrap_or_default();
                    all.insert(path, to.into_os_string().into_encoded_bytes());
                }
                Ok(meta) if meta.is_file() && !running => {
                    all.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
                }
                _ => {}
            }
        }
    }
    all
}

/// No agent turns its own sandbox off, or another's: nothing inside the
/// sandbox reaches what decides it. A program in an agent's sandbox tries
/// every way at the agent's settings (where `--no-sandbox` is kept), its
/// spec, its sandbox file, its harness's grants, another agent's, the
/// machine's settings, the tools that run outside, the bridge's and the
/// records' files, the rules files, the services' units, timers and
/// autostart, systemd and D-Bus, and running `lnk` itself; none of them
/// changes, nor does anything else of Link's settings.
#[test]
#[expect(clippy::disallowed_methods, reason = "moves onto the testing kit")]
fn nothing_inside_the_sandbox_changes_its_grants_spec_or_start() {
    let world = World::new("inside");
    if world.skips() {
        return;
    }
    // Its script runs programs, so it asks for them.
    world.needs(
        r#"{"needs": [{"spec": {"network": {"mode": "host"}}, "why": "its websockets"},
                      {"spec": {"commands": {"toolchains": ["shell"]}}, "why": "its script"}]}"#,
    );
    let spec = world.root.join("spec.toml");
    std::fs::write(&spec, "schema = 1\n[turns]\nrounds = 3\n").unwrap();
    let spec = spec.display().to_string();
    ok(&world.start(
        "main",
        &[
            "create",
            "main",
            "--harness",
            "needy",
            "--yes",
            "--spec",
            &spec,
        ],
    ));
    ok(&world.start("other", &["create", "other", "--harness", "plain"]));
    ok(&world.lnk(&["-a", "main", "permission", "add", "--files.read", "~"]));
    let tool = Command::new(&world.core)
        .args(["sandbox", "tool", "add", "gh", "--", "/bin/true"])
        .env_clear()
        .env("HOME", &world.home)
        .env(
            "XDG_RUNTIME_DIR",
            link_test_kit::env::made_runtime(&world.home),
        )
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    ok(&tool);
    let config = world.home.join(".lnk");
    let lnk_config = config.clone();
    // Each agent's settings and spec, in its folder in the Link folder.
    let agents = world.home.join("Link/Safe/Agents");
    let owned = [
        agents.join("main/agent.toml"),
        agents.join("other/agent.toml"),
        agents.join("main/sandbox.toml"),
        agents.join("other/sandbox.toml"),
    ];
    let snapshot = |config: &Path| {
        let mut all = snapshot(config);
        all.extend(snapshot(
            &link_files::layout::Home::Safe.at(&world.home.join("Link")),
        ));
        for file in &owned {
            if let Ok(contents) = std::fs::read(file) {
                all.insert(file.clone(), contents);
            }
        }
        all
    };
    for file in [
        owned[0].clone(),
        owned[2].clone(),
        world.sandbox("main"),
        world.grants("needy"),
        link_files::layout::Hidden::Installation.at(&lnk_config),
        link_files::layout::Hidden::Sandboxes
            .at(&lnk_config)
            .join("tools/gh.json"),
    ] {
        assert!(file.is_file(), "{} isn't there to try", file.display());
    }
    // Link's own program, put where the sandbox sees it (it hides where
    // it's built), before what's there is taken: it's the test's, not a
    // change from inside.
    let harness = world
        .home
        .join(".lnk/data/agents/main/harnesses/needy/lnk-agent");
    let built = env!("CARGO_BIN_EXE_lnk-agent");
    std::fs::hard_link(built, &harness)
        .or_else(|_| std::fs::copy(built, &harness).map(drop))
        .unwrap();
    let before = snapshot(&config);
    let permissions = ok(&world.lnk(&["-a", "main", "show", "--permissions"]));

    // Each way at each, from inside: written over, added to, removed,
    // moved, linked elsewhere, a file made beside it; and `lnk` run as
    // if at home.
    let targets = [
        owned[0].clone(),
        owned[1].clone(),
        owned[2].clone(),
        agents.join("main/sandbox.toml"),
        agents.join("other/sandbox.toml"),
        link_files::layout::Hidden::HarnessGrants
            .at(&lnk_config)
            .join("needy.toml"),
        link_files::layout::Hidden::HarnessGrants
            .at(&lnk_config)
            .join("plain.toml"),
        link_files::layout::Hidden::AgentPolicy.at(&lnk_config),
        link_files::layout::Home::TeamSpecs
            .at(&world.home.join("Link"))
            .join("all.toml"),
        link_files::layout::Hidden::AgentPolicy.at(&lnk_config),
        link_files::layout::Hidden::SecretPreferences.at(&lnk_config),
        link_files::layout::Home::Models.at(&world.home.join("Link")),
        link_files::layout::Hidden::ModelFacts.at(&lnk_config),
        link_files::layout::Hidden::Sandboxes
            .at(&lnk_config)
            .join("tools/gh.json"),
        link_files::layout::Hidden::Sandboxes
            .at(&lnk_config)
            .join("tools/memory-planted.json"),
        link_files::layout::Hidden::Sandboxes
            .at(&lnk_config)
            .join("main-needy.sb"),
        link_files::layout::Hidden::Agents
            .at(&lnk_config)
            .join("main/bridge.json"),
        link_files::layout::Hidden::Agents
            .at(&lnk_config)
            .join("main/memory.json"),
        link_files::layout::Hidden::Agents
            .at(&lnk_config)
            .join("main/scheduler.json"),
        world
            .home
            .join(".config/systemd/user/lnk-agent-main.service"),
        world.home.join(".config/systemd/user/lnk-planted.timer"),
        world
            .home
            .join(".config/systemd/user/default.target.wants/lnk-planted.service"),
        world.home.join(".config/autostart/lnk-planted.desktop"),
    ];
    let mut script = String::from("set +e\n");
    for target in &targets {
        let t = target.display();
        let dir = target.parent().unwrap().display();
        script.push_str(&format!(
            "echo 'unsandboxed = true' > '{t}'\necho 'sandbox = false' >> '{t}'\n\
             mv '{t}' '{t}.gone'\nrm -f '{t}'\nln -sf /tmp '{t}'\nmkdir -p '{dir}'\n\
             touch '{dir}/planted.toml' '{dir}/planted.json'\n"
        ));
    }
    // The services' manager and the session's bus.
    script.push_str(
        "systemctl --user daemon-reload\nsystemctl --user start lnk-planted.timer\n\
         systemd-run --user /bin/true\n\
         dbus-send --session --dest=org.freedesktop.systemd1 /org/freedesktop/systemd1 \
         org.freedesktop.systemd1.Manager.Reload\n",
    );
    // Link's own program, at home (put there above), where the sandbox
    // writes: never run, so it changes nothing.
    let harness = harness.display();
    let home = world.home.display();
    for args in [
        "harness-permission add --network.mode host --harness needy",
        "-a main permission add --network.mode host",
        "-a main permission add --browser.hosts '*'",
        "-a main permission add --tools.names gh",
        "-a other start --no-sandbox",
        "-a main start --no-sandbox --yes",
    ] {
        script.push_str(&format!(
            "HOME='{home}' '{harness}' {args} </dev/null 2>&1\n\
             echo \"lnk ran: $?\"\n"
        ));
    }
    script.push_str("echo tried\n");
    let ran = world.lnk(&[
        "-a",
        "main",
        "harness",
        "run",
        "--harness",
        "needy",
        "--",
        "/bin/sh",
        "-c",
        &script,
    ]);
    let said = String::from_utf8_lossy(&ran.stdout);
    assert!(said.contains("tried"), "{}", text(&ran));
    // Each `lnk` is refused before it runs: a program in a folder the
    // sandbox writes is never one it may run, whatever it asks for.
    let codes: Vec<&str> = said
        .lines()
        .filter_map(|line| line.strip_prefix("lnk ran: "))
        .collect();
    assert!(
        codes.len() == 6 && codes.iter().all(|c| *c == "126"),
        "{said}"
    );

    // Nothing of Link's settings changed, nor what it shows.
    let after = snapshot(&config);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "a file came or went"
    );
    for (path, contents) in &before {
        assert!(
            after.get(path) == Some(contents),
            "{} changed from inside the sandbox",
            path.display()
        );
    }
    assert_eq!(
        ok(&world.lnk(&["-a", "main", "show", "--permissions"])),
        permissions
    );
}
